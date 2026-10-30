//! LLVM IR backend for scalar and statically-shaped dense matrices.
use std::collections::HashMap;
use sun_ast::{BinaryOp, Expr, ExprKind, Module, StmtKind, UnaryOp};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error("unsupported construct for native compilation: {0}")]
    Unsupported(String),
    #[error("undefined variable '{0}'")]
    UndefinedVariable(String),
    #[error("matrix dimensions are incompatible for {0}")]
    IncompatibleDimensions(&'static str),
    #[error("matrix index ({row}, {col}) is outside the 1-based shape {rows}x{cols}")]
    IndexOutOfBounds { row: usize, col: usize, rows: usize, cols: usize },
}
#[derive(Clone)]
enum NativeValue { Scalar(String), Matrix { pointer: String, rows: usize, cols: usize } }
#[derive(Default)]
struct Generator { next_value: usize, variables: HashMap<String, NativeValue>, body: Vec<String> }
impl Generator {
    fn value(&mut self) -> String { let name=format!("%v{}",self.next_value);self.next_value+=1;name }
    fn emit(&mut self,line:String){self.body.push(format!("  {line}"));}
    fn scalar(&mut self,expr:&Expr)->Result<String,BackendError>{
        match self.expression(expr)? { NativeValue::Scalar(v)=>Ok(v), NativeValue::Matrix{..}=>Err(BackendError::Unsupported("expected a scalar expression".into())) }
    }
    fn expression(&mut self,expr:&Expr)->Result<NativeValue,BackendError>{
        match &expr.kind {
            ExprKind::Number(n)=>Ok(NativeValue::Scalar(format!("{:.17e}",n))),
            ExprKind::Bool(_)=>Err(BackendError::Unsupported("booleans are only available in the interpreter; use sun run".into())),
            ExprKind::IndexEnd=>Err(BackendError::Unsupported("end in matrix indices is only available in the interpreter; use sun run".into())),
            ExprKind::IndexAll|ExprKind::IndexRange{..}=>Err(BackendError::Unsupported("ranges and matrix slices are only available in the interpreter; use sun run".into())),
            ExprKind::Identifier(name) if name=="pause"&&!self.variables.contains_key(name)=>Err(BackendError::Unsupported("pause is only available in the interpreter; use sun run".into())),
            ExprKind::Identifier(name)=>self.variables.get(name).cloned().ok_or_else(||BackendError::UndefinedVariable(name.clone())),
            ExprKind::FieldAccess{..}=>Err(BackendError::Unsupported("exception field access is only available in the interpreter; use sun run".into())),
            ExprKind::Unary{op,operand}=>{
                let value=self.scalar(operand)?;
                match op{
                    UnaryOp::Positive=>Ok(NativeValue::Scalar(value)),
                    UnaryOp::Negative=>{let r=self.value();self.emit(format!("{r} = fneg double {value}"));Ok(NativeValue::Scalar(r))},
                    UnaryOp::Not=>Err(BackendError::Unsupported("logical negation".into())),
                }
            }
            ExprKind::Matrix(rows)=>{
                if rows.is_empty()||rows[0].is_empty(){return Err(BackendError::Unsupported("empty matrices are only available in the interpreter; use sun run".into()));}
                let cols=rows[0].len();
                if rows.iter().any(|r|r.len()!=cols){return Err(BackendError::Unsupported("matrix rows must have equal lengths".into()));}
                let pointer=self.value();
                self.emit(format!("{pointer} = call ptr @sun_matrix_create(i64 {}, i64 {})", rows.len(), cols));
                self.emit(format!("call void @sun_runtime_check_matrix(ptr {pointer})"));

                for (index,item) in rows.iter().flatten().enumerate(){
                    let v=self.scalar(item)?;
                    let row=index/cols+1; let col=index%cols+1;
                    self.emit(format!("call i32 @sun_matrix_set(ptr {pointer}, i64 {row}, i64 {col}, double {v})"));
                }
                Ok(NativeValue::Matrix{pointer,rows:rows.len(),cols})
            }
            ExprKind::Binary{left,op,right}=>{
                let a=self.expression(left)?;let b=self.expression(right)?;self.binary(a,*op,b)
            }
            ExprKind::Call{callee,arguments}=>{
                let ExprKind::Identifier(name)=&callee.kind else{return Err(BackendError::Unsupported("indirect calls".into()));};
                if matches!(name.as_str(),"plot"|"plot3"|"surf"|"title"|"xlabel"|"ylabel"|"zlabel"|"legend"|"grid"|"axis"|"clf"){
                    return Err(BackendError::Unsupported(format!("{name} requires a graphical interpreter host; use Sun Studio")));
                }
                if matches!(name.as_str(),"error"|"rethrow"|"pause"|"readmatrix"|"writematrix"){
                    return Err(BackendError::Unsupported(format!("{name} is only available in the interpreter; use sun run")));
                }
                if name=="disp"{
                    if arguments.len()!=1{return Err(BackendError::Unsupported("disp expects one argument".into()));}
                    let v=self.expression(&arguments[0])?;self.print(&v);return Ok(NativeValue::Scalar("0.0".into()));
                }
                let runtime_function=match name.as_str(){
                    "sqrt"=>Some("sun_rt_sqrt"),"sin"=>Some("sun_rt_sin"),
                    "cos"=>Some("sun_rt_cos"),"exp"=>Some("sun_rt_exp"),
                    "log"=>Some("sun_rt_log"),_=>None
                };
                if let Some(function)=runtime_function{
                    if arguments.len()!=1{return Err(BackendError::Unsupported(format!("{name} expects one argument")));}
                    let argument=self.scalar(&arguments[0])?;
                    let result=self.value();
                    self.emit(format!("{result} = call double @{function}(double {argument})"));
                    return Ok(NativeValue::Scalar(result));
                }
                if let Some(NativeValue::Matrix{pointer,rows,cols})=self.variables.get(name).cloned(){
                    if arguments.len()==1{return Err(BackendError::Unsupported("linear matrix indexing is only available in the interpreter; use sun run".into()));}
                    if arguments.len()!=2{return Err(BackendError::Unsupported("matrix indexing uses A(row, column)".into()));}
                    // Keep literal coordinates compile-time checked; evaluate all other
                    // index expressions at runtime and let the ABI validate their values.
                    if let (Ok(row), Ok(col)) = (Self::literal_index(&arguments[0]), Self::literal_index(&arguments[1])) {
                        if row==0||col==0||row>rows||col>cols{return Err(BackendError::IndexOutOfBounds{row,col,rows,cols});}
                        let status=self.value();self.emit(format!("{status} = alloca i32, align 4"));
                        let result=self.value();self.emit(format!("{result} = call double @sun_matrix_get(ptr {pointer}, i64 {row}, i64 {col}, ptr {status})"));
                        return Ok(NativeValue::Scalar(result));
                    }
                    let row=self.scalar(&arguments[0])?;
                    let col=self.scalar(&arguments[1])?;
                    let result=self.value();
                    self.emit(format!("{result} = call double @sun_matrix_get_indexed(ptr {pointer}, double {row}, double {col})"));
                    return Ok(NativeValue::Scalar(result));
                }
                Err(BackendError::Unsupported(format!("function '{name}' is not available in native compilation yet; use sun run")))
            }
            ExprKind::String(_)=>Err(BackendError::Unsupported("string literals".into())),
        }
    }
    fn literal_index(expr:&Expr)->Result<usize,BackendError>{
        match &expr.kind{ExprKind::Number(n) if n.is_finite()&&n.fract()==0.0&&*n>=0.0=>Ok(*n as usize),
            _=>Err(BackendError::Unsupported("native matrix indices must currently be non-negative integer literals".into()))}
    }
    fn binary(&mut self,a:NativeValue,op:BinaryOp,b:NativeValue)->Result<NativeValue,BackendError>{
        use BinaryOp::*;
        match (a,b) {
            (NativeValue::Scalar(x),NativeValue::Scalar(y)) => {
                let instruction=match op {
                    Add=>"fadd", Subtract=>"fsub", Multiply|ElementMultiply=>"fmul",
                    Divide|ElementDivide=>"fdiv",
                    Power=>return Err(BackendError::Unsupported("power operator".into())),
                    _=>return Err(BackendError::Unsupported(format!("operator {op:?}"))),
                };
                let r=self.value(); self.emit(format!("{r} = {instruction} double {x}, {y}"));
                Ok(NativeValue::Scalar(r))
            }
            (NativeValue::Matrix{pointer:a,rows:ar,cols:ac},NativeValue::Matrix{pointer:b,rows:br,cols:bc}) => {
                let (function,rows,cols)=match op {
                    Add|ElementMultiply => {
                        if ar!=br||ac!=bc { return Err(BackendError::IncompatibleDimensions(if op==Add {"matrix addition"} else {"element-wise multiplication"})); }
                        (if op==Add {"sun_matrix_add"} else {"sun_matrix_element_multiply"},ar,ac)
                    }
                    Multiply => {
                        if ac!=br { return Err(BackendError::IncompatibleDimensions("matrix multiplication")); }
                        ("sun_matrix_multiply",ar,bc)
                    }
                    _=>return Err(BackendError::Unsupported(format!("matrix operator {op:?}"))),
                };
                let out=self.value(); self.emit(format!("{out} = call ptr @{function}(ptr {a}, ptr {b})"));
                self.emit(format!("call void @sun_runtime_check_matrix(ptr {out})"));

                Ok(NativeValue::Matrix{pointer:out,rows,cols})
            }
            (NativeValue::Matrix{pointer,rows,cols},NativeValue::Scalar(s))|
            (NativeValue::Scalar(s),NativeValue::Matrix{pointer,rows,cols}) if matches!(op,Multiply|ElementMultiply) => {
                let out=self.value(); self.emit(format!("{out} = call ptr @sun_matrix_scale(ptr {pointer}, double {s})"));
                self.emit(format!("call void @sun_runtime_check_matrix(ptr {out})"));

                Ok(NativeValue::Matrix{pointer:out,rows,cols})
            }
            (NativeValue::Matrix{pointer,rows,cols},NativeValue::Scalar(s)) if matches!(op,Divide|ElementDivide) => {
                let out=self.value(); self.emit(format!("{out} = call ptr @sun_matrix_divide_scalar(ptr {pointer}, double {s})"));
                self.emit(format!("call void @sun_runtime_check_matrix(ptr {out})"));

                Ok(NativeValue::Matrix{pointer:out,rows,cols})
            }
            _=>Err(BackendError::Unsupported(format!("operator {op:?} for these operand types"))),
        }
    }
    fn print(&mut self,v:&NativeValue){
        match v{
            NativeValue::Scalar(n)=>self.emit(format!("call i32 (ptr, ...) @printf(ptr @.sun_fmt, double {n})")),
            NativeValue::Matrix{pointer,..}=>self.emit(format!("call void @sun_matrix_print(ptr {pointer})")),
        }
    }
}
pub fn compile_to_llvm_ir(module:&Module)->Result<String,BackendError>{
    let mut g=Generator::default();
    for stmt in &module.statements{match &stmt.kind{
        StmtKind::Function(_)=>return Err(BackendError::Unsupported("user functions are only available in the interpreter; use sun run".into())),
        StmtKind::ParFor{..}=>return Err(BackendError::Unsupported("parfor is only available in the interpreter; use sun run".into())),
        StmtKind::For{..}|StmtKind::ForEach{..}=>return Err(BackendError::Unsupported("for loops are only available in the interpreter; use sun run".into())),
        StmtKind::While{..}=>return Err(BackendError::Unsupported("while loops are only available in the interpreter; use sun run".into())),
        StmtKind::If{..}=>return Err(BackendError::Unsupported("if statements are only available in the interpreter; use sun run".into())),
        StmtKind::Switch{..}=>return Err(BackendError::Unsupported("switch statements are only available in the interpreter; use sun run".into())),
        StmtKind::Return=>return Err(BackendError::Unsupported("return statements are only available in the interpreter; use sun run".into())),
        StmtKind::Try{..}=>return Err(BackendError::Unsupported("try/catch statements are only available in the interpreter; use sun run".into())),
        StmtKind::LinearIndexAssign{..}=>return Err(BackendError::Unsupported("linear matrix indexing is only available in the interpreter; use sun run".into())),
        StmtKind::Break|StmtKind::Continue=>return Err(BackendError::Unsupported("loop control statements are only available in the interpreter; use sun run".into())),
        StmtKind::Assign{name,value}=>{let v=g.expression(value)?;g.variables.insert(name.clone(),v);}
        StmtKind::IndexAssign{name,row,col,value}=>{
            let Some(NativeValue::Matrix{pointer,rows,cols})=g.variables.get(name).cloned() else {
                return Err(BackendError::Unsupported(format!("indexed assignment target '{name}' must be a matrix variable")));
            };
            if let (Ok(r),Ok(c))=(Generator::literal_index(row),Generator::literal_index(col)) {
                if r==0||c==0||r>rows||c>cols { return Err(BackendError::IndexOutOfBounds{row:r,col:c,rows,cols}); }
            }
            let r=g.scalar(row)?;
            let c=g.scalar(col)?;
            let v=g.scalar(value)?;
            g.emit(format!("call void @sun_matrix_set_indexed(ptr {pointer}, double {r}, double {c}, double {v})"));
        }
        StmtKind::Expr(expr)=>{let v=g.expression(expr)?;let is_disp=matches!(&expr.kind,ExprKind::Call{callee,..}if matches!(&callee.kind,ExprKind::Identifier(n)if n=="disp"));if !stmt.suppress_output&&!is_disp{g.print(&v);}}
    }}
    let mut ir=String::from("; Generated by Sun\n@.sun_fmt = private unnamed_addr constant [4 x i8] c\"%g\\0A\\00\"\n@.sun_cell = private unnamed_addr constant [4 x i8] c\"%g \\00\"\ndeclare i32 @printf(ptr, ...)\ndeclare i32 @putchar(i32)\ndeclare double @sun_rt_sqrt(double)\ndeclare double @sun_rt_sin(double)\ndeclare double @sun_rt_cos(double)\ndeclare double @sun_rt_exp(double)\ndeclare double @sun_rt_log(double)\ndeclare ptr @sun_matrix_create(i64, i64)\ndeclare void @sun_matrix_free(ptr)\ndeclare void @sun_runtime_check_matrix(ptr)\ndeclare void @sun_runtime_cleanup()\ndeclare i32 @sun_matrix_set(ptr, i64, i64, double)\ndeclare void @sun_matrix_set_indexed(ptr, double, double, double)\ndeclare double @sun_matrix_get(ptr, i64, i64, ptr)\ndeclare double @sun_matrix_get_indexed(ptr, double, double)\ndeclare ptr @sun_matrix_add(ptr, ptr)\ndeclare ptr @sun_matrix_element_multiply(ptr, ptr)\ndeclare ptr @sun_matrix_multiply(ptr, ptr)\ndeclare ptr @sun_matrix_scale(ptr, double)\ndeclare ptr @sun_matrix_divide_scalar(ptr, double)\ndeclare void @sun_matrix_print(ptr)\n\ndefine i32 @main() {\nentry:\n");
    for line in g.body{ir.push_str(&line);ir.push('\n');}
    ir.push_str("  call void @sun_runtime_cleanup()\n");
    ir.push_str("  ret i32 0\n}\n");Ok(ir)
}
#[cfg(test)]
mod tests{
 use super::*;use sun_parser::parse;
 #[test]fn rejects_parfor_with_interpreter_guidance(){let ast=parse("parfor index = 1:3; output(index) = index; end").unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("parfor"));assert!(error.contains("sun run"));}
 #[test]fn rejects_empty_matrix_with_interpreter_guidance(){let ast=parse("A=[];").unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("empty matrices"));assert!(error.contains("sun run"));}
 #[test]fn rejects_shape_queries_in_native_backend(){for name in ["size","numel","isempty"]{let ast=parse(&format!("disp({name}([1]));")).unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains(name));assert!(error.contains("native compilation"));}}
 #[test]fn rejects_matrix_slices_with_interpreter_guidance(){for source in ["A=[1,2;3,4]; disp(A(:,1));","A=[1,2;3,4]; disp(A(1:end,:));","A=[1,2;3,4]; A(:,end)=0;","A=[1,2;3,4]; A(1:2,:)=A;"]{let ast=parse(source).unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("matrix slices"));assert!(error.contains("sun run"));}}
 #[test]fn rejects_linear_indexing_with_interpreter_guidance(){for source in ["A=[1,2;3,4]; disp(A(2));","A=[1,2;3,4]; disp(A(end));","A=[1,2;3,4]; A(end)=7;","A=[1,2;3,4]; disp(A(:));","A=[1,2;3,4]; disp(A(2:end));","A=[1,2;3,4]; A(:)=0;","A=[1,2;3,4]; A(1:end)=A;"]{let ast=parse(source).unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("linear matrix indexing"));assert!(error.contains("sun run"));}}
 #[test]fn rejects_index_end_with_interpreter_guidance(){for source in ["A=[1,2;3,4]; disp(A(end,1));","A=[1,2;3,4]; A(1,end)=7;"]{let ast=parse(source).unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("end in matrix indices"));assert!(error.contains("sun run"));}}
 #[test]fn rejects_pause_with_interpreter_guidance(){for source in ["pause(0);","pause();","pause;","pause(\"query\");"]{let ast=parse(source).unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("pause"));assert!(error.contains("sun run"));}}
 #[test]fn compiles_variable_named_pause(){let ast=parse("pause=7; disp(pause);").unwrap();assert!(compile_to_llvm_ir(&ast).is_ok());}
 #[test]fn rejects_exception_features_with_interpreter_guidance(){for source in ["try; catch failure; disp(failure.message); end","disp(failure.identifier);","error(\"failure\");","rethrow(failure);"]{let ast=parse(source).unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("interpreter"));assert!(error.contains("sun run"));}}
 #[test]fn rejects_try_catch_with_interpreter_guidance(){let ast=parse("try; catch; end").unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("try/catch statements"));assert!(error.contains("sun run"));}
 #[test]fn rejects_return_with_interpreter_guidance(){let ast=parse("return;").unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("return statements"));assert!(error.contains("sun run"));}
 #[test]fn rejects_switch_with_interpreter_guidance(){let ast=parse("switch 1; case 1; otherwise; end").unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("switch statements"));assert!(error.contains("sun run"));}
 #[test]fn rejects_for_with_interpreter_guidance(){for source in ["for index = 1:3; end","for column = [1,2;3,4]; end","for value = 7; end"]{let ast=parse(source).unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("for loops"));assert!(error.contains("sun run"));}}
 #[test]fn rejects_if_with_interpreter_guidance(){let ast=parse("if 0; elseif 1; else; end").unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("if statements"));assert!(error.contains("sun run"));}
 #[test]fn rejects_loop_control_in_direct_ast(){for keyword in ["break","continue"]{let mut ast=parse(&format!("while 1; {keyword}; end")).unwrap();let StmtKind::While{body,..}=ast.statements.remove(0).kind else{panic!("expected while")};let error=compile_to_llvm_ir(&Module{statements:body}).unwrap_err().to_string();assert!(error.contains("loop control statements"));assert!(error.contains("sun run"));}}
 #[test]fn rejects_while_with_interpreter_guidance(){let ast=parse("while 0\nend").unwrap();let error=compile_to_llvm_ir(&ast).unwrap_err().to_string();assert!(error.contains("while loops"));assert!(error.contains("sun run"));}
 #[test]fn compiles_matrix_literal_and_print(){let ast=parse("A=[1,2;3,4];\ndisp(A);").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("call ptr @sun_matrix_create(i64 2, i64 2)"));assert!(ir.contains("call void @sun_matrix_print"));assert!(ir.contains("call void @sun_runtime_check_matrix"));
 assert!(ir.contains("call void @sun_runtime_cleanup"));}
 #[test]fn matrix_aliases_and_reassignments_use_single_runtime_cleanup(){let ast=parse("A=[1,2;3,4]; B=A; A=A*A; disp(B); disp(A);").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert_eq!(ir.matches("call void @sun_runtime_cleanup()").count(),1);assert!(ir.matches("call void @sun_runtime_check_matrix").count()>=2);}
 #[test]fn compiles_matrix_multiplication(){let ast=parse("A=[1,2;3,4]; B=A*A; disp(B);").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("call ptr @sun_matrix_multiply"));}
 #[test]fn reads_matrix_elements_with_one_based_indices(){let ast=parse("A=[1,2;3,4]; disp(A(2,1));").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("call double @sun_matrix_get"));}
 #[test]fn compiles_indexed_matrix_assignment(){let ast=parse("A=[1,2;3,4]; i=2; j=1; A(i,j)=99; disp(A);").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("call void @sun_matrix_set_indexed"));assert!(ir.contains("call void @sun_matrix_print"));}
 #[test]fn rejects_out_of_bounds_literal_assignment(){let ast=parse("A=[1,2;3,4]; A(3,1)=9;").unwrap();assert!(compile_to_llvm_ir(&ast).unwrap_err().to_string().contains("outside the 1-based shape"));}
 #[test]fn compiles_dynamic_matrix_indices(){let ast=parse("A=[1,2;3,4]; i=2; j=1+0; disp(A(i,j));").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("call double @sun_matrix_get_indexed"));}
 #[test]fn lowers_math_functions_through_the_runtime_abi(){let ast=parse("x=sqrt(81); y=sin(0);").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("call double @sun_rt_sqrt"));assert!(ir.contains("call double @sun_rt_sin"));}
 #[test]fn rejects_zero_based_index(){let ast=parse("A=[1,2;3,4]; disp(A(0,1));").unwrap();assert!(compile_to_llvm_ir(&ast).unwrap_err().to_string().contains("1-based"));}
}

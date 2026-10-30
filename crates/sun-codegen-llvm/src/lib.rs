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
            ExprKind::Identifier(name)=>self.variables.get(name).cloned().ok_or_else(||BackendError::UndefinedVariable(name.clone())),
            ExprKind::Unary{op,operand}=>{
                let value=self.scalar(operand)?;
                match op{
                    UnaryOp::Positive=>Ok(NativeValue::Scalar(value)),
                    UnaryOp::Negative=>{let r=self.value();self.emit(format!("{r} = fneg double {value}"));Ok(NativeValue::Scalar(r))},
                    UnaryOp::Not=>Err(BackendError::Unsupported("logical negation".into())),
                }
            }
            ExprKind::Matrix(rows)=>{
                if rows.is_empty()||rows[0].is_empty(){return Err(BackendError::Unsupported("empty matrices".into()));}
                let cols=rows[0].len();
                if rows.iter().any(|r|r.len()!=cols){return Err(BackendError::Unsupported("matrix rows must have equal lengths".into()));}
                let count=rows.len()*cols;
                let pointer=self.value();self.emit(format!("{pointer} = alloca double, i64 {count}, align 8"));
                for (index,item) in rows.iter().flatten().enumerate(){
                    let v=self.scalar(item)?;let slot=self.value();
                    self.emit(format!("{slot} = getelementptr inbounds double, ptr {pointer}, i64 {index}"));
                    self.emit(format!("store double {v}, ptr {slot}, align 8"));
                }
                Ok(NativeValue::Matrix{pointer,rows:rows.len(),cols})
            }
            ExprKind::Binary{left,op,right}=>{
                let a=self.expression(left)?;let b=self.expression(right)?;self.binary(a,*op,b)
            }
            ExprKind::Call{callee,arguments}=>{
                let ExprKind::Identifier(name)=&callee.kind else{return Err(BackendError::Unsupported("indirect calls".into()));};
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
                    if arguments.len()!=2{return Err(BackendError::Unsupported("matrix indexing uses A(row, column)".into()));}
                    let row=Self::literal_index(&arguments[0])?;let col=Self::literal_index(&arguments[1])?;
                    if row==0||col==0||row>rows||col>cols{return Err(BackendError::IndexOutOfBounds{row,col,rows,cols});}
                    let index=(row-1)*cols+(col-1);let slot=self.value();self.emit(format!("{slot} = getelementptr inbounds double, ptr {pointer}, i64 {index}"));
                    let result=self.value();self.emit(format!("{result} = load double, ptr {slot}, align 8"));return Ok(NativeValue::Scalar(result));
                }
                Err(BackendError::Unsupported(format!("function '{name}' is not available in native compilation yet")))
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
        match(a,b){
            (NativeValue::Scalar(x),NativeValue::Scalar(y))=>{
                let instruction=match op{Add=>"fadd",Subtract=>"fsub",Multiply|ElementMultiply=>"fmul",Divide|ElementDivide=>"fdiv",Power=>return Err(BackendError::Unsupported("power operator".into())),_=>return Err(BackendError::Unsupported(format!("operator {op:?}")))};
                let r=self.value();self.emit(format!("{r} = {instruction} double {x}, {y}"));Ok(NativeValue::Scalar(r))
            }
            (NativeValue::Matrix{pointer:a,rows:ar,cols:ac},NativeValue::Matrix{pointer:b,rows:br,cols:bc})=>{
                match op{
                    Add|ElementMultiply=>{
                        if(ar!=br||ac!=bc){return Err(BackendError::IncompatibleDimensions(if op==Add{"matrix addition"}else{"element-wise multiplication"}));}
                        let out=self.value();let n=ar*ac;self.emit(format!("{out} = alloca double, i64 {n}, align 8"));
                        for i in 0..n{let x=self.load(&a,i);let y=self.load(&b,i);let z=self.value();self.emit(format!("{z} = {} double {x}, {y}",if op==Add{"fadd"}else{"fmul"}));self.store(&out,i,&z);}
                        Ok(NativeValue::Matrix{pointer:out,rows:ar,cols:ac})
                    }
                    Multiply=>{
                        if ac!=br{return Err(BackendError::IncompatibleDimensions("matrix multiplication"));}
                        let out=self.value();self.emit(format!("{out} = alloca double, i64 {}, align 8",ar*bc));
                        for i in 0..ar{for j in 0..bc{
                            let mut acc:Option<String>=None;
                            for k in 0..ac{let x=self.load(&a,i*ac+k);let y=self.load(&b,k*bc+j);let p=self.value();self.emit(format!("{p} = fmul double {x}, {y}"));
                                acc=Some(if let Some(prev)=acc{let s=self.value();self.emit(format!("{s} = fadd double {prev}, {p}"));s}else{p});}
                            self.store(&out,i*bc+j,&acc.unwrap_or_else(||"0.0".into()));
                        }}
                        Ok(NativeValue::Matrix{pointer:out,rows:ar,cols:bc})
                    }
                    _=>Err(BackendError::Unsupported(format!("matrix operator {op:?}"))),
                }
            }
            (NativeValue::Matrix{pointer,rows,cols},NativeValue::Scalar(s))|(NativeValue::Scalar(s),NativeValue::Matrix{pointer,rows,cols}) if matches!(op,Multiply|ElementMultiply)=>{
                let out=self.value();self.emit(format!("{out} = alloca double, i64 {}, align 8",rows*cols));
                for i in 0..rows*cols{let x=self.load(&pointer,i);let z=self.value();self.emit(format!("{z} = fmul double {x}, {s}"));self.store(&out,i,&z);}
                Ok(NativeValue::Matrix{pointer:out,rows,cols})
            }
            (NativeValue::Matrix{pointer,rows,cols},NativeValue::Scalar(s)) if matches!(op,Divide|ElementDivide)=>{
                let out=self.value();self.emit(format!("{out} = alloca double, i64 {}, align 8",rows*cols));
                for i in 0..rows*cols{let x=self.load(&pointer,i);let z=self.value();self.emit(format!("{z} = fdiv double {x}, {s}"));self.store(&out,i,&z);}
                Ok(NativeValue::Matrix{pointer:out,rows,cols})
            }
            _=>Err(BackendError::Unsupported(format!("operator {op:?} for these operand types"))),
        }
    }
    fn load(&mut self,p:&str,i:usize)->String{let slot=self.value();self.emit(format!("{slot} = getelementptr inbounds double, ptr {p}, i64 {i}"));let v=self.value();self.emit(format!("{v} = load double, ptr {slot}, align 8"));v}
    fn store(&mut self,p:&str,i:usize,v:&str){let slot=self.value();self.emit(format!("{slot} = getelementptr inbounds double, ptr {p}, i64 {i}"));self.emit(format!("store double {v}, ptr {slot}, align 8"));}
    fn print(&mut self,v:&NativeValue){
        match v{
            NativeValue::Scalar(n)=>self.emit(format!("call i32 (ptr, ...) @printf(ptr @.sun_fmt, double {n})")),
            NativeValue::Matrix{pointer,rows,cols}=>for i in 0..*rows{for j in 0..*cols{let x=self.load(pointer,i*cols+j);self.emit(format!("call i32 (ptr, ...) @printf(ptr @.sun_cell, double {x})"));}self.emit("call i32 @putchar(i32 10)".into());},
        }
    }
}
pub fn compile_to_llvm_ir(module:&Module)->Result<String,BackendError>{
    let mut g=Generator::default();
    for stmt in &module.statements{match &stmt.kind{
        StmtKind::Assign{name,value}=>{let v=g.expression(value)?;g.variables.insert(name.clone(),v);}
        StmtKind::Expr(expr)=>{let v=g.expression(expr)?;let is_disp=matches!(&expr.kind,ExprKind::Call{callee,..}if matches!(&callee.kind,ExprKind::Identifier(n)if n=="disp"));if !stmt.suppress_output&&!is_disp{g.print(&v);}}
    }}
    let mut ir=String::from("; Generated by Sun\n@.sun_fmt = private unnamed_addr constant [4 x i8] c\"%g\\0A\\00\"\n@.sun_cell = private unnamed_addr constant [4 x i8] c\"%g \\00\"\ndeclare i32 @printf(ptr, ...)\ndeclare i32 @putchar(i32)\ndeclare double @sun_rt_sqrt(double)\ndeclare double @sun_rt_sin(double)\ndeclare double @sun_rt_cos(double)\ndeclare double @sun_rt_exp(double)\ndeclare double @sun_rt_log(double)\n\ndefine i32 @main() {\nentry:\n");
    for line in g.body{ir.push_str(&line);ir.push('\n');}ir.push_str("  ret i32 0\n}\n");Ok(ir)
}
#[cfg(test)]
mod tests{
 use super::*;use sun_parser::parse;
 #[test]fn compiles_matrix_literal_and_print(){let ast=parse("A=[1,2;3,4];\ndisp(A);").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("alloca double, i64 4"));assert!(ir.contains("@.sun_cell"));}
 #[test]fn compiles_matrix_multiplication(){let ast=parse("A=[1,2;3,4]; B=A*A; disp(B);").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("fadd double"));assert!(ir.contains("fmul double"));}
 #[test]fn reads_matrix_elements_with_one_based_indices(){let ast=parse("A=[1,2;3,4]; disp(A(2,1));").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("load double"));}
 #[test]fn lowers_math_functions_through_the_runtime_abi(){let ast=parse("x=sqrt(81); y=sin(0);").unwrap();let ir=compile_to_llvm_ir(&ast).unwrap();assert!(ir.contains("call double @sun_rt_sqrt"));assert!(ir.contains("call double @sun_rt_sin"));}
 #[test]fn rejects_zero_based_index(){let ast=parse("A=[1,2;3,4]; disp(A(0,1));").unwrap();assert!(compile_to_llvm_ir(&ast).unwrap_err().to_string().contains("1-based"));}
}

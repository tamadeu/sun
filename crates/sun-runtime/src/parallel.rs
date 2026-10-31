use std::{collections::{BTreeMap, BTreeSet, HashMap}, io, time::Duration};
use rayon::prelude::*;
use sun_ast::{Expr, ExprKind, FunctionDefinition, Module, Stmt, StmtKind};
use sun_numeric::{MatrixError, SunMatrix};
use super::{eval, index_number, ExecutionContext, NumericRange, RuntimeError, RuntimeHost, Value};

const PURE_FUNCTIONS: &[&str] = &["keywords", "constants", "builtins", "complex", "real", "imag", "conj", "sqrt", "sin", "cos", "tan", "abs", "exp", "log", "sum", "mean", "min", "max", "zeros", "ones", "eye", "transpose", "det", "inv", "solve", "size", "numel", "isempty"];
const EFFECT_FUNCTIONS: &[&str] = &["disp", "pause", "error", "rethrow", "readmatrix", "writematrix", "plot", "plot3", "surf", "title", "xlabel", "ylabel", "zlabel", "legend", "grid", "axis", "clf"];

fn invalid(message: impl Into<String>) -> RuntimeError { RuntimeError::InvalidParFor(message.into()) }

#[derive(Clone, Copy, PartialEq, Eq)]
enum OutputIndex { Linear, Row(usize), Column(usize) }

impl OutputIndex {
    fn check(self, matrix: &SunMatrix, index: usize) -> Result<(), MatrixError> {
        match self {
            Self::Linear => matrix.get_linear(index),
            Self::Row(col) => matrix.get(index, col),
            Self::Column(row) => matrix.get(row, index),
        }.map(|_| ())
    }

    fn write(self, matrix: &mut SunMatrix, index: usize, value: f64) -> Result<(), MatrixError> {
        match self {
            Self::Linear => matrix.set_linear(index, value),
            Self::Row(col) => matrix.set(index, col, value),
            Self::Column(row) => matrix.set(row, index, value),
        }
    }
}

struct Output { name: String, index: OutputIndex }
enum Instruction { Assign { name: String, value: Expr }, Write { target: usize, value: Expr } }
struct Write { target: usize, index: usize, value: f64 }

#[derive(Default)]
struct Inputs {
    required: BTreeSet<String>,
    optional: BTreeSet<String>,
    matrix_calls: BTreeSet<String>,
}

impl Inputs {
    fn validate(&self, env: &HashMap<String, Value>, functions: &HashMap<String, std::sync::Arc<FunctionDefinition>>) -> Result<(), RuntimeError> {
        for name in self.optional.iter().chain(&self.matrix_calls) {
            if functions.contains_key(name) && !matches!(env.get(name), Some(Value::Matrix(_))) {
                return Err(invalid(format!("user function '{name}' is not supported inside parfor")));
            }
        }
        for name in &self.required {
            if !env.contains_key(name) { return Err(RuntimeError::UndefinedVariable(name.clone())); }
        }
        for name in &self.matrix_calls {
            if !matches!(env.get(name), Some(Value::Matrix(_))) {
                return Err(invalid(format!("'{name}' must be a read-only matrix or a supported pure function")));
            }
        }
        Ok(())
    }

    fn snapshot(&self, env: &HashMap<String, Value>) -> HashMap<String, Value> {
        self.required.union(&self.optional)
            .filter_map(|name| env.get(name).map(|value| (name.clone(), value.clone())))
            .collect()
    }
}

struct Plan {
    outputs: Vec<Output>,
    temporaries: BTreeSet<String>,
    inputs: Inputs,
    instructions: Vec<Instruction>,
}

fn is_iterator(expr: &Expr, iterator: &str) -> bool {
    matches!(&expr.kind, ExprKind::Identifier(name) if name == iterator)
}

fn literal_index(expr: &Expr) -> Option<usize> {
    match expr.kind {
        ExprKind::Number(number) if number.is_finite() && number.fract() == 0.0 && number >= 1.0 && number <= usize::MAX as f64 => Some(number as usize),
        _ => None,
    }
}

fn register_output(outputs: &mut BTreeMap<String, OutputIndex>, iterator: &str, name: &str, index: OutputIndex) -> Result<(), RuntimeError> {
    if name == iterator { return Err(invalid("the iteration variable cannot be assigned")); }
    if let Some(previous) = outputs.insert(name.into(), index) {
        if previous != index { return Err(invalid(format!("all writes to '{name}' must use the same indexing pattern"))); }
    }
    Ok(())
}

fn check_local(name: &str, iterator: &str, temporaries: &BTreeSet<String>, initialized: &BTreeSet<String>, outputs: &BTreeMap<String, OutputIndex>) -> Result<bool, RuntimeError> {
    if outputs.contains_key(name) { return Err(invalid(format!("output matrix '{name}' cannot be read inside parfor"))); }
    if name == iterator { return Ok(true); }
    if temporaries.contains(name) {
        if !initialized.contains(name) { return Err(invalid(format!("temporary '{name}' is read before assignment in the iteration"))); }
        return Ok(true);
    }
    Ok(false)
}

fn validate_expression(expr: &Expr, iterator: &str, temporaries: &BTreeSet<String>, initialized: &BTreeSet<String>, outputs: &BTreeMap<String, OutputIndex>, inputs: &mut Inputs) -> Result<(), RuntimeError> {
    match &expr.kind {
        ExprKind::Identifier(name) => {
            if !check_local(name, iterator, temporaries, initialized, outputs)? { inputs.required.insert(name.clone()); }
        }
        ExprKind::Call { callee, arguments } => {
            let ExprKind::Identifier(name) = &callee.kind else { return Err(invalid("indirect calls are not supported")); };
            if EFFECT_FUNCTIONS.contains(&name.as_str()) { return Err(invalid(format!("'{name}' is not allowed inside parfor"))); }
            if !check_local(name, iterator, temporaries, initialized, outputs)? {
                if PURE_FUNCTIONS.contains(&name.as_str()) { inputs.optional.insert(name.clone()); }
                else {
                    inputs.required.insert(name.clone());
                    inputs.matrix_calls.insert(name.clone());
                }
            }
            for argument in arguments { validate_expression(argument, iterator, temporaries, initialized, outputs, inputs)?; }
        }
        ExprKind::Unary { operand, .. } => validate_expression(operand, iterator, temporaries, initialized, outputs, inputs)?,
        ExprKind::Binary { left, right, .. } => {
            validate_expression(left, iterator, temporaries, initialized, outputs, inputs)?;
            validate_expression(right, iterator, temporaries, initialized, outputs, inputs)?;
        }
        ExprKind::FieldAccess { object, .. } => validate_expression(object, iterator, temporaries, initialized, outputs, inputs)?,
        ExprKind::Matrix(rows) => {
            for item in rows.iter().flatten() { validate_expression(item, iterator, temporaries, initialized, outputs, inputs)?; }
        }
        ExprKind::IndexRange { start, step, stop } => {
            validate_expression(start, iterator, temporaries, initialized, outputs, inputs)?;
            if let Some(step) = step { validate_expression(step, iterator, temporaries, initialized, outputs, inputs)?; }
            validate_expression(stop, iterator, temporaries, initialized, outputs, inputs)?;
        }
        ExprKind::Number(_) | ExprKind::Complex { .. } | ExprKind::Bool(_) | ExprKind::String(_) | ExprKind::IndexEnd | ExprKind::IndexAll => {}
    }
    Ok(())
}

impl Plan {
    fn analyze(iterator: &str, body: &[Stmt]) -> Result<Self, RuntimeError> {
        let mut outputs = BTreeMap::new();
        let mut temporaries = BTreeSet::new();
        for statement in body {
            match &statement.kind {
                StmtKind::Assign { name, .. } => {
                    if name == iterator { return Err(invalid("the iteration variable cannot be assigned")); }
                    if !statement.suppress_output { return Err(invalid("temporary assignments require a semicolon to suppress output")); }
                    temporaries.insert(name.clone());
                }
                StmtKind::LinearIndexAssign { name, index, .. } if is_iterator(index, iterator) => {
                    register_output(&mut outputs, iterator, name, OutputIndex::Linear)?;
                }
                StmtKind::IndexAssign { name, row, col, .. } => {
                    let index = if is_iterator(row, iterator) {
                        literal_index(col).map(OutputIndex::Row)
                    } else if is_iterator(col, iterator) { literal_index(row).map(OutputIndex::Column) }
                    else { None };
                    let index = index.ok_or_else(|| invalid("writes must use output(i), output(i, literal) or output(literal, i)"))?;
                    register_output(&mut outputs, iterator, name, index)?;
                }
                _ => return Err(invalid("only local assignments and scalar indexed writes are supported in the body")),
            }
        }
        if outputs.is_empty() { return Err(invalid("at least one indexed output matrix is required")); }
        if let Some(name) = temporaries.iter().find(|name| outputs.contains_key(*name)) {
            return Err(invalid(format!("'{name}' cannot be both an output matrix and a temporary")));
        }
        let mut inputs = Inputs::default();
        let mut initialized = BTreeSet::new();
        let mut instructions = Vec::new();
        let positions: BTreeMap<_, _> = outputs.keys().enumerate().map(|(position, name)| (name.clone(), position)).collect();
        for statement in body {
            match &statement.kind {
                StmtKind::Assign { name, value } => {
                    validate_expression(value, iterator, &temporaries, &initialized, &outputs, &mut inputs)?;
                    initialized.insert(name.clone());
                    instructions.push(Instruction::Assign { name: name.clone(), value: value.clone() });
                }
                StmtKind::IndexAssign { name, value, .. } | StmtKind::LinearIndexAssign { name, value, .. } => {
                    validate_expression(value, iterator, &temporaries, &initialized, &outputs, &mut inputs)?;
                    instructions.push(Instruction::Write { target: positions[name], value: value.clone() });
                }
                _ => unreachable!(),
            }
        }
        let outputs = outputs.into_iter().map(|(name, index)| Output { name, index }).collect();
        Ok(Self { outputs, temporaries, inputs, instructions })
    }
}

fn validate_header(start: &Expr, step: Option<&Expr>, stop: &Expr) -> Result<Inputs, RuntimeError> {
    let empty = BTreeSet::new();
    let outputs = BTreeMap::new();
    let mut inputs = Inputs::default();
    for expr in std::iter::once(start).chain(step).chain(std::iter::once(stop)) {
        validate_expression(expr, "", &empty, &empty, &outputs, &mut inputs)?;
    }
    Ok(inputs)
}

pub(super) fn validate_module(module: &Module) -> Result<(), RuntimeError> { validate_statements(&module.statements) }

fn validate_statements(statements: &[Stmt]) -> Result<(), RuntimeError> {
    for statement in statements {
        match &statement.kind {
            StmtKind::ParFor { name, start, step, stop, body } => {
                Plan::analyze(name, body)?;
                validate_header(start, step.as_ref(), stop)?;
            }
            StmtKind::Function(function) => validate_statements(&function.body)?,
            StmtKind::For { body, .. } | StmtKind::ForEach { body, .. } | StmtKind::While { body, .. } => validate_statements(body)?,
            StmtKind::If { branches, else_body } => {
                for (_, body) in branches { validate_statements(body)?; }
                validate_statements(else_body)?;
            }
            StmtKind::Switch { cases, otherwise_body, .. } => {
                for (_, body) in cases { validate_statements(body)?; }
                validate_statements(otherwise_body)?;
            }
            StmtKind::Try { body, catch_body, .. } => {
                validate_statements(body)?;
                validate_statements(catch_body)?;
            }
            _ => {}
        }
    }
    Ok(())
}

struct QuietHost;
impl RuntimeHost for QuietHost {
    fn read_file(&mut self, _path: &std::path::Path) -> io::Result<Box<dyn io::Read>> { Err(io::Error::other("parfor workers cannot read files")) }
    fn write_file_atomic(&mut self, _path: &std::path::Path, _contents: &[u8]) -> io::Result<()> { Err(io::Error::other("parfor workers cannot write files")) }
    fn write_line(&mut self, _line: &str) -> io::Result<()> { Err(io::Error::other("parfor workers cannot write output")) }
    fn pause(&mut self, _duration: Duration) -> io::Result<()> { Err(io::Error::other("parfor workers cannot pause")) }
    fn wait_for_input(&mut self) -> io::Result<()> { Err(io::Error::other("parfor workers cannot read input")) }
}

fn integer_bound(expr: &Expr, env: &HashMap<String, Value>, context: &mut ExecutionContext<'_>) -> Result<f64, RuntimeError> {
    match eval(expr, env, context)? {
        Value::Number(number) if number.is_finite() && number.fract() == 0.0 => Ok(number),
        _ => Err(invalid("range bounds and step must be finite scalar integers")),
    }
}

pub(super) fn execute(iterator: &str, start: &Expr, step: Option<&Expr>, stop: &Expr, body: &[Stmt], env: &mut HashMap<String, Value>, context: &mut ExecutionContext<'_>) -> Result<(), RuntimeError> {
    let plan = Plan::analyze(iterator, body)?;
    validate_header(start, step, stop)?.validate(env, context.functions)?;
    let start = integer_bound(start, env, context)?;
    let step = match step { Some(step) => integer_bound(step, env, context)?, None => 1.0 };
    let stop = integer_bound(stop, env, context)?;
    if step != 1.0 && step != -1.0 { return Err(invalid("step must be 1 or -1")); }
    let range = NumericRange::new(start, step, stop)?;
    plan.inputs.validate(env, context.functions)?;
    for name in &plan.temporaries {
        if env.contains_key(name) { return Err(invalid(format!("temporary '{name}' must not already exist outside parfor"))); }
    }
    for output in &plan.outputs {
        let matrix = match env.get(&output.name) {
            Some(Value::Matrix(matrix)) => matrix,
            Some(_) => return Err(invalid(format!("output '{}' must be a preallocated matrix", output.name))),
            None => return Err(RuntimeError::UndefinedVariable(output.name.clone())),
        };
        if range.length > 0 {
            for value in [range.value_at(0), range.value_at(range.length - 1)] {
                let index = index_number(Value::Number(value), "parallel")?;
                output.index.check(matrix, index)?;
            }
        }
    }
    if range.length == 0 { return Ok(()); }
    let length = usize::try_from(range.length).map_err(|_| invalid("range has too many iterations"))?;
    let workers = rayon::current_num_threads().min(length);
    context.check_cancelled()?;
    let cancelled = context.cancelled.clone();
    let functions = context.functions;
    let globals: &HashMap<String, Value> = env;
    let staged: Vec<Vec<Write>> = (0..workers).into_par_iter().map(|worker| -> Result<Vec<Write>, RuntimeError> {
        let mut locals = plan.inputs.snapshot(globals);
        let mut host = QuietHost;
        let mut graphics = super::graphics::GraphicsState::default();
        let sources = HashMap::new();
        let mut context = ExecutionContext { host: &mut host, pause_enabled: false, index_end: None, cancelled: cancelled.clone(), functions, call_depth: 0, graphics: &mut graphics,
            debug: super::debug::DebugState::default(), function_sources: &sources };
        let mut writes = Vec::new();
        for offset in (worker..length).step_by(workers) {
            context.check_cancelled()?;
            for temporary in &plan.temporaries { locals.remove(temporary); }
            let value = range.value_at(offset as u64);
            let index = index_number(Value::Number(value), "parallel")?;
            locals.insert(iterator.into(), Value::Number(value));
            for instruction in &plan.instructions {
                context.check_cancelled()?;
                match instruction {
                    Instruction::Assign { name, value } => {
                        let value = eval(value, &locals, &mut context)?;
                        locals.insert(name.clone(), value);
                    }
                    Instruction::Write { target, value } => {
                        let value = eval(value, &locals, &mut context)?;
                        let value = super::numeric::scalar(&value).ok_or_else(|| invalid("output values must be numeric scalars"))?;
                        writes.push(Write { target: *target, index, value });
                    }
                }
            }
        }
        Ok(writes)
    }).collect::<Result<_, RuntimeError>>()?;
    context.check_cancelled()?;
    for writes in staged {
        for write in writes {
            let output = &plan.outputs[write.target];
            let Some(Value::Matrix(matrix)) = env.get_mut(&output.name) else { unreachable!() };
            output.index.write(matrix, write.index, write.value)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(source: &str) -> Result<Vec<String>, RuntimeError> {
        super::super::execute(&sun_parser::parse(source).unwrap())
    }

    #[test]
    fn runs_parfor_example_with_two_threads() {
        let module = sun_parser::parse(include_str!("../../../examples/parfor.sun")).unwrap();
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
        assert_eq!(pool.install(|| super::super::execute(&module)).unwrap(), vec!["Quadrados por coluna:", "[1, 4, 9, 16]",
            "Somas por coluna:", "[11; 22; 33; 44]", "Soma dos quadrados apos parfor:", "30", "Entrada preservada:",
            "[1, 2, 3, 4; 10, 20, 30, 40]", "Sun:IndexOutOfBounds", "Saida apos falha:", "[0, 0, 0, 0]"]);
    }

    #[test]
    fn executes_independent_writes_in_an_explicit_two_thread_pool() {
        let module = sun_parser::parse("input = [2, 3, 4, 5]; output = [0, 0, 0, 0]; parfor index = 1:4; square = input(index) * input(index); output(index) = square; end; disp(output);").unwrap();
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
        assert_eq!(pool.install(|| super::super::execute(&module)).unwrap(), vec!["[4, 9, 16, 25]"]);
    }

    #[test]
    fn parallel_results_match_serial_for_on_read_only_matrix_slices() {
        let setup = "input = [1, 2, 3; 4, 5, 6]; output = [0, 0, 0];";
        let body = "temporary = sum(input(:, index)); output(index) = temporary * temporary;";
        let serial = format!("{setup} for index = 1:3; {body} end; disp(output); disp(input);");
        let parallel = format!("{setup} parfor index = 1:3; {body} end; disp(output); disp(input);");
        assert_eq!(run(&parallel).unwrap(), run(&serial).unwrap());
    }

    #[test]
    fn descending_ranges_support_multiple_independent_output_patterns() {
        let source = "index = 99; output = [0, 0, 0]; rows = [0; 0; 0]; columns = [0, 0, 0]; parfor index = 3:-1:1; output(index) = index; rows(index, 1) = index * index; columns(1, index) = index + 1; end; disp(output); disp(rows); disp(columns); disp(index);";
        assert_eq!(run(source).unwrap(), vec!["[1, 2, 3]", "[1; 4; 9]", "[2, 3, 4]", "99"]);
    }

    #[test]
    fn temporaries_are_local_and_multiple_writes_keep_iteration_order() {
        let source = "output = [0, 0, 0]; parfor index = 1:3; temporary = index; temporary = temporary + 1; output(index) = temporary; output(index) = temporary * 2; end; disp(output); try; disp(temporary); catch failure; disp(failure.identifier); end;";
        assert_eq!(run(source).unwrap(), vec!["[4, 6, 8]", "Sun:UndefinedVariable"]);
    }

    #[test]
    fn empty_ranges_do_not_assign_outputs_or_iteration_variable() {
        let source = "index = 99; output = []; parfor index = 1:0; output(index) = index; end; disp(output); disp(index);";
        assert_eq!(run(source).unwrap(), vec!["[]", "99"]);
    }

    #[test]
    fn pure_function_shadowing_by_read_only_matrix_keeps_normal_meaning() {
        let source = "sqrt = [10, 20, 30]; output = [0, 0, 0]; parfor index = 1:numel(output); output(index) = sqrt(index); end; disp(output);";
        assert_eq!(run(source).unwrap(), vec!["[10, 20, 30]"]);
    }

    #[test]
    fn rejects_dependencies_effects_and_unclassified_writes_statically() {
        for body in [
            "output(index) = output(index) + 1;",
            "output(index) = output(index - 1);",
            "total = total + index; output(index) = total;",
            "index = index + 1; output(index) = index;",
            "output(1) = index;",
            "output(index + 1) = index;",
            "output(index, index) = index;",
            "output(index, 1) = index; output(index, 2) = index;",
            "temporary = index\n output(index) = temporary;",
            "output(index) = pause(0);",
            "output(index) = disp(index);",
            "output(index) = error(\"failure\");",
            "output(index) = rethrow(failure);",
            "disp(index);",
            "continue;", "break;", "return;",
            "if 1; output(index) = index; end;",
            "for inner = 1:2; output(index) = index; end;",
            "parfor inner = 1:2; output(inner) = inner; end;",
            "try; output(index) = index; catch; end;",
            "output = index; output(index) = index;",
            "output(index) = temporary; temporary = index;",
        ] {
            let source = format!("output = [0, 0, 0]; parfor index = 1:3; {body} end;");
            let module = sun_parser::parse(&source).unwrap();
            assert!(matches!(validate_module(&module), Err(RuntimeError::InvalidParFor(_))), "unexpected result for {body}");
        }
    }

    #[test]
    fn static_validation_checks_unexecuted_blocks_before_script_side_effects() {
        struct RecordingHost { lines: Vec<String> }
        impl RuntimeHost for RecordingHost {
            fn write_line(&mut self, line: &str) -> io::Result<()> {
                self.lines.push(line.into());
                Ok(())
            }
        }
        let module = sun_parser::parse("disp(1); if 0; parfor index = 1:3; output(index) = output(index); end; end;").unwrap();
        let mut host = RecordingHost { lines: Vec::new() };
        assert!(matches!(super::super::execute_with_host(&module, &mut host), Err(RuntimeError::InvalidParFor(_))));
        assert!(host.lines.is_empty());
    }

    #[test]
    fn headers_reject_effectful_calls_without_running_them() {
        let module = sun_parser::parse("output = [0]; parfor index = pause(1):1; output(index) = index; end;").unwrap();
        assert!(matches!(validate_module(&module), Err(RuntimeError::InvalidParFor(_))));
    }

    #[test]
    fn rejects_noninteger_ranges_and_steps_other_than_one_or_minus_one() {
        for range in ["1:0:3", "1:2:3", "1:0.5:3", "1.5:3", "1:(1 / 0)", "1:[1]:3"] {
            let source = format!("output = [0, 0, 0]; try; parfor index = {range}; output(index) = index; end; catch failure; disp(failure.identifier); end; disp(output);");
            assert_eq!(run(&source).unwrap(), vec!["Sun:InvalidParFor", "[0, 0, 0]"]);
        }
    }

    #[test]
    fn output_bounds_are_checked_before_workers_modify_anything() {
        let source = "output = [0, 0, 0]; try; parfor index = 1:4; output(index) = index; end; catch failure; disp(failure.identifier); end; disp(output);";
        assert_eq!(run(source).unwrap(), vec!["Sun:IndexOutOfBounds", "[0, 0, 0]"]);
    }

    #[test]
    fn worker_errors_leave_all_output_matrices_unchanged() {
        let source = "input = [10, 20, 30]; first = [0, 0, 0]; second = [0, 0, 0]; try; parfor index = 1:3; first(index) = index; second(index) = input(index + 1); end; catch failure; disp(failure.identifier); end; disp(first); disp(second);";
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
        assert_eq!(pool.install(|| run(source)).unwrap(), vec!["Sun:IndexOutOfBounds", "[0, 0, 0]", "[0, 0, 0]"]);
    }

    #[test]
    fn worker_output_type_errors_do_not_commit_earlier_writes() {
        let source = "first = [0, 0]; second = [0, 0]; try; parfor index = 1:2; first(index) = index; second(index) = [index]; end; catch failure; disp(failure.identifier); end; disp(first); disp(second);";
        assert_eq!(run(source).unwrap(), vec!["Sun:InvalidParFor", "[0, 0]", "[0, 0]"]);
    }

    #[test]
    fn complex_outputs_do_not_commit_real_matrix_writes() {
        let source = "first = [0, 0]; second = [0, 0]; try; parfor index = 1:2; first(index) = index; second(index) = index * i; end; catch failure; disp(failure.identifier); end; disp(first); disp(second);";
        assert_eq!(run(source).unwrap(), vec!["Sun:InvalidParFor", "[0, 0]", "[0, 0]"]);
    }

    #[test]
    fn existing_variables_cannot_be_reclassified_as_temporaries() {
        let source = "temporary = 9; output = [0, 0]; try; parfor index = 1:2; temporary = index; output(index) = temporary; end; catch failure; disp(failure.identifier); end; disp(temporary); disp(output);";
        assert_eq!(run(source).unwrap(), vec!["Sun:InvalidParFor", "9", "[0, 0]"]);
    }

    #[test]
    fn inputs_and_outputs_must_have_valid_bindings() {
        for (setup, body, identifier) in [
            ("", "output(index) = index;", "Sun:UndefinedVariable"),
            ("output = 7;", "output(index) = index;", "Sun:InvalidParFor"),
            ("output = [0, 0];", "output(index) = missing(index);", "Sun:UndefinedVariable"),
            ("output = [0, 0]; scalar = 7;", "output(index) = scalar(index);", "Sun:InvalidParFor"),
        ] {
            let source = format!("{setup} try; parfor index = 1:2; {body} end; catch failure; disp(failure.identifier); end;");
            assert_eq!(run(&source).unwrap(), vec![identifier]);
        }
    }
}
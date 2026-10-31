//! Initial interpreter/runtime for Sun 0.1.

mod parallel;
mod numeric;
mod matrix_csv;
mod graphics;
mod graphics3d;
mod debug;

pub use debug::{DebugAction, DebugBreakpoint, DebugController, DebugFrame, DebugLocation, DebugNumber, DebugSnapshot, DebugSource, DebugStopReason, DebugVariable};
pub use graphics::{Figure, FigureEvent, FigureLayout, FigureTrace};
pub use graphics3d::{Curve3D, Scene3D};

use std::{collections::HashMap, io, sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};
use num_complex::Complex64;
use sun_ast::{BinaryOp, Expr, ExprKind, FunctionDefinition, Module, Stmt, StmtKind, UnaryOp};
use sun_numeric::{MatrixError, SunMatrix};
use thiserror::Error;

const MAX_MATRIX_ELEMENTS: u64 = 16_777_216;

const LANGUAGE_KEYWORDS: &[&str] = &["break", "case", "catch", "continue", "else", "elseif", "end", "false", "for", "function", "if", "otherwise", "parfor", "return", "switch", "true", "try", "while"];
const LANGUAGE_CONSTANTS: &[&str] = &["Inf", "NaN", "eps", "false", "flintmax", "i", "intmax", "intmin", "j", "pi", "realmax", "realmin", "true"];
const LANGUAGE_BUILTINS: &[&str] = &["abs", "axis", "builtins", "clf", "colormap", "complex", "conj", "constants", "cos", "det", "disp", "error", "exp", "eye", "grid", "imag", "isempty", "inv", "keywords", "legend", "log", "max", "mean", "min", "numel", "ones", "pause", "plot", "plot3", "readmatrix", "real", "rethrow", "shading", "sin", "size", "solve", "sqrt", "sum", "surf", "tan", "title", "transpose", "view", "writematrix", "xlabel", "ylabel", "zeros", "zlabel"];

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Complex(Complex64),
    Bool(bool),
    String(String),
    Matrix(SunMatrix),
    Exception(SunException),
    Void,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{identifier}: {message}")]
pub struct SunException {
    pub identifier: String,
    pub message: String,
}

fn display_number(number: f64, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if number.is_nan() { write!(formatter, "NaN") }
    else if number == f64::INFINITY { write!(formatter, "Inf") }
    else if number == f64::NEG_INFINITY { write!(formatter, "-Inf") }
    else if number.fract() == 0.0 && number.abs() < 1e15 { write!(formatter, "{number:.0}") }
    else { write!(formatter, "{number}") }
}

fn number_text(number: f64) -> String {
    struct Number(f64);
    impl std::fmt::Display for Number {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { display_number(self.0, formatter) }
    }
    Number(number).to_string()
}

fn display_complex(value: Complex64, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    if value.re == 0.0 {
        return match value.im {
            1.0 => write!(formatter, "i"),
            -1.0 => write!(formatter, "-i"),
            imaginary => write!(formatter, "{}i", number_text(imaginary)),
        };
    }
    let operator = if value.im.is_sign_negative() { '-' } else { '+' };
    write!(formatter, "{} {operator} {}i", number_text(value.re), number_text(value.im.abs()))
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bool(value) => write!(f, "{value}"),
            Self::Number(number) => display_number(*number, f),
            Self::Complex(value) => display_complex(*value, f),
            Self::String(s) => write!(f, "{s}"),
            Self::Matrix(matrix) if matrix.is_empty() => write!(f, "[]"),
            Self::Matrix(m) => {
                write!(f, "[")?;
                for (ri, row) in m.to_rows().iter().enumerate() {
                    if ri > 0 { write!(f, "; ")?; }
                    for (ci, value) in row.iter().enumerate() {
                        if ci > 0 { write!(f, ", ")?; }
                        display_number(*value, f)?;
                    }
                }
                write!(f, "]")
            }
            Self::Exception(exception) => write!(f, "{exception}"),
            Self::Void => Ok(()),
        }
    }
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("undefined variable '{0}'")]
    UndefinedVariable(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("function '{0}' expects {1}")]
    Arity(String, String),
    #[error("invalid for range: {0}")]
    InvalidForRange(String),
    #[error("invalid argument: {0}")]
    InvalidArgument(String),
    #[error("invalid parfor: {0}")]
    InvalidParFor(String),
    #[error("execution cancelled")]
    Cancelled,
    #[error("function '{0}' did not assign output '{1}'")]
    UnassignedOutput(String, String),
    #[error("function '{0}' does not return a value")]
    NoOutput(String),
    #[error("maximum function call depth of 32 exceeded")]
    RecursionLimit,
    #[error(transparent)]
    Exception(#[from] SunException),
    #[error("runtime I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("invalid numeric CSV: {0}")]
    Csv(String),
    #[error(transparent)]
    Matrix(#[from] MatrixError),
}

impl RuntimeError {
    pub fn identifier(&self) -> &str {
        match self {
            Self::UndefinedVariable(_) => "Sun:UndefinedVariable",
            Self::Unsupported(_) => "Sun:UnsupportedOperation",
            Self::Arity(_, _) => "Sun:InvalidArgumentCount",
            Self::InvalidForRange(_) => "Sun:InvalidForRange",
            Self::InvalidArgument(_) => "Sun:InvalidArgument",
            Self::InvalidParFor(_) => "Sun:InvalidParFor",
            Self::Cancelled => "Sun:Cancelled",
            Self::UnassignedOutput(_, _) => "Sun:UnassignedOutput",
            Self::NoOutput(_) => "Sun:NoOutput",
            Self::RecursionLimit => "Sun:RecursionLimit",
            Self::Io(_) => "Sun:IOError",
            Self::Csv(_) => "Sun:InvalidCSV",
            Self::Exception(exception) => &exception.identifier,
            Self::Matrix(error) => match error {
                MatrixError::SizeLimit => "Sun:MatrixSizeLimit",
                MatrixError::Empty => "Sun:EmptyMatrix",
                MatrixError::RaggedRows => "Sun:RaggedMatrixRows",
                MatrixError::IndexOutOfBounds { .. } | MatrixError::LinearIndexOutOfBounds { .. } => "Sun:IndexOutOfBounds",
                MatrixError::IncompatibleDimensions => "Sun:IncompatibleDimensions",
                MatrixError::SingularMatrix => "Sun:SingularMatrix",
            },
        }
    }

    pub fn to_exception(&self) -> SunException {
        match self {
            Self::Exception(exception) => exception.clone(),
            _ => SunException { identifier: self.identifier().into(), message: self.to_string() },
        }
    }
}

pub type CancellationFlag = Arc<AtomicBool>;

pub trait RuntimeHost {
    fn write_line(&mut self, line: &str) -> io::Result<()>;

    fn debug_enabled(&self) -> bool { false }

    fn debug_checkpoint(&mut self, _location: &DebugLocation, _depth: usize) -> io::Result<Option<DebugStopReason>> { Ok(None) }

    fn debug_pause(&mut self, _snapshot: &DebugSnapshot) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "debugging requires a debugger host"))
    }

    fn figure_event(&mut self, _event: &FigureEvent) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "figure output requires a graphical host; use Sun Studio"))
    }

    fn read_file(&mut self, path: &std::path::Path) -> io::Result<Box<dyn io::Read>> {
        if !std::fs::metadata(path)?.is_file() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "CSV input must be a regular file"));
        }
        Ok(Box::new(std::fs::File::open(path)?))
    }

    fn write_file_atomic(&mut self, path: &std::path::Path, contents: &[u8]) -> io::Result<()> {
        use io::Write;
        let cancelled = self.cancellation_flag();
        if cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "execution cancelled"));
        }
        let permissions = match std::fs::symlink_metadata(path) {
            Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => {
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "CSV output must not be a directory, special file or symbolic link"));
            }
            Ok(metadata) => Some(metadata.permissions()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        let parent = path.parent().filter(|parent| !parent.as_os_str().is_empty()).unwrap_or_else(|| std::path::Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
        temporary.write_all(contents)?;
        if let Some(permissions) = permissions { temporary.as_file().set_permissions(permissions)?; }
        temporary.as_file().sync_all()?;
        if cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "execution cancelled"));
        }
        temporary.persist(path).map_err(|error| error.error)?;
        Ok(())
    }

    fn cancellation_flag(&self) -> Option<CancellationFlag> { None }

    fn pause(&mut self, duration: Duration) -> io::Result<()> {
        if !duration.is_zero() { std::thread::sleep(duration); }
        Ok(())
    }

    fn wait_for_input(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "interactive pause requires a host with input support"))
    }
}

struct ExecutionContext<'a> {
    host: &'a mut dyn RuntimeHost,
    pause_enabled: bool,
    index_end: Option<usize>,
    cancelled: Option<CancellationFlag>,
    functions: &'a HashMap<String, Arc<FunctionDefinition>>,
    call_depth: usize,
    graphics: &'a mut graphics::GraphicsState,
    debug: debug::DebugState,
    function_sources: &'a HashMap<String, Arc<DebugSource>>,
}

impl ExecutionContext<'_> {
    fn debug_statement(&mut self, span: sun_source::Span, env: &HashMap<String, Value>) -> Result<(), RuntimeError> {
        if !self.debug.enabled { return Ok(()); }
        self.debug.span = span;
        let Some(source) = &self.debug.source else { return Ok(()); };
        let location = source.location(span);
        if let Some(reason) = self.host.debug_checkpoint(&location, self.call_depth)? {
            let mut frames = self.debug.callers.clone();
            if let Some(frame) = self.debug.frame(env) { frames.push(frame); }
            self.host.debug_pause(&DebugSnapshot { reason, frames, depth: self.call_depth })?;
            self.check_cancelled()?;
        }
        Ok(())
    }

    fn check_cancelled(&self) -> Result<(), RuntimeError> {
        if self.cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) { Err(RuntimeError::Cancelled) }
        else { Ok(()) }
    }

    fn write_line(&mut self, line: &str) -> io::Result<()> { self.host.write_line(line) }

    fn pause(&mut self, duration: Duration) -> io::Result<()> {
        if self.pause_enabled { self.host.pause(duration) } else { Ok(()) }
    }

    fn wait_for_input(&mut self) -> io::Result<()> {
        if self.pause_enabled { self.host.wait_for_input() } else { Ok(()) }
    }
}

struct CollectingHost { lines: Vec<String> }

impl RuntimeHost for CollectingHost {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        self.lines.push(line.to_string());
        Ok(())
    }
}

pub fn execute(module: &Module) -> Result<Vec<String>, RuntimeError> {
    let mut host = CollectingHost { lines: Vec::new() };
    execute_with_host(module, &mut host)?;
    Ok(host.lines)
}

pub fn validate_parallel_blocks(module: &Module) -> Result<(), RuntimeError> {
    parallel::validate_module(module)
}

pub struct ExecutionReport {
    pub variables: HashMap<String, Value>,
    pub result: Result<(), RuntimeError>,
}

pub struct Session {
    variables: HashMap<String, Value>,
    pause_enabled: bool,
    functions: HashMap<String, Arc<FunctionDefinition>>,
    graphics: graphics::GraphicsState,
    function_sources: HashMap<String, Arc<DebugSource>>,
}

impl Default for Session {
    fn default() -> Self { Self::new() }
}

impl Session {
    pub fn new() -> Self { Self { variables: HashMap::new(), pause_enabled: true, functions: HashMap::new(), graphics: graphics::GraphicsState::default(), function_sources: HashMap::new() } }

    pub fn variables(&self) -> &HashMap<String, Value> { &self.variables }

    pub fn functions(&self) -> impl Iterator<Item = &FunctionDefinition> { self.functions.values().map(Arc::as_ref) }

    pub fn figure(&self) -> Option<&Figure> { self.graphics.current.as_ref() }

    pub fn debug_sources(&self) -> impl Iterator<Item = &DebugSource> { self.function_sources.values().map(Arc::as_ref) }

    pub fn clear(&mut self) {
        self.variables.clear();
        self.pause_enabled = true;
        self.functions.clear();
        self.graphics = graphics::GraphicsState::default();
        self.function_sources.clear();
    }

    pub fn execute(&mut self, module: &Module, host: &mut dyn RuntimeHost) -> Result<(), RuntimeError> {
        self.execute_source(module, host, None)
    }

    pub fn execute_with_source(&mut self, module: &Module, host: &mut dyn RuntimeHost, source: DebugSource) -> Result<(), RuntimeError> {
        self.execute_source(module, host, Some(Arc::new(source)))
    }

    fn execute_source(&mut self, module: &Module, host: &mut dyn RuntimeHost, source: Option<Arc<DebugSource>>) -> Result<(), RuntimeError> {
        validate_parallel_blocks(module)?;
        let cancelled = host.cancellation_flag();
        if cancelled.as_ref().is_some_and(|flag| flag.load(Ordering::Acquire)) { return Err(RuntimeError::Cancelled); }
        for statement in &module.statements {
            if let StmtKind::Function(function) = &statement.kind {
                self.functions.insert(function.name.clone(), Arc::new(function.clone()));
                if let Some(source) = &source { self.function_sources.insert(function.name.clone(), source.clone()); }
                else { self.function_sources.remove(&function.name); }
            }
        }
        let debug = debug::DebugState { enabled: host.debug_enabled() && source.is_some(), source, ..debug::DebugState::default() };
        let mut context = ExecutionContext { host, pause_enabled: self.pause_enabled, index_end: None, cancelled,
            functions: &self.functions, call_depth: 0, graphics: &mut self.graphics, debug, function_sources: &self.function_sources };
        let result = execute_statements(&module.statements, &mut self.variables, &mut context).and_then(|flow| match flow {
            ExecutionFlow::Normal | ExecutionFlow::Return => Ok(()),
            ExecutionFlow::Break => Err(RuntimeError::Unsupported("break is only valid inside a loop".into())),
            ExecutionFlow::Continue => Err(RuntimeError::Unsupported("continue is only valid inside a loop".into())),
        });
        self.pause_enabled = context.pause_enabled;
        if context.check_cancelled().is_err() { Err(RuntimeError::Cancelled) } else { result }
    }
}

pub fn execute_with_host(module: &Module, host: &mut dyn RuntimeHost) -> Result<(), RuntimeError> {
    execute_with_report(module, host).result
}

pub fn execute_with_report(module: &Module, host: &mut dyn RuntimeHost) -> ExecutionReport {
    let mut session = Session::new();
    let result = session.execute(module, host);
    ExecutionReport { variables: session.variables, result }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExecutionFlow { Normal, Break, Continue, Return }

struct NumericRange {
    start: f64,
    step: f64,
    stop: f64,
    length: u64,
    includes_stop: bool,
}

impl NumericRange {
    fn new(start: f64, step: f64, stop: f64) -> Result<Self, RuntimeError> {
        if !start.is_finite() || !step.is_finite() || !stop.is_finite() {
            return Err(RuntimeError::InvalidForRange("start, step and stop must be finite".into()));
        }
        if step == 0.0 { return Err(RuntimeError::InvalidForRange("step must not be zero".into())); }
        if (step > 0.0 && start > stop) || (step < 0.0 && start < stop) {
            return Ok(Self { start, step, stop, length: 0, includes_stop: false });
        }
        let distance = stop - start;
        let quotient = if distance.is_finite() { distance / step } else { stop / step - start / step };
        if !quotient.is_finite() || quotient >= (1_u64 << 53) as f64 {
            return Err(RuntimeError::InvalidForRange("too many iterations for exact f64 iteration indices".into()));
        }
        let nearest = quotient.round();
        let tolerance = (4.0 * f64::EPSILON * quotient.abs().max(1.0)).min(0.25);
        let includes_stop = (nearest >= 1.0 && (quotient - nearest).abs() <= tolerance) || start == stop;
        let last_index = if includes_stop { nearest } else { quotient.floor() };
        let range = Self { start, step, stop, length: last_index as u64 + 1, includes_stop };
        if range.length > 1 && range.value_at(1) == start {
            return Err(RuntimeError::InvalidForRange("step is too small to advance at this magnitude".into()));
        }
        Ok(range)
    }

    fn value_at(&self, index: u64) -> f64 {
        if index == 0 { self.start }
        else if index + 1 == self.length && self.includes_stop { self.stop }
        else { self.step.mul_add(index as f64, self.start) }
    }
}

fn for_scalar(value: Value) -> Result<f64, RuntimeError> {
    match value {
        Value::Number(number) => Ok(number),
        other => Err(RuntimeError::InvalidForRange(format!("bounds must be scalar numbers, got {other}"))),
    }
}

fn scalar_condition(value: Value, context: &str) -> Result<bool, RuntimeError> {
    match value {
        Value::Bool(value) => Ok(value),
        Value::Number(number) if number.is_finite() => Ok(number != 0.0),
        other => Err(RuntimeError::Unsupported(format!("{context} condition must be a finite scalar number, got {other}"))),
    }
}

fn switch_value(value: Value, context: &str) -> Result<Value, RuntimeError> {
    match value {
        Value::Number(_) | Value::String(_) | Value::Bool(_) => Ok(value),
        other => Err(RuntimeError::Unsupported(format!("switch {context} must be a number or string, got {other}"))),
    }
}

fn execute_statements(statements: &[Stmt], env: &mut HashMap<String, Value>, output: &mut ExecutionContext<'_>) -> Result<ExecutionFlow, RuntimeError> {
    output.check_cancelled()?;
    for statement in statements {
        output.check_cancelled()?;
        if !matches!(statement.kind, StmtKind::Function(_) | StmtKind::While { .. }) { output.debug_statement(statement.span, env)?; }
        match &statement.kind {
            StmtKind::Function(_) => {}
            StmtKind::Assign { name, value } => {
                let result = eval(value, env, output)?;
                env.insert(name.clone(), result.clone());
                if !statement.suppress_output { output.write_line(&format!("{name} = {result}"))?; }
            }
            StmtKind::LinearIndexAssign { name, index, value } => {
                let shape = match env.get(name) {
                    Some(Value::Matrix(matrix)) => matrix.shape(),
                    Some(other) => return Err(RuntimeError::Unsupported(format!("cannot index-assign into non-matrix variable '{name}' ({other})"))),
                    None => return Err(RuntimeError::UndefinedVariable(name.clone())),
                };
                let index = evaluate_index(index, env, output, IndexAxis::Linear, shape)?;
                if let MatrixIndex::Selection(indices) = index {
                    let value = eval(value, env, output)?;
                    match env.get_mut(name) {
                        Some(Value::Matrix(matrix)) => match value {
                            Value::Number(number) => matrix.fill_linear_selection(&indices, number)?,
                            Value::Bool(value) => matrix.fill_linear_selection(&indices, u8::from(value) as f64)?,
                            Value::Matrix(values) => matrix.assign_linear_selection(&indices, &values)?,
                            other => return Err(RuntimeError::Unsupported(format!("linear slice assignment value must be a number or matrix, got {other}"))),
                        },
                        Some(other) => return Err(RuntimeError::Unsupported(format!("cannot index-assign into non-matrix variable '{name}' ({other})"))),
                        None => return Err(RuntimeError::UndefinedVariable(name.clone())),
                    }
                    continue;
                }
                let MatrixIndex::Scalar(index) = index else { unreachable!() };
                let value = match eval(value, env, output)? {
                    Value::Number(number) => number,
                    Value::Bool(value) => u8::from(value) as f64,
                    other => return Err(RuntimeError::Unsupported(format!("matrix assignment value must be a number, got {other}"))),
                };
                match env.get_mut(name) {
                    Some(Value::Matrix(matrix)) => matrix.set_linear(index, value)?,
                    Some(other) => return Err(RuntimeError::Unsupported(format!("cannot index-assign into non-matrix variable '{name}' ({other})"))),
                    None => return Err(RuntimeError::UndefinedVariable(name.clone())),
                }
            }
            StmtKind::IndexAssign { name, row, col, value } => {
                let (rows, cols) = match env.get(name) {
                    Some(Value::Matrix(matrix)) => matrix.shape(),
                    Some(other) => return Err(RuntimeError::Unsupported(format!("cannot index-assign into non-matrix variable '{name}' ({other})"))),
                    None => return Err(RuntimeError::UndefinedVariable(name.clone())),
                };
                let row = evaluate_index(row, env, output, IndexAxis::Row, (rows, cols))?;
                let col = evaluate_index(col, env, output, IndexAxis::Column, (rows, cols))?;
                if !matches!(row, MatrixIndex::Scalar(_)) || !matches!(col, MatrixIndex::Scalar(_)) {
                    let row_indices = row.into_indices();
                    let column_indices = col.into_indices();
                    let value = eval(value, env, output)?;
                    match env.get_mut(name) {
                        Some(Value::Matrix(matrix)) => match value {
                            Value::Number(number) => matrix.fill_selection(&row_indices, &column_indices, number)?,
                            Value::Bool(value) => matrix.fill_selection(&row_indices, &column_indices, u8::from(value) as f64)?,
                            Value::Matrix(values) => matrix.assign_selection(&row_indices, &column_indices, &values)?,
                            other => return Err(RuntimeError::Unsupported(format!("slice assignment value must be a number or matrix, got {other}"))),
                        },
                        Some(other) => return Err(RuntimeError::Unsupported(format!("cannot index-assign into non-matrix variable '{name}' ({other})"))),
                        None => return Err(RuntimeError::UndefinedVariable(name.clone())),
                    }
                    continue;
                }
                let MatrixIndex::Scalar(row) = row else { unreachable!() };
                let MatrixIndex::Scalar(col) = col else { unreachable!() };
                let value = match eval(value, env, output)? {
                    Value::Number(number) => number,
                    Value::Bool(value) => u8::from(value) as f64,
                    other => return Err(RuntimeError::Unsupported(format!("matrix assignment value must be a number, got {other}"))),
                };
                match env.get_mut(name) {
                    Some(Value::Matrix(matrix)) => matrix.set(row, col, value)?,
                    Some(other) => return Err(RuntimeError::Unsupported(format!("cannot index-assign into non-matrix variable '{name}' ({other})"))),
                    None => return Err(RuntimeError::UndefinedVariable(name.clone())),
                }
            }
            StmtKind::ParFor { name, start, step, stop, body } => {
                parallel::execute(name, start, step.as_ref(), stop, body, env, output)?;
            }
            StmtKind::For { name, start, step, stop, body } => {
                let start = for_scalar(eval(start, env, output)?)?;
                let step = match step {
                    Some(step) => for_scalar(eval(step, env, output)?)?,
                    None => 1.0,
                };
                let stop = for_scalar(eval(stop, env, output)?)?;
                let range = NumericRange::new(start, step, stop)?;
                let mut previous = None;
                for index in 0..range.length {
                    if index > 0 { output.debug_statement(statement.span, env)?; }
                    let value = range.value_at(index);
                    if !value.is_finite() || previous == Some(value) {
                        return Err(RuntimeError::InvalidForRange("step cannot produce distinct finite iteration values".into()));
                    }
                    previous = Some(value);
                    env.insert(name.clone(), Value::Number(value));
                    match execute_statements(body, env, output)? {
                        ExecutionFlow::Return => return Ok(ExecutionFlow::Return),
                        ExecutionFlow::Break => break,
                        ExecutionFlow::Normal | ExecutionFlow::Continue => {}
                    }
                }
            }
            StmtKind::ForEach { name, source, body } => {
                match eval(source, env, output)? {
                    value @ (Value::Number(_) | Value::Bool(_)) => {
                        env.insert(name.clone(), value);
                        match execute_statements(body, env, output)? {
                            ExecutionFlow::Return => return Ok(ExecutionFlow::Return),
                            ExecutionFlow::Normal | ExecutionFlow::Break | ExecutionFlow::Continue => {}
                        }
                    }
                    Value::Matrix(matrix) => {
                        if matrix.is_empty() { continue; }
                        let (rows, cols) = matrix.shape();
                        for column in 1..=cols {
                            if column > 1 { output.debug_statement(statement.span, env)?; }
                            let value = if rows == 1 {
                                Value::Number(matrix.get(1, column)?)
                            } else {
                                Value::Matrix(SunMatrix::from_rows(matrix.view(1, rows, column, column)?.to_rows())?)
                            };
                            env.insert(name.clone(), value);
                            match execute_statements(body, env, output)? {
                                ExecutionFlow::Return => return Ok(ExecutionFlow::Return),
                                ExecutionFlow::Break => break,
                                ExecutionFlow::Normal | ExecutionFlow::Continue => {}
                            }
                        }
                    }
                    other => return Err(RuntimeError::Unsupported(format!("for source must be a number or matrix, got {other}"))),
                }
            }
            StmtKind::While { condition, body } => {
                loop {
                    output.check_cancelled()?;
                    output.debug_statement(statement.span, env)?;
                    if !scalar_condition(eval(condition, env, output)?, "while")? { break; }
                    match execute_statements(body, env, output)? {
                        ExecutionFlow::Return => return Ok(ExecutionFlow::Return),
                        ExecutionFlow::Break => break,
                        ExecutionFlow::Normal | ExecutionFlow::Continue => {}
                    }
                }
            }
            StmtKind::If { branches, else_body } => {
                let mut selected_body = else_body.as_slice();
                for (index, (condition, body)) in branches.iter().enumerate() {
                    if index > 0 { output.debug_statement(condition.span, env)?; }
                    if scalar_condition(eval(condition, env, output)?, "if")? {
                        selected_body = body;
                        break;
                    }
                }
                let flow = execute_statements(selected_body, env, output)?;
                if flow != ExecutionFlow::Normal { return Ok(flow); }
            },
            StmtKind::Switch { selector, cases, otherwise_body } => {
                let selector = switch_value(eval(selector, env, output)?, "selector")?;
                let mut selected_body = otherwise_body.as_slice();
                for (case, body) in cases {
                    output.debug_statement(case.span, env)?;
                    let case = switch_value(eval(case, env, output)?, "case expression")?;
                    if selector == case {
                        selected_body = body;
                        break;
                    }
                }
                let flow = execute_statements(selected_body, env, output)?;
                if flow != ExecutionFlow::Normal { return Ok(flow); }
            }
            StmtKind::Try { body, catch_name, catch_body } => {
                let flow = match execute_statements(body, env, output) {
                    Ok(flow) => flow,
                    Err(error) => {
                        output.check_cancelled()?;
                        if let Some(name) = catch_name {
                            env.insert(name.clone(), Value::Exception(error.to_exception()));
                        }
                        execute_statements(catch_body, env, output)?
                    }
                };
                if flow != ExecutionFlow::Normal { return Ok(flow); }
            }
            StmtKind::Break => return Ok(ExecutionFlow::Break),
            StmtKind::Continue => return Ok(ExecutionFlow::Continue),
            StmtKind::Return => return Ok(ExecutionFlow::Return),
            StmtKind::Expr(expr) => {
                let result = eval_expression(expr, env, output, true)?;
                if !statement.suppress_output && result != Value::Void { output.write_line(&result.to_string())?; }
            }
        }
    }
    Ok(ExecutionFlow::Normal)
}

fn index_number(value: Value, axis: &str) -> Result<usize, RuntimeError> {
    match value {
        Value::Number(n) if n.is_finite() && n.fract() == 0.0 && n >= 1.0 && n <= usize::MAX as f64 => Ok(n as usize),
        other => Err(RuntimeError::Unsupported(format!("matrix {axis} index must be a positive integer, got {other}"))),
    }
}

#[derive(Clone, Copy)]
enum IndexAxis { Row, Column, Linear }

enum MatrixIndex { Scalar(usize), Selection(Vec<usize>) }

impl MatrixIndex {
    fn into_indices(self) -> Vec<usize> {
        match self { Self::Scalar(index) => vec![index], Self::Selection(indices) => indices }
    }
}

fn evaluate_index(expr: &Expr, env: &HashMap<String, Value>, output: &mut ExecutionContext<'_>, axis: IndexAxis, shape: (usize, usize)) -> Result<MatrixIndex, RuntimeError> {
    if matches!(expr.kind, ExprKind::IndexAll | ExprKind::IndexRange { .. }) {
        return Ok(MatrixIndex::Selection(eval_selection(expr, env, output, axis, shape)?));
    }
    let (label, dimension) = match axis {
        IndexAxis::Row => ("row", shape.0), IndexAxis::Column => ("column", shape.1), IndexAxis::Linear => ("linear", shape.0 * shape.1),
    };
    let previous = output.index_end.replace(dimension);
    let result = eval(expr, env, output);
    output.index_end = previous;
    match result? {
        Value::Matrix(matrix) => {
            if !matrix.is_empty() && matrix.shape().0 != 1 && matrix.shape().1 != 1 {
                return Err(RuntimeError::InvalidArgument("matrix indices must be numeric vectors".into()));
            }
            let mut indices = Vec::with_capacity(matrix.element_count());
            for offset in 1..=matrix.element_count() {
                let index = index_number(Value::Number(matrix.get_linear(offset)?), label)?;
                if index > dimension {
                    return Err(match axis {
                        IndexAxis::Row => MatrixError::IndexOutOfBounds { row: index, col: 1, rows: shape.0, cols: shape.1 },
                        IndexAxis::Column => MatrixError::IndexOutOfBounds { row: 1, col: index, rows: shape.0, cols: shape.1 },
                        IndexAxis::Linear => MatrixError::LinearIndexOutOfBounds { index, elements: dimension },
                    }.into());
                }
                indices.push(index);
            }
            Ok(MatrixIndex::Selection(indices))
        }
        value => Ok(MatrixIndex::Scalar(index_number(value, label)?)),
    }
}

fn slice_integer(value: Value, component: &str) -> Result<f64, RuntimeError> {
    match value {
        Value::Number(number) if number.is_finite() && number.fract() == 0.0 => Ok(number),
        _ => Err(RuntimeError::InvalidArgument(format!("slice {component} must be a finite integer"))),
    }
}

fn eval_selection(expr: &Expr, env: &HashMap<String, Value>, output: &mut ExecutionContext<'_>, axis: IndexAxis, shape: (usize, usize)) -> Result<Vec<usize>, RuntimeError> {
    let (label, dimension) = match axis {
        IndexAxis::Row => ("row", shape.0),
        IndexAxis::Column => ("column", shape.1),
        IndexAxis::Linear => ("linear", shape.0 * shape.1),
    };
    match &expr.kind {
        ExprKind::IndexAll => Ok((1..=dimension).collect()),
        ExprKind::IndexRange { start, step, stop } => {
            let previous = output.index_end.replace(dimension);
            let result = (|| -> Result<Vec<usize>, RuntimeError> {
                let start = slice_integer(eval(start, env, output)?, "start")?;
                let step = match step {
                    Some(step) => slice_integer(eval(step, env, output)?, "step")?,
                    None => 1.0,
                };
                let stop = slice_integer(eval(stop, env, output)?, "stop")?;
                if step == 0.0 { return Err(RuntimeError::InvalidArgument("slice step must not be zero".into())); }
                let range = NumericRange::new(start, step, stop).map_err(|_| {
                    RuntimeError::InvalidArgument("slice range is too large or cannot advance safely".into())
                })?;
                if range.length == 0 { return Ok(Vec::new()); }
                let checked_index = |number: f64| -> Result<usize, RuntimeError> {
                    let index = index_number(Value::Number(number), label)?;
                    if index > dimension {
                        let error = match axis {
                            IndexAxis::Row => MatrixError::IndexOutOfBounds { row: index, col: 1, rows: shape.0, cols: shape.1 },
                            IndexAxis::Column => MatrixError::IndexOutOfBounds { row: 1, col: index, rows: shape.0, cols: shape.1 },
                            IndexAxis::Linear => MatrixError::LinearIndexOutOfBounds { index, elements: dimension },
                        };
                        return Err(error.into());
                    }
                    Ok(index)
                };
                checked_index(range.value_at(0))?;
                checked_index(range.value_at(range.length - 1))?;
                if range.length > dimension as u64 {
                    return Err(RuntimeError::InvalidArgument("slice has too many indices for its dimension".into()));
                }
                let mut selected = Vec::with_capacity(range.length as usize);
                for offset in 0..range.length {
                    let index = checked_index(range.value_at(offset))?;
                    if selected.last() == Some(&index) {
                        return Err(RuntimeError::InvalidArgument("slice step does not produce distinct indices".into()));
                    }
                    selected.push(index);
                }
                Ok(selected)
            })();
            output.index_end = previous;
            result
        }
        _ => Err(RuntimeError::InvalidArgument("expected : or a range selector".into())),
    }
}

fn eval(expr: &Expr, env: &HashMap<String, Value>, output: &mut ExecutionContext<'_>) -> Result<Value, RuntimeError> {
    eval_expression(expr, env, output, false)
}

fn eval_expression(expr: &Expr, env: &HashMap<String, Value>, output: &mut ExecutionContext<'_>, allow_void: bool) -> Result<Value, RuntimeError> {
    match &expr.kind {
        ExprKind::Number(n) => Ok(Value::Number(*n)),
        ExprKind::Complex { real, imaginary } => Ok(Value::Complex(Complex64::new(*real, *imaginary))),
        ExprKind::Bool(value) => Ok(Value::Bool(*value)),
        ExprKind::String(s) => Ok(Value::String(s.clone())),
        ExprKind::IndexEnd => output.index_end.map(|dimension| Value::Number(dimension as f64))
            .ok_or_else(|| RuntimeError::InvalidArgument("end is only valid inside a matrix index".into())),
        ExprKind::IndexAll => Err(RuntimeError::InvalidArgument("bare : requires a direct matrix index".into())),
        ExprKind::IndexRange { start, step, stop } => {
            let start = for_scalar(eval(start, env, output)?)?;
            let step = match step { Some(step) => for_scalar(eval(step, env, output)?)?, None => 1.0 };
            let stop = for_scalar(eval(stop, env, output)?)?;
            let range = NumericRange::new(start, step, stop)?;
            if range.length > MAX_MATRIX_ELEMENTS {
                return Err(RuntimeError::InvalidArgument("range exceeds the 16777216-element matrix limit".into()));
            }
            let mut values = Vec::with_capacity(range.length as usize);
            for offset in 0..range.length {
                if offset % 4096 == 0 { output.check_cancelled()?; }
                let value = range.value_at(offset);
                if !value.is_finite() || values.last() == Some(&value) {
                    return Err(RuntimeError::InvalidForRange("step cannot produce distinct finite range values".into()));
                }
                values.push(value);
            }
            Ok(Value::Matrix(SunMatrix::from_rows(vec![values])?))
        }
        ExprKind::Identifier(name) => match env.get(name) {
            Some(value) => Ok(value.clone()),
            None if name == "pause" => call_function(name, Vec::new(), env, output, allow_void),
            None => Err(RuntimeError::UndefinedVariable(name.clone())),
        },
        ExprKind::FieldAccess { object, field } => {
            match eval(object, env, output)? {
                Value::Exception(exception) => match field.as_str() {
                    "message" => Ok(Value::String(exception.message)),
                    "identifier" => Ok(Value::String(exception.identifier)),
                    _ => Err(RuntimeError::InvalidArgument(format!("exception has no field '{field}'"))),
                },
                other => Err(RuntimeError::InvalidArgument(format!("field access requires an exception, got {other}"))),
            }
        }
        ExprKind::Matrix(rows) => {
            let mut row_blocks = Vec::with_capacity(rows.len());
            for row in rows {
                let mut blocks = Vec::with_capacity(row.len());
                for item in row {
                    match eval(item, env, output)? {
                        Value::Number(number) => blocks.push(SunMatrix::filled(1, 1, number)?),
                        Value::Bool(value) => blocks.push(SunMatrix::filled(1, 1, u8::from(value) as f64)?),
                        Value::Matrix(matrix) => blocks.push(matrix),
                        other => return Err(RuntimeError::Unsupported(format!("matrix elements must be numbers, got {other}"))),
                    }
                }
                row_blocks.push(SunMatrix::concatenate(&blocks, 1)?);
            }
            let matrix = SunMatrix::concatenate(&row_blocks, 0).map_err(|error| match error {
                MatrixError::IncompatibleDimensions => MatrixError::RaggedRows,
                other => other,
            })?;
            Ok(Value::Matrix(matrix))
        }
        ExprKind::Unary { op, operand } => {
            let value = eval(operand, env, output)?;
            match (op, value) {
                (UnaryOp::Positive, Value::Number(n)) => Ok(Value::Number(n)),
                (UnaryOp::Negative, Value::Number(n)) => Ok(Value::Number(-n)),
                (UnaryOp::Positive, Value::Complex(value)) => Ok(Value::Complex(value)),
                (UnaryOp::Negative, Value::Complex(value)) => Ok(Value::Complex(-value)),
                (UnaryOp::Positive, Value::Bool(value)) => Ok(Value::Number(u8::from(value) as f64)),
                (UnaryOp::Negative, Value::Bool(value)) => Ok(Value::Number(-(u8::from(value) as f64))),
                (UnaryOp::Not, value) => Ok(Value::Bool(!scalar_condition(value, "logical negation")?)),
                (op, value) => Err(RuntimeError::Unsupported(format!("unary {op:?} on {value}"))),
            }
        }
        ExprKind::Binary { left, op, right } => {
            let left = eval(left, env, output)?;
            if matches!(op, BinaryOp::And | BinaryOp::Or) {
                let left = scalar_condition(left, "logical operator")?;
                let result = match op {
                    BinaryOp::And if !left => false,
                    BinaryOp::Or if left => true,
                    _ => scalar_condition(eval(right, env, output)?, "logical operator")?,
                };
                return Ok(Value::Bool(result));
            }
            let right = eval(right, env, output)?;
            binary(left, *op, right)
        }
        ExprKind::Call { callee, arguments } => {
            let ExprKind::Identifier(name) = &callee.kind else {
                return Err(RuntimeError::Unsupported("only named function calls are supported".into()));
            };
            // A matrix followed by parentheses is indexed with Sun's 1-based convention.
            if let Some(Value::Matrix(matrix)) = env.get(name) {
                if arguments.len() == 1 {
                    match evaluate_index(&arguments[0], env, output, IndexAxis::Linear, matrix.shape())? {
                        MatrixIndex::Selection(indices) => {
                            let force_column = matches!(arguments[0].kind, ExprKind::IndexAll);
                            return Ok(Value::Matrix(matrix.select_linear(&indices, force_column)?));
                        }
                        MatrixIndex::Scalar(index) => return Ok(Value::Number(matrix.get_linear(index)?)),
                    }
                }
                if arguments.len() != 2 {
                    return Err(RuntimeError::Arity(name.clone(), "one linear index A(index) or two indices A(row, column)".into()));
                }
                let (rows, cols) = matrix.shape();
                let row = evaluate_index(&arguments[0], env, output, IndexAxis::Row, (rows, cols))?;
                let col = evaluate_index(&arguments[1], env, output, IndexAxis::Column, (rows, cols))?;
                if !matches!(row, MatrixIndex::Scalar(_)) || !matches!(col, MatrixIndex::Scalar(_)) {
                    let row_indices = row.into_indices();
                    let column_indices = col.into_indices();
                    return Ok(Value::Matrix(matrix.select(&row_indices, &column_indices)?));
                }
                let MatrixIndex::Scalar(row) = row else { unreachable!() };
                let MatrixIndex::Scalar(col) = col else { unreachable!() };
                return Ok(Value::Number(matrix.get(row, col)?));
            }
            if output.functions.contains_key(name) && env.contains_key(name) {
                return Err(RuntimeError::Unsupported(format!("variable '{name}' is not a callable function or matrix")));
            }
            let mut args = Vec::with_capacity(arguments.len());
            for arg in arguments { args.push(eval(arg, env, output)?); }
            call_function(name, args, env, output, allow_void)
        }
    }
}

fn call_function(name: &str, args: Vec<Value>, caller_env: &HashMap<String, Value>, context: &mut ExecutionContext<'_>, allow_void: bool) -> Result<Value, RuntimeError> {
    let Some(function) = context.functions.get(name).cloned() else {
        if (name == "writematrix" || graphics::handles(name)) && !allow_void { return Err(RuntimeError::NoOutput(name.into())); }
        return call(name, args, context);
    };
    context.check_cancelled()?;
    if args.len() != function.parameters.len() {
        return Err(RuntimeError::Arity(name.into(), format!("{} arguments", function.parameters.len())));
    }
    if function.output.is_none() && !allow_void { return Err(RuntimeError::NoOutput(name.into())); }
    if context.call_depth >= 32 { return Err(RuntimeError::RecursionLimit); }
    let mut locals: HashMap<String, Value> = function.parameters.iter().cloned().zip(args).collect();
    let previous_end = context.index_end.take();
    let mut pushed = false;
    let previous_debug = if context.debug.enabled {
        if let Some(frame) = context.debug.frame(caller_env) { context.debug.callers.push(frame); pushed = true; }
        let previous = (context.debug.source.take(), context.debug.name.clone(), context.debug.span);
        context.debug.source = context.function_sources.get(name).cloned();
        context.debug.name = name.into();
        Some(previous)
    } else { None };
    context.call_depth += 1;
    let result = (|| {
        match execute_statements(&function.body, &mut locals, context)? {
            ExecutionFlow::Normal | ExecutionFlow::Return => {}
            _ => return Err(RuntimeError::Unsupported("function loop control cannot escape a call".into())),
        }
        context.check_cancelled()?;
        match &function.output {
            Some(output) => locals.remove(output).filter(|value| *value != Value::Void)
                .ok_or_else(|| RuntimeError::UnassignedOutput(name.into(), output.clone())),
            None => Ok(Value::Void),
        }
    })();
    context.call_depth -= 1;
    context.index_end = previous_end;
    if let Some((source, name, span)) = previous_debug {
        if pushed { context.debug.callers.pop(); }
        context.debug.source = source; context.debug.name = name; context.debug.span = span;
    }
    result
}

fn binary(left: Value, op: BinaryOp, right: Value) -> Result<Value, RuntimeError> {
    use BinaryOp::*;
    let left = match left { Value::Bool(value) => Value::Number(u8::from(value) as f64), other => other };
    let right = match right { Value::Bool(value) => Value::Number(u8::from(value) as f64), other => other };
    match (left, right) {
        (Value::Complex(a), Value::Complex(b)) => complex_binary(a, op, b),
        (Value::Complex(a), Value::Number(b)) => complex_binary(a, op, Complex64::new(b, 0.0)),
        (Value::Number(a), Value::Complex(b)) => complex_binary(Complex64::new(a, 0.0), op, b),
        (Value::Number(a), Value::Number(b)) => {
            if matches!(op, Equal | NotEqual | Less | LessEqual | Greater | GreaterEqual) {
                return Ok(Value::Bool(match op {
                    Equal => a == b, NotEqual => a != b, Less => a < b, LessEqual => a <= b,
                    Greater => a > b, GreaterEqual => a >= b, _ => unreachable!(),
                }));
            }
            let result = match op {
                Add => a + b, Subtract => a - b, Multiply | ElementMultiply => a * b,
                Divide | ElementDivide => a / b, Power => a.powf(b),
                Equal => (a == b) as u8 as f64, NotEqual => (a != b) as u8 as f64,
                Less => (a < b) as u8 as f64, LessEqual => (a <= b) as u8 as f64,
                Greater => (a > b) as u8 as f64, GreaterEqual => (a >= b) as u8 as f64,
                And | Or => unreachable!("logical operators are evaluated with short-circuiting"),
            };
            Ok(Value::Number(result))
        }
        (Value::Matrix(a), Value::Matrix(b)) => match op {
            Add => Ok(Value::Matrix(a.broadcast_add(&b)?)),
            Subtract => Ok(Value::Matrix(a.broadcast_subtract(&b)?)),
            Multiply => Ok(Value::Matrix(a.matmul(&b)?)),
            ElementMultiply => Ok(Value::Matrix(a.broadcast_elementwise_mul(&b)?)),
            _ => Err(RuntimeError::Unsupported(format!("matrix operator {op:?}"))),
        },
        (Value::Matrix(a), Value::Number(b)) if matches!(op, Multiply | ElementMultiply) => Ok(Value::Matrix(matrix_map(&a, |x| x * b)?)),
        (Value::Number(a), Value::Matrix(b)) if matches!(op, Multiply | ElementMultiply) => Ok(Value::Matrix(matrix_map(&b, |x| x * a)?)),
        (Value::Matrix(a), Value::Number(b)) if matches!(op, Divide | ElementDivide) => Ok(Value::Matrix(matrix_map(&a, |x| x / b)?)),
        (a, b) => Err(RuntimeError::Unsupported(format!("operator {op:?} on {a} and {b}"))),
    }
}

fn complex_binary(left: Complex64, op: BinaryOp, right: Complex64) -> Result<Value, RuntimeError> {
    use BinaryOp::*;
    match op {
        Add => Ok(Value::Complex(left + right)),
        Subtract => Ok(Value::Complex(left - right)),
        Multiply | ElementMultiply => Ok(Value::Complex(left * right)),
        Divide | ElementDivide => Ok(Value::Complex(left / right)),
        Power if right.im == 0.0 && right.re.fract() == 0.0 && right.re >= i32::MIN as f64 && right.re <= i32::MAX as f64 => {
            Ok(Value::Complex(left.powi(right.re as i32)))
        }
        Power => Ok(Value::Complex(left.powc(right))),
        Equal => Ok(Value::Bool(left == right)),
        NotEqual => Ok(Value::Bool(left != right)),
        Less | LessEqual | Greater | GreaterEqual => Err(RuntimeError::Unsupported("complex numbers cannot be ordered".into())),
        And | Or => unreachable!("logical operators are evaluated with short-circuiting"),
    }
}

fn matrix_map(matrix: &SunMatrix, f: impl Fn(f64) -> f64) -> Result<SunMatrix, MatrixError> {
    Ok(matrix.map(f))
}

fn valid_error_identifier(identifier: &str) -> bool {
    identifier.split(':').count() >= 2 && identifier.split(':').all(|component| {
        let mut characters = component.chars();
        characters.next().is_some_and(|character| character.is_ascii_alphabetic())
            && characters.all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

fn language_list(name: &str, args: &[Value]) -> Result<Option<Value>, RuntimeError> {
    let items = match name {
        "keywords" => LANGUAGE_KEYWORDS,
        "constants" => LANGUAGE_CONSTANTS,
        "builtins" => LANGUAGE_BUILTINS,
        _ => return Ok(None),
    };
    if !args.is_empty() { return Err(RuntimeError::Arity(name.into(), "no arguments".into())); }
    let mut items = items.to_vec();
    items.sort_unstable();
    items.dedup();
    Ok(Some(Value::String(items.join("\n"))))
}

fn call(name: &str, args: Vec<Value>, output: &mut ExecutionContext<'_>) -> Result<Value, RuntimeError> {
    if let Some(value) = language_list(name, &args)? { return Ok(value); }
    if graphics::handles(name) { return graphics::call(name, &args, output); }
    if matches!(name, "readmatrix" | "writematrix") { return matrix_csv::call(name, &args, output); }
    if name == "pause" {
        return match args.as_slice() {
            [] => {
                output.wait_for_input()?;
                Ok(Value::Void)
            }
            [Value::String(state)] => {
                let previous = if output.pause_enabled { "on" } else { "off" };
                match state.as_str() {
                    "on" => output.pause_enabled = true,
                    "off" => output.pause_enabled = false,
                    "query" => {}
                    _ => return Err(RuntimeError::InvalidArgument("pause state must be 'on', 'off' or 'query'".into())),
                }
                Ok(Value::String(previous.into()))
            }
            [Value::Number(seconds)] => {
                let duration = Duration::try_from_secs_f64(*seconds).map_err(|_| {
                    RuntimeError::InvalidArgument("pause duration must be finite, nonnegative and representable as a duration".into())
                })?;
                output.pause(duration)?;
                Ok(Value::Void)
            }
            [_] => Err(RuntimeError::InvalidArgument("pause expects a duration or 'on', 'off' or 'query'".into())),
            _ => Err(RuntimeError::Arity(name.into(), "no arguments, one duration or one state".into())),
        };
    }
    if name == "error" {
        let (identifier, message) = match args.as_slice() {
            [Value::String(message)] => ("Sun:UserError".to_string(), message.clone()),
            [Value::String(identifier), Value::String(message)] => (identifier.clone(), message.clone()),
            [_] | [_, _] => return Err(RuntimeError::InvalidArgument("error expects a string message or a string identifier and message".into())),
            _ => return Err(RuntimeError::Arity(name.into(), "one message or an identifier and message".into())),
        };
        if !valid_error_identifier(&identifier) {
            return Err(RuntimeError::InvalidArgument("error identifier must have colon-separated ASCII components starting with a letter and containing only letters, digits or underscores".into()));
        }
        return Err(SunException { identifier, message }.into());
    }
    if name == "rethrow" {
        if args.len() != 1 { return Err(RuntimeError::Arity(name.into(), "one exception argument".into())); }
        return match args.into_iter().next().unwrap() {
            Value::Exception(exception) => Err(exception.into()),
            other => Err(RuntimeError::InvalidArgument(format!("rethrow expects an exception, got {other}"))),
        };
    }
    if name == "disp" {
        if args.len() != 1 { return Err(RuntimeError::Arity(name.into(), "one argument".into())); }
        output.write_line(&args[0].to_string())?;
        return Ok(Value::Void);
    }
    numeric::call(name, &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_complex_constants_arithmetic_and_functions() {
        let source = "z = 2 + 3 * i; disp(i); disp(j); disp(z); disp(-z); disp(z + 1); disp(z * i); disp(z / i); disp(i * i); disp(i ^ 2); disp(z == complex(2, 3)); disp(real(z)); disp(imag(z)); disp(conj(z)); disp(abs(3 + 4 * i)); disp(sqrt(-1));";
        let output = execute(&sun_parser::parse(source).unwrap()).unwrap();
        assert_eq!(output, ["i", "i", "2 + 3i", "-2 - 3i", "3 + 3i", "-3 + 2i", "3 - 2i", "-1 + 0i", "-1 + 0i", "true", "2", "3", "2 - 3i", "5", "i"]);
    }

    #[test]
    fn lists_language_keywords_constants_and_builtins() {
        let output = execute(&sun_parser::parse("keywords()\nconstants()\nbuiltins()").unwrap()).unwrap();
        assert_eq!(output.len(), 3);
        for (list, required) in [(&output[0], ["function", "if", "while"]), (&output[1], ["pi", "i", "true"]), (&output[2], ["disp", "complex", "keywords"])] {
            let entries: Vec<_> = list.lines().collect();
            assert!(required.into_iter().all(|name| entries.contains(&name)));
            assert!(entries.windows(2).all(|pair| pair[0] < pair[1]), "list is not sorted and unique: {entries:?}");
        }
        for name in ["keywords", "constants", "builtins"] {
            assert!(matches!(execute(&sun_parser::parse(&format!("{name}(1);")).unwrap()), Err(RuntimeError::Arity(_, _))));
        }
    }

    #[test]
    fn rejects_complex_values_at_real_only_boundaries() {
        for source in ["value = [i];", "if i; end", "for value = i; end", "A = [1]; A(1) = i;"] {
            let error = execute(&sun_parser::parse(source).unwrap()).unwrap_err().to_string();
            assert!(error.contains("number") || error.contains("condition") || error.contains("source"), "unexpected error for {source}: {error}");
        }
    }

    #[test]
    fn evaluates_predefined_constants_without_workspace_bindings() {
        let module = sun_parser::parse("disp(pi); disp(Inf); disp(NaN); disp(eps); disp(intmin); disp(intmax);").unwrap();
        let mut session = Session::new();
        let mut host = CollectingHost { lines: Vec::new() };
        session.execute(&module, &mut host).unwrap();
        assert_eq!(host.lines, ["3.141592653589793", "Inf", "NaN", "0.0000000000000002220446049250313", "-2147483648", "2147483647"]);
        assert!(session.variables().is_empty());
    }

    #[test]
    fn session_executes_only_new_code_and_retains_variables() {
        let mut session = Session::new();
        let mut host = CollectingHost { lines: Vec::new() };
        session.execute(&sun_parser::parse("value = 1; disp(value);").unwrap(), &mut host).unwrap();
        session.execute(&sun_parser::parse("value = value + 1; disp(value);").unwrap(), &mut host).unwrap();
        assert_eq!(host.lines, vec!["1", "2"]);
        assert!(matches!(session.variables().get("value"), Some(Value::Number(2.0))));
    }

    #[test]
    fn cooperative_cancel_keeps_workspace_and_bypasses_catch() {
        struct CancellingHost { flag: CancellationFlag, lines: Vec<String> }
        impl RuntimeHost for CancellingHost {
            fn cancellation_flag(&self) -> Option<CancellationFlag> { Some(self.flag.clone()) }
            fn write_line(&mut self, line: &str) -> io::Result<()> {
                self.lines.push(line.into());
                self.flag.store(true, Ordering::Release);
                Ok(())
            }
        }
        let mut session = Session::new();
        let mut host = CancellingHost { flag: Arc::new(AtomicBool::new(false)), lines: Vec::new() };
        let module = sun_parser::parse("try; value = 7; disp(value); while 1; end; catch failure; handled = 1; end;").unwrap();
        assert!(matches!(session.execute(&module, &mut host), Err(RuntimeError::Cancelled)));
        assert_eq!(host.lines, vec!["7"]);
        assert!(!session.variables().contains_key("handled"));
        assert!(!session.variables().contains_key("failure"));
        let mut next = CollectingHost { lines: Vec::new() };
        session.execute(&sun_parser::parse("disp(value);").unwrap(), &mut next).unwrap();
        assert_eq!(next.lines, vec!["7"]);
    }

    #[test]
    fn session_keeps_progress_after_error_and_return_without_replaying_output() {
        let mut session = Session::new();
        let mut host = CollectingHost { lines: Vec::new() };
        let module = sun_parser::parse("value = 7; disp(value); error(\"stop\");").unwrap();
        assert!(session.execute(&module, &mut host).is_err());
        session.execute(&sun_parser::parse("value = value + 1; return; value = 99;").unwrap(), &mut host).unwrap();
        session.execute(&sun_parser::parse("disp(value);").unwrap(), &mut host).unwrap();
        assert_eq!(host.lines, vec!["7", "8"]);
    }

    #[test]
    fn session_preserves_matrix_updates_across_commands() {
        let mut session = Session::new();
        let mut host = CollectingHost { lines: Vec::new() };
        session.execute(&sun_parser::parse("A = [1, 2; 3, 4];").unwrap(), &mut host).unwrap();
        session.execute(&sun_parser::parse("A(:, end) = 9;").unwrap(), &mut host).unwrap();
        session.execute(&sun_parser::parse("disp(A);").unwrap(), &mut host).unwrap();
        assert_eq!(host.lines, vec!["[1, 9; 3, 9]"]);
    }

    #[test]
    fn session_clear_resets_variables_and_pause_state() {
        let mut session = Session::new();
        let mut host = CollectingHost { lines: Vec::new() };
        session.execute(&sun_parser::parse("value = 7; pause(\"off\");").unwrap(), &mut host).unwrap();
        session.execute(&sun_parser::parse("pause(); disp(pause(\"query\"));").unwrap(), &mut host).unwrap();
        assert_eq!(host.lines, vec!["off"]);
        session.clear();
        assert!(session.variables().is_empty());
        assert!(session.execute(&sun_parser::parse("pause();").unwrap(), &mut host).is_err());
        session.execute(&sun_parser::parse("disp(pause(\"query\"));").unwrap(), &mut host).unwrap();
        assert_eq!(host.lines, vec!["off", "on"]);
    }

    #[test]
    fn session_static_errors_leave_existing_environment_unchanged() {
        let mut session = Session::new();
        let mut host = CollectingHost { lines: Vec::new() };
        session.execute(&sun_parser::parse("value = 1;").unwrap(), &mut host).unwrap();
        let invalid = sun_parser::parse("value = 99; parfor index = 1:3; output(index) = output(index); end;").unwrap();
        assert!(session.execute(&invalid, &mut host).is_err());
        assert!(matches!(session.variables().get("value"), Some(Value::Number(1.0))));
    }

    #[test]
    fn execution_report_preserves_workspace_after_runtime_failure() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; total = sum(A); error(\"stop\"); late = 99;").unwrap();
        let mut host = CollectingHost { lines: Vec::new() };
        let report = execute_with_report(&module, &mut host);
        assert!(report.result.is_err());
        assert_eq!(report.variables.get("total"), Some(&Value::Number(10.0)));
        assert!(matches!(report.variables.get("A"), Some(Value::Matrix(_))));
        assert!(!report.variables.contains_key("late"));
    }

    #[test]
    fn runs_empty_matrix_example() {
        let module = sun_parser::parse(include_str!("../../../examples/matrizes_vazias.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Literal vazio:", "[]", "Forma do literal:", "[0, 0]",
            "Sem linhas:", "[0, 3]", "Sem colunas:", "[2, 0]", "Selecao sem elementos.", "Quantidade de elementos:", "0",
            "Soma vazia:", "0", "Iteracoes do for vazio:", "0", "Transposta sem linhas:", "[3, 0]",
            "Destino apos escrita vazia:", "[1, 2, 3; 4, 5, 6]", "Sun:IndexOutOfBounds"]);
    }

    #[test]
    fn empty_literal_is_displayed_and_can_be_queried() {
        let module = sun_parser::parse("E = []; disp(E); disp(size(E)); disp(numel(E)); disp(isempty(E)); disp(size(7)); disp(numel(7)); disp(isempty(0)); if isempty(E); disp(9); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[]", "[0, 0]", "0", "true", "[1, 1]", "1", "false", "9"]);
    }

    #[test]
    fn empty_slices_preserve_axis_and_linear_shapes() {
        for (expression, shape) in [("E", "[0, 0]"), ("R", "[0, 3]"), ("C", "[2, 0]"),
            ("transpose(R)", "[3, 0]"), ("transpose(C)", "[0, 2]"), ("R(:, end)", "[0, 1]"),
            ("C(end, :)", "[1, 0]"), ("R(:)", "[0, 1]"), ("R(1:end)", "[1, 0]"),
            ("E(:)", "[0, 1]"), ("E(1:end)", "[1, 0]")] {
            let source = format!("A = [1, 2, 3; 4, 5, 6]; E = []; R = A(2:1, :); C = A(:, 2:1); disp(size({expression})); disp({expression});");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec![shape, "[]"], "unexpected result for {expression}");
        }
    }

    #[test]
    fn empty_arithmetic_preserves_shapes_and_compatible_broadcasting() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; R = A(2:1, :); C = A(:, 2:1); S = [2]; disp(size(R * 2)); disp(size(2 .* R)); disp(size(R / 2)); disp(size(R + S)); disp(size(S - R)); disp(size(C .* S)); disp(size(R + R)); disp(sum(R));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[0, 3]", "[0, 3]", "[0, 3]", "[0, 3]", "[0, 3]", "[2, 0]", "[0, 3]", "0"]);
    }

    #[test]
    fn empty_products_and_linear_algebra_use_defined_shapes() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; R = A(2:1, :); C = A(:, 2:1); E = []; disp(C * R); disp(det(E)); disp(inv(E)); disp(size(solve(E, R))); disp(size(solve(A, C)));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[0, 0; 0, 0]", "1", "[]", "[0, 2]", "[2, 0]"]);
    }

    #[test]
    fn empty_for_sources_execute_no_body_and_keep_existing_variable() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; value = 42; for value = []; disp(undefined); end; for value = A(2:1, :); disp(undefined); end; for value = A(:, 2:1); disp(undefined); end; disp(value); disp(7);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["42", "7"]);
    }

    #[test]
    fn empty_selection_writes_do_not_mutate_or_grow_target() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; A(2:1, :) = 0; A(2:1, :) = []; A(2:1) = []; A(2:1) = [99]; A(:, 2:1) = A(:, 2:1); disp(A); E = []; E(:) = 1; E(:, :) = []; E(:, :) = [7]; disp(size(E));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 2, 3; 4, 5, 6]", "[0, 0]"]);
    }

    #[test]
    fn nonempty_selection_cannot_be_deleted_by_empty_rhs() {
        for operation in ["A(:, 1) = [];", "A(1:end) = [];", "A(2:1, :) = [1, 2];"] {
            let source = format!("A = [1, 2; 3, 4]; try; {operation} catch failure; disp(failure.identifier); end; disp(A);");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["Sun:IncompatibleDimensions", "[1, 2; 3, 4]"]);
        }
    }

    #[test]
    fn empty_dimensions_still_enforce_nonempty_indices() {
        for operation in ["E(1);", "E(1, 1);", "R(1, :);", "R(:, end + 1);", "C(:, 1);", "E(1) = 7;"] {
            let source = format!("A = [1, 2; 3, 4]; E = []; R = A(2:1, :); C = A(:, 2:1); {operation}");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap_err().identifier(), "Sun:IndexOutOfBounds");
        }
        let module = sun_parser::parse("E = []; E(end);").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("positive integer"));
    }

    #[test]
    fn shape_queries_validate_argument_counts_and_numeric_types() {
        for source in ["size();", "size([], 1);", "numel(1, 2);", "isempty();"] {
            assert!(matches!(execute(&sun_parser::parse(source).unwrap()), Err(RuntimeError::Arity(_, _))));
        }
        for source in ["size(\"text\");", "isempty(\"\");", "numel(\"text\");"] {
            assert!(matches!(execute(&sun_parser::parse(source).unwrap()), Err(RuntimeError::Unsupported(_))));
        }
    }

    #[test]
    fn runs_linear_slice_example() {
        let module = sun_parser::parse(include_str!("../../../examples/fatias_lineares.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Todos os elementos em coluna:", "[1; 4; 2; 5; 3; 6]",
            "Do segundo ate end:", "[4, 2, 5, 3, 6]", "Ordem inversa:", "[6, 3, 5, 2, 4, 1]",
            "Vetor coluna preservado:", "[20; 30]", "Preenchimento alternado:", "[0, 0, 0; 4, 5, 6]",
            "Copia linear sobreposta:", "[0, 4, 5; 0, 0, 0]", "Substituicao por quantidade:", "[10, 50, 40; 30, 20, 60]",
            "Sun:IncompatibleDimensions", "Apos falha de quantidade:", "[10, 50, 40; 30, 20, 60]"]);
    }

    #[test]
    fn reads_linear_colon_and_range_slices() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; disp(A(:)); disp(A(2:end));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1; 4; 2; 5; 3; 6]", "[4, 2, 5, 3, 6]"]);
    }

    #[test]
    fn linear_slices_preserve_vector_orientation_and_colon_forces_a_column() {
        let module = sun_parser::parse("row = [1, 2, 3]; column = [4; 5; 6]; disp(row(:)); disp(row(2:end)); disp(column(2:end)); disp(row(1:1)); disp(row(1));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1; 2; 3]", "[2, 3]", "[5; 6]", "[1]", "1"]);
    }

    #[test]
    fn linear_slice_steps_follow_column_order_forward_and_backward() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; disp(A(end:-1:1)); disp(A(1:2:end)); disp(A(end:-2:1)); disp(A(1:7:end + 1));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[6, 3, 5, 2, 4, 1]", "[1, 2, 3]", "[6, 5, 4]", "[1]"]);
    }

    #[test]
    fn linear_slice_copies_are_independent_and_transposes_use_current_shape() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; selected = A(:); selected(1) = 99; disp(A(1)); T = transpose(A); disp(T(1:end)); A = [7, 8, 9]; disp(A(:));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "[1, 2, 3, 4, 5, 6]", "[7; 8; 9]"]);
    }

    #[test]
    fn fills_linear_slices_with_scalars_without_changing_shape() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; A(:) = 0; A(2:2:end) = [9]; A(1:2:end) = 7; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[7, 7, 7; 9, 9, 9]"]);
    }

    #[test]
    fn linear_slice_writes_compare_element_counts_and_flatten_rhs_by_columns() {
        let module = sun_parser::parse("A = [0, 0, 0; 0, 0, 0]; A(:) = [10, 20; 30, 40; 50, 60]; disp(A); A(1:2:end) = [7; 8; 9]; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[10, 50, 40; 30, 20, 60]", "[7, 8, 9; 30, 20, 60]"]);
    }

    #[test]
    fn overlapping_linear_slice_writes_use_rhs_snapshot() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; A(2:end) = A(1:end - 1); disp(A); A = [1, 2, 3; 4, 5, 6]; A(end:-1:1) = A; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 4, 5; 1, 2, 3]", "[6, 5, 4; 3, 2, 1]"]);
    }

    #[test]
    fn linear_slice_bounds_resolve_nested_end_and_function_arguments() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; B = [1; 2]; disp(A(B(end):end - 1)); disp(A(1:B(end):end)); disp(A(1:sqrt(end + 3):end)); disp(A(B(end), :));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[4, 2, 5, 3]", "[1, 2, 3]", "[1, 5]", "[4, 5, 6]"]);
    }

    #[test]
    fn linear_slice_write_rhs_resolves_its_own_end_context() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; B = [10, 20, 30, 40, 50]; A(2:end) = B(:); disp(A); try; A(:) = sqrt(end); catch failure; disp(failure.identifier); end; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 20, 40; 10, 30, 50]", "Sun:InvalidArgument", "[1, 20, 40; 10, 30, 50]"]);
    }

    #[test]
    fn linear_slice_errors_preserve_target_and_report_existing_identifiers() {
        for (operation, identifier) in [
            ("A(1:end + 1) = 0;", "Sun:IndexOutOfBounds"),
            ("A(1:1000000000) = 0;", "Sun:IndexOutOfBounds"),
            ("A(2:end) = [1, 2];", "Sun:IncompatibleDimensions"),
            ("A(:) = [1, 2];", "Sun:IncompatibleDimensions"),
            ("A(:) = \"text\";", "Sun:UnsupportedOperation"),
            ("A(1:0:end) = 0;", "Sun:InvalidArgument"),
            ("A(1:0.5:end) = 0;", "Sun:InvalidArgument"),
            ("A(:) = undefined;", "Sun:UndefinedVariable"),
        ] {
            let source = format!("A = [1, 2, 3; 4, 5, 6]; try; {operation} catch failure; disp(failure.identifier); end; disp(A);");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec![identifier, "[1, 2, 3; 4, 5, 6]"], "unexpected result for {operation}");
        }
    }

    #[test]
    fn linear_slice_context_is_restored_after_reads_and_errors() {
        for operation in ["A(1:end);", "A(1:undefined);", "A(1:undefined) = 7;"] {
            let source = format!("A = [1, 2; 3, 4]; try; {operation} catch; end; try; disp(end); catch failure; disp(failure.identifier); end; disp(A(end));");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["Sun:InvalidArgument", "4"]);
        }
    }

    #[test]
    fn linear_slice_orientation_controls_for_iteration() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; total = 0; for value = A(2:end); total = total + value; end; disp(total); count = 0; for column = A(:); count = count + 1; disp(sum(column)); end; disp(count);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["20", "21", "1"]);
    }

    #[test]
    fn runs_slice_assignment_example() {
        let module = sun_parser::parse(include_str!("../../../examples/atribuicao_fatias.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Coluna zerada:", "[1, 2, 0; 4, 5, 0; 7, 8, 0]",
            "Linha substituida:", "[10, 20, 0; 4, 5, 0; 7, 8, 0]", "Bloco substituido:", "[10, 20, 0; 40, 50, 0; 70, 80, 0]",
            "Copia sobreposta:", "[20, 0, 0; 50, 0, 0; 80, 0, 0]", "Linhas invertidas:", "[80, 0, 0; 50, 0, 0; 20, 0, 0]",
            "Escalar matricial expandido:", "[80, 7, 7; 50, 7, 7; 20, 0, 0]", "Sun:IncompatibleDimensions",
            "Apos falha de dimensoes:", "[80, 7, 7; 50, 7, 7; 20, 0, 0]"]);
    }

    #[test]
    fn fills_a_selected_column_with_a_scalar() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; A(:, end) = 0; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 2, 0; 4, 5, 0]"]);
    }

    #[test]
    fn fills_rows_columns_and_whole_matrix_with_scalar_values() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; A(end, :) = 9; A(:, 2:2) = 7; disp(A); A(:, :) = [0]; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 7, 3; 9, 7, 9]", "[0, 0, 0; 0, 0, 0]"]);
    }

    #[test]
    fn assigns_exactly_shaped_matrix_values_to_slices() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; A(:, end) = [30; 60]; A(1, 1:2) = [10, 20]; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[10, 20, 30; 4, 5, 60]"]);
    }

    #[test]
    fn slice_assignment_preserves_reversed_and_strided_target_order() {
        let module = sun_parser::parse("A = [11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 34]; A(end:-2:1, end:-2:1) = [100, 200; 300, 400]; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[11, 400, 13, 300; 21, 22, 23, 24; 31, 200, 33, 100]"]);
    }

    #[test]
    fn overlapping_slice_assignments_read_rhs_before_writing() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; A(:, 1:2) = A(:, 2:3); disp(A); A(end:-1:1, :) = A; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[2, 3, 3; 5, 6, 6]", "[5, 6, 6; 2, 3, 3]"]);
    }

    #[test]
    fn slice_assignment_resolves_nested_indices_and_rhs_end_independently() {
        let module = sun_parser::parse("A = [11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 34]; B = [1; 2]; C = [7, 8; 9, 10]; A(B(end):end, end - 1:end) = C(end:-1:1, :); disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[11, 12, 13, 14; 21, 22, 9, 10; 31, 32, 7, 8]"]);
    }

    #[test]
    fn slice_assignment_failures_leave_target_unchanged() {
        for (operation, identifier) in [
            ("A(1:3, 1) = 0;", "Sun:IndexOutOfBounds"),
            ("A(:, 1:4) = 0;", "Sun:IndexOutOfBounds"),
            ("A(1:2, 4) = 0;", "Sun:IndexOutOfBounds"),
            ("A(1:1, :) = [7, 8];", "Sun:IncompatibleDimensions"),
            ("A(:, end) = [7, 8];", "Sun:IncompatibleDimensions"),
            ("A(:, end) = [7; 8; 9];", "Sun:IncompatibleDimensions"),
            ("A(:, end) = \"text\";", "Sun:UnsupportedOperation"),
            ("A(:, end) = undefined;", "Sun:UndefinedVariable"),
            ("A(1:0:end, :) = 0;", "Sun:InvalidArgument"),
        ] {
            let source = format!("A = [1, 2, 3; 4, 5, 6]; try; {operation} catch failure; disp(failure.identifier); end; disp(A);");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec![identifier, "[1, 2, 3; 4, 5, 6]"], "unexpected result for {operation}");
        }
    }

    #[test]
    fn slice_assignment_does_not_leak_end_context_into_rhs_or_handlers() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; try; A(:, end) = sqrt(end); catch failure; disp(failure.identifier); end; try; disp(end); catch failure; disp(failure.identifier); end; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Sun:InvalidArgument", "Sun:InvalidArgument", "[1, 2; 3, 4]"]);
    }

    #[test]
    fn slice_assignment_requires_existing_matrix_target() {
        let module = sun_parser::parse("missing(:, end) = 0;").unwrap();
        assert!(matches!(execute(&module), Err(RuntimeError::UndefinedVariable(name)) if name == "missing"));
        let module = sun_parser::parse("A = 7; A(:, end) = 0;").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("non-matrix"));
    }

    #[test]
    fn slice_assignment_continues_with_following_statements_and_loop_iterations() {
        let module = sun_parser::parse("A = [0, 0; 0, 0]; for column = 1:2; A(:, column) = column; disp(column); end; A(1:1, :) = 9\ndisp(A); return; A(:, :) = 0;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "[9, 9; 1, 2]"]);
    }

    #[test]
    fn runs_matrix_slice_example() {
        let module = sun_parser::parse(include_str!("../../../examples/fatias.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Linhas de 2 ate end:",
            "[21, 22, 23, 24; 31, 32, 33, 34; 41, 42, 43, 44]", "Ultima coluna:", "[14; 24; 34; 44]",
            "Ultima linha:", "[41, 42, 43, 44]", "Linhas e colunas alternadas:", "[12, 14; 32, 34]",
            "Ordem invertida:", "[44, 42; 34, 32; 24, 22; 14, 12]", "Copia independente:", "[99; 24; 34; 44]",
            "Valor original:", "14"]);
    }

    #[test]
    fn reads_bidimensional_slices_and_complete_axes() {
        let module = sun_parser::parse("A = [11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 34]; disp(A(2:end, :)); disp(A(:, end)); disp(A(end, :));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[21, 22, 23, 24; 31, 32, 33, 34]", "[14; 24; 34]", "[31, 32, 33, 34]"]);
    }

    #[test]
    fn slice_steps_preserve_forward_and_reverse_order() {
        let module = sun_parser::parse("A = [11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 34]; disp(A(1:2:end, 2:2:end)); disp(A(end:-1:1, end:-2:1)); disp(A(1:2:end + 1, 1));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[12, 14; 32, 34]", "[34, 32; 24, 22; 14, 12]", "[11; 31]"]);
    }

    #[test]
    fn slices_remain_matrices_even_when_selecting_one_element() {
        let module = sun_parser::parse("A = [11, 12; 21, 22]; disp(A(1:1, 2:2)); disp(A(1, 2)); selected = A(:, 1); for column = selected; disp(column); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[12]", "12", "[11; 21]"]);
    }

    #[test]
    fn slice_results_are_independent_copies_using_the_current_shape() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; selected = A(:, :); selected(1, 1) = 99; disp(A); disp(selected); A = [7, 8, 9]; disp(A(:, end));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 2; 3, 4]", "[99, 2; 3, 4]", "[9]"]);
    }

    #[test]
    fn slice_bounds_resolve_nested_indices_and_function_arguments() {
        let module = sun_parser::parse("A = [11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 34]; B = [1; 2]; disp(A(B(end):end, :)); disp(A(1:sqrt(end + 1):end, end));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[21, 22, 23, 24; 31, 32, 33, 34]", "[14; 34]"]);
    }

    #[test]
    fn slice_context_is_restored_after_success_and_failure() {
        for operation in ["A(1:end, :);", "A(1:undefined, :);", "A(:, 1:undefined);"] {
            let source = format!("A = [1, 2; 3, 4]; try; {operation} catch; end; try; disp(end); catch failure; disp(failure.identifier); end; disp(A(end, end));");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["Sun:InvalidArgument", "4"]);
        }
    }

    #[test]
    fn slice_bounds_are_checked_before_large_index_allocations() {
        for operation in ["A(1:1000000000, :);", "A(:, 1:end + 1);", "A(end + 1:end + 1, :);", "A(1, end + 1:end + 1);"] {
            let source = format!("A = [1, 2; 3, 4]; {operation}");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap_err().identifier(), "Sun:IndexOutOfBounds");
        }
    }

    #[test]
    fn slices_reject_zero_steps_fractional_components_and_nonfinite_bounds() {
        for operation in ["A(1:0:end, :);", "A(1:0.5:end, :);", "A(1.5:end, :);", "A(1:2.5, :);",
            "A(1:(1 / 0):end, :);", "A(1:(0 / 0), :);", "A(1:1e100, :);"] {
            let source = format!("A = [1, 2; 3, 4]; {operation}");
            assert!(matches!(execute(&sun_parser::parse(&source).unwrap()), Err(RuntimeError::InvalidArgument(_))));
        }
    }

    #[test]
    fn empty_slices_return_empty_matrix_values() {
        for operation in ["A(2:1, :);", "A(1:-1:2, :);", "A(:, 2:1);"] {
            let source = format!("A = [1, 2; 3, 4]; {operation} disp(7);");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["7"]);
        }
    }

    #[test]
    fn slice_expressions_require_a_direct_matrix_index_context() {
        for source in ["disp(:);"] {
            assert!(matches!(execute(&sun_parser::parse(source).unwrap()), Err(RuntimeError::InvalidArgument(_))));
        }
    }

    #[test]
    fn runs_linear_index_example() {
        let module = sun_parser::parse(include_str!("../../../examples/indices_lineares.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Ordem linear por colunas:", "11", "21", "12", "22", "13", "23",
            "Ultimo elemento:", "23", "Penultimo elemento:", "13", "Apos atribuicoes lineares:",
            "[11, 12, 88; 21, 22, 99]", "Indice aninhado:", "99", "Sun:IndexOutOfBounds"]);
    }

    #[test]
    fn reads_linear_indices_in_column_major_order() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; for index = 1:6; disp(A(index)); end; disp(A(end)); disp(A(end - 1));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "4", "2", "5", "3", "6", "6", "3"]);
    }

    #[test]
    fn linear_indexing_handles_vectors_singletons_and_transposes() {
        let module = sun_parser::parse("row = [7, 8, 9]; column = [10; 20; 30]; scalar = [42]; disp(row(2)); disp(row(end)); disp(column(2)); disp(column(end)); disp(scalar(end)); A = [1, 2, 3; 4, 5, 6]; T = transpose(A); for index = 1:6; disp(T(index)); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["8", "9", "20", "30", "42", "1", "2", "3", "4", "5", "6"]);
    }

    #[test]
    fn linear_assignments_use_column_order_and_preserve_shape() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; A(2) = 40; A(5) = 30; A(end) = 99; disp(A); disp(A(2, 1)); disp(A(1, 3));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 2, 30; 40, 5, 99]", "40", "30"]);
    }

    #[test]
    fn linear_end_uses_current_element_count_after_reassignment() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; disp(A(end)); A = [7, 8, 9]; disp(A(end)); A(end) = 10; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["4", "9", "[7, 8, 10]"]);
    }

    #[test]
    fn linear_and_bidimensional_end_contexts_nest_and_restore() {
        let module = sun_parser::parse("A = [11, 12, 13; 21, 22, 23]; B = [1, 2]; disp(A(B(end))); disp(A(B(end) + end - 2)); disp(A(B(end), end)); disp(A(B(end, 1) + end - 1)); disp(A(sqrt(end + 3)));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["21", "23", "23", "23", "12"]);
    }

    #[test]
    fn linear_assignment_rhs_uses_its_own_matrix_context() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; B = [1, 2]; A(end) = B(end); A(end - 1) = A(2); disp(A); A(B(end) + end - 2) = 99; disp(A(end));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 2, 4; 4, 5, 2]", "99"]);
    }

    #[test]
    fn linear_index_context_is_restored_on_success_and_failure() {
        for operation in ["A(end);", "A(end + undefined);", "A(end + undefined) = 7;"] {
            let source = format!("A = [1, 2; 3, 4]; try; {operation} catch; end; try; disp(end); catch failure; disp(failure.identifier); end; disp(A(end));");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["Sun:InvalidArgument", "4"]);
        }
        let module = sun_parser::parse("A = [1]; A(end) = sqrt(end);").unwrap();
        assert!(matches!(execute(&module), Err(RuntimeError::InvalidArgument(_))));
    }

    #[test]
    fn linear_bounds_errors_preserve_matrix_and_use_existing_identifier() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; try; A(end + 1) = 0; catch failure; disp(failure.identifier); end; try; disp(A(7)); catch failure; disp(failure.identifier); end; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Sun:IndexOutOfBounds", "Sun:IndexOutOfBounds", "[1, 2, 3; 4, 5, 6]"]);
    }

    #[test]
    fn linear_indices_reject_nonpositive_fractional_and_nonfinite_values() {
        for index in ["0", "-1", "1.5", "0 / 0", "1 / 0", "end / 4", "1e100", "[0]"] {
            for operation in [format!("disp(A({index}));"), format!("A({index}) = 7;")] {
                let source = format!("A = [1, 2, 3; 4, 5, 6]; {operation}");
                let error = execute(&sun_parser::parse(&source).unwrap()).unwrap_err();
                assert!(error.to_string().contains("positive integer"), "unexpected error for {operation}: {error}");
            }
        }
    }

    #[test]
    fn linear_assignments_require_existing_matrix_and_numeric_values() {
        let module = sun_parser::parse("missing(end) = 7;").unwrap();
        assert!(matches!(execute(&module), Err(RuntimeError::UndefinedVariable(name)) if name == "missing"));
        let module = sun_parser::parse("A = 7; A(1) = 9;").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("non-matrix"));
        let module = sun_parser::parse("A = [1]; A(end) = [7];").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("assignment value must be a number"));
    }

    #[test]
    fn runs_index_end_example() {
        let module = sun_parser::parse(include_str!("../../../examples/indices_end.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Ultima linha, primeira coluna:", "31",
            "Primeira linha, ultima coluna:", "14", "Penultima linha, penultima coluna:", "23",
            "Apos alterar o ultimo elemento:", "[11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 99]",
            "Indice aninhado:", "99", "Sun:IndexOutOfBounds"]);
    }

    #[test]
    fn reads_last_rows_and_columns_using_index_end() {
        let module = sun_parser::parse("A = [11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 34]; disp(A(end, 1)); disp(A(1, end)); disp(A(end, end)); disp(A(end - 1, end - 1)); disp(A(end / end, end));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["31", "14", "34", "23", "14"]);
    }

    #[test]
    fn index_end_uses_current_shape_after_reassignment() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; disp(A(end, end)); A = [7, 8, 9]; disp(A(end, end)); A = [5; 6; 7]; disp(A(end, end)); A = [42]; disp(A(end, end));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["4", "9", "7", "42"]);
    }

    #[test]
    fn nested_matrix_indices_resolve_end_for_their_own_dimensions() {
        let module = sun_parser::parse("A = [11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 34]; B = [1; 2]; disp(A(B(end, 1), end)); disp(A(B(end, 1) + end - 2, end)); disp(A(end - B(end, 1), end)); disp(A(end, B(1, end)));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["24", "34", "14", "31"]);
    }

    #[test]
    fn functions_in_indices_inherit_the_enclosing_end_dimension() {
        let module = sun_parser::parse("A = [11, 12, 13, 14; 21, 22, 23, 24; 31, 32, 33, 34]; disp(A(end, sqrt(end))); disp(A(sum([end - 1, 1]), 1));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["32", "31"]);
    }

    #[test]
    fn assigns_using_end_and_evaluates_rhs_in_its_own_index_context() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; B = [8; 9; 10]; A(end, end) = B(end, 1); A(end - 1, end - 1) = 7; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 7, 3; 4, 5, 10]"]);
    }

    #[test]
    fn assignment_indices_support_nested_end_resolution() {
        let module = sun_parser::parse("A = [1, 2; 3, 4; 5, 6]; B = [1; 2]; A(B(end, 1) + end - 2, end) = 99; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 2; 3, 4; 5, 99]"]);
    }

    #[test]
    fn index_end_does_not_grow_matrices_or_bypass_index_validation() {
        for source in ["A(end + 1, 1);", "A(1, end + 1);", "A(end + 1, 1) = 9;", "A(1, end + 1) = 9;"] {
            let module = sun_parser::parse(&format!("A = [1, 2; 3, 4; 5, 6]; {source}")).unwrap();
            assert!(matches!(execute(&module), Err(RuntimeError::Matrix(MatrixError::IndexOutOfBounds { .. }))));
        }
        for source in ["A(end - end, 1);", "A(end / 2, 1);", "A(end / (end - end), 1);", "A(end - end, 1) = 9;"] {
            let module = sun_parser::parse(&format!("A = [1, 2; 3, 4; 5, 6]; {source}")).unwrap();
            assert!(execute(&module).unwrap_err().to_string().contains("positive integer"));
        }
    }

    #[test]
    fn end_is_not_available_to_functions_without_an_enclosing_index() {
        for source in ["disp(end);", "sqrt(end);", "A = [1]; A(end, 1) = sqrt(end);"] {
            let error = execute(&sun_parser::parse(source).unwrap()).unwrap_err();
            assert_eq!(error.identifier(), "Sun:InvalidArgument");
            assert!(error.to_string().contains("inside a matrix index"));
        }
    }

    #[test]
    fn index_context_is_restored_after_success_and_errors() {
        for index_expression in ["A(end, 1);", "A(end + undefined, 1);", "A(end + undefined, 1) = 9;"] {
            let source = format!("A = [1, 2; 3, 4]; try; {index_expression} catch; end; try; disp(end); catch failure; disp(failure.identifier); end; disp(A(end, end));");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["Sun:InvalidArgument", "4"]);
        }
    }

    #[test]
    fn index_end_requires_a_matrix_target_and_one_or_two_indices() {
        let module = sun_parser::parse("missing(end, 1) = 7;").unwrap();
        assert!(matches!(execute(&module), Err(RuntimeError::UndefinedVariable(name)) if name == "missing"));
        let module = sun_parser::parse("A = 7; A(end, 1) = 9;").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("non-matrix"));
        for source in ["A = [1, 2]; A();", "A = [1, 2]; A(1, 1, 1);"] {
            let module = sun_parser::parse(source).unwrap();
            assert!(matches!(execute(&module), Err(RuntimeError::Arity(_, _))));
        }
    }

    #[test]
    fn pause_controls_report_and_restore_previous_state() {
        let module = sun_parser::parse("disp(pause(\"query\")); previous = pause(\"off\"); disp(previous); disp(pause(\"query\")); pause(previous); disp(pause(\"query\"));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["on", "on", "off", "on"]);
    }

    #[test]
    fn repeated_pause_controls_return_previous_state_without_waiting() {
        let module = sun_parser::parse("disp(pause(\"off\")); disp(pause(\"off\")); disp(pause(\"on\")); disp(pause(\"on\")); disp(pause(\"query\"));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["on", "off", "off", "on", "on"]);
    }

    #[test]
    fn pause_off_suppresses_timed_interactive_and_bare_waits() {
        let module = sun_parser::parse("pause(\"off\"); pause(5); pause(); pause; disp(\"off\"); pause(\"on\"); pause(0.125); pause(); pause;").unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Line("off".into()), HostEvent::Pause(Duration::from_millis(125)),
            HostEvent::InteractivePause, HostEvent::InteractivePause]);
    }

    #[test]
    fn pause_state_is_shared_by_blocks_but_not_separate_executions() {
        let module = sun_parser::parse("for index = 1:2; if index == 1; pause(\"off\"); else; pause(1); end; end; try; error(\"failure\"); catch failure; disp(pause(\"query\")); end;").unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        let module = sun_parser::parse("disp(pause(\"query\")); pause();").unwrap();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Line("off".into()), HostEvent::Line("on".into()), HostEvent::InteractivePause]);
    }

    #[test]
    fn disabled_pauses_still_validate_arguments_without_changing_state() {
        let module = sun_parser::parse("pause(\"off\"); try; pause(-1); catch; disp(pause(\"query\")); end; try; pause(\"ON\"); catch; disp(pause(\"query\")); end; try; pause(\"on\", \"off\"); catch; disp(pause(\"query\")); end; pause(0);").unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Line("off".into()), HostEvent::Line("off".into()), HostEvent::Line("off".into())]);
    }

    #[test]
    fn interactive_pause_obeys_branches_and_control_flow() {
        let module = sun_parser::parse("if 0; pause(); end; for index = 1:2; continue; pause; end; return; pause();").unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert!(host.events.is_empty());
    }

    #[test]
    fn bare_pause_respects_an_existing_variable_named_pause() {
        let module = sun_parser::parse("pause = 7; disp(pause); pause\npause(0);").unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Line("7".into()), HostEvent::Line("7".into()), HostEvent::Pause(Duration::ZERO)]);
    }

    #[test]
    fn collecting_host_reports_unsupported_interactive_input_without_blocking() {
        for source in ["pause();", "pause;"] {
            let error = execute(&sun_parser::parse(source).unwrap()).unwrap_err();
            assert_eq!(error.identifier(), "Sun:IOError");
            assert!(error.to_string().contains("input support"));
        }
    }

    #[test]
    fn interactive_input_errors_are_catchable_and_disabled_waits_skip_input() {
        #[derive(Default)]
        struct FailingInputHost { lines: Vec<String> }
        impl RuntimeHost for FailingInputHost {
            fn write_line(&mut self, line: &str) -> io::Result<()> {
                self.lines.push(line.into());
                Ok(())
            }
            fn wait_for_input(&mut self) -> io::Result<()> {
                Err(io::Error::new(io::ErrorKind::UnexpectedEof, "input ended"))
            }
        }
        let module = sun_parser::parse("try; pause(); catch failure; disp(failure.identifier); end; disp(pause(\"query\")); pause(\"off\"); pause(); pause;").unwrap();
        let mut host = FailingInputHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.lines, vec!["Sun:IOError", "on"]);
    }

    #[derive(Debug, PartialEq)]
    enum HostEvent { Line(String), Pause(Duration), InteractivePause }

    #[derive(Default)]
    struct RecordingPauseHost { events: Vec<HostEvent> }

    impl RuntimeHost for RecordingPauseHost {
        fn write_line(&mut self, line: &str) -> io::Result<()> {
            self.events.push(HostEvent::Line(line.into()));
            Ok(())
        }

        fn pause(&mut self, duration: Duration) -> io::Result<()> {
            self.events.push(HostEvent::Pause(duration));
            Ok(())
        }

        fn wait_for_input(&mut self) -> io::Result<()> {
            self.events.push(HostEvent::InteractivePause);
            Ok(())
        }
    }

    #[test]
    fn runs_interactive_pause_example_without_real_input_or_waits() {
        let module = sun_parser::parse(include_str!("../../../examples/pause_interativa.sun")).unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Line("Estado inicial:".into()), HostEvent::Line("on".into()),
            HostEvent::Line("Pausas desativadas:".into()), HostEvent::Line("off".into()),
            HostEvent::Line("Pressione uma tecla para continuar.".into()), HostEvent::InteractivePause,
            HostEvent::Line("Pressione outra tecla para concluir.".into()), HostEvent::InteractivePause,
            HostEvent::Line("Execucao concluida.".into())]);
    }

    #[test]
    fn runs_pause_example_without_real_waits() {
        let module = sun_parser::parse(include_str!("../../../examples/pause.sun")).unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Line("Inicio do processamento:".into()),
            HostEvent::Line("1".into()), HostEvent::Pause(Duration::from_millis(100)),
            HostEvent::Line("2".into()), HostEvent::Pause(Duration::from_millis(100)),
            HostEvent::Line("3".into()), HostEvent::Pause(Duration::from_millis(100)),
            HostEvent::Line("Processamento concluido.".into())]);
    }

    #[test]
    fn pause_accepts_zero_without_emitting_an_implicit_result() {
        let module = sun_parser::parse("pause(0)\npause(-0);").unwrap();
        assert!(execute(&module).unwrap().is_empty());
    }

    #[test]
    fn pause_occurs_between_incremental_output_events() {
        let module = sun_parser::parse("value = 1\npause(0.125); disp(2);").unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Line("value = 1".into()),
            HostEvent::Pause(Duration::from_millis(125)), HostEvent::Line("2".into())]);
    }

    #[test]
    fn pause_evaluates_expressions_and_obeys_loop_and_return_flow() {
        let module = sun_parser::parse("for index = 1:3; if index == 2; continue; end; pause(index / 10); end; return; pause(10);").unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Pause(Duration::from_millis(100)), HostEvent::Pause(Duration::from_millis(300))]);
    }

    #[test]
    fn pause_rejects_invalid_arguments_before_invoking_host_pause() {
        for source in ["pause(1, 2);", "pause(-1);", "pause(0 / 0);", "pause(1 / 0);",
            "pause(1e308);", "pause(\"invalid\");", "pause([1]);", "pause(disp(1));"] {
            let module = sun_parser::parse(source).unwrap();
            let mut host = RecordingPauseHost::default();
            let error = execute_with_host(&module, &mut host).unwrap_err();
            assert!(matches!(error, RuntimeError::InvalidArgument(_) | RuntimeError::Arity(_, _)), "unexpected error for {source}: {error}");
            assert!(!host.events.iter().any(|event| matches!(event, HostEvent::Pause(_) | HostEvent::InteractivePause)));
        }
    }

    #[test]
    fn pause_validation_errors_can_be_handled_by_named_catch() {
        let module = sun_parser::parse("try; pause(-1); catch failure; disp(failure.identifier); end; pause(0);").unwrap();
        let mut host = RecordingPauseHost::default();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.events, vec![HostEvent::Line("Sun:InvalidArgument".into()), HostEvent::Pause(Duration::ZERO)]);
    }

    #[test]
    fn host_pause_errors_propagate_and_can_be_caught() {
        #[derive(Default)]
        struct FailingPauseHost { lines: Vec<String> }
        impl RuntimeHost for FailingPauseHost {
            fn write_line(&mut self, line: &str) -> io::Result<()> {
                self.lines.push(line.into());
                Ok(())
            }
            fn pause(&mut self, _duration: Duration) -> io::Result<()> {
                Err(io::Error::other("pause failed"))
            }
        }
        let mut host = FailingPauseHost::default();
        let module = sun_parser::parse("pause(1);").unwrap();
        let error = execute_with_host(&module, &mut host).unwrap_err();
        assert_eq!(error.identifier(), "Sun:IOError");
        let module = sun_parser::parse("try; pause(1); catch failure; disp(failure.identifier); end").unwrap();
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.lines, vec!["Sun:IOError"]);
    }

    #[test]
    fn pause_is_not_called_after_an_earlier_error() {
        let module = sun_parser::parse("disp(1); error(\"stop\"); pause(1);").unwrap();
        let mut host = RecordingPauseHost::default();
        assert!(execute_with_host(&module, &mut host).is_err());
        assert_eq!(host.events, vec![HostEvent::Line("1".into())]);
    }

    #[test]
    fn host_write_errors_are_reported_as_io_errors() {
        struct FailingHost;
        impl RuntimeHost for FailingHost {
            fn write_line(&mut self, _line: &str) -> io::Result<()> {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
            }
        }
        let module = sun_parser::parse("disp(1); disp(undefined);").unwrap();
        let error = execute_with_host(&module, &mut FailingHost).unwrap_err();
        assert_eq!(error.identifier(), "Sun:IOError");
        assert!(matches!(error, RuntimeError::Io(_)));
    }

    #[test]
    fn streams_output_before_an_uncaught_error() {
        struct RecordingHost { lines: Vec<String> }
        impl RuntimeHost for RecordingHost {
            fn write_line(&mut self, line: &str) -> std::io::Result<()> {
                self.lines.push(line.to_string());
                Ok(())
            }
        }
        let module = sun_parser::parse("disp(1); error(\"stop\");").unwrap();
        let mut host = RecordingHost { lines: Vec::new() };
        assert!(execute_with_host(&module, &mut host).is_err());
        assert_eq!(host.lines, vec!["1"]);
    }

    #[test]
    fn runs_named_exception_example() {
        let module = sun_parser::parse(include_str!("../../../examples/excecoes.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Erro numerico identificado:", "Sun:SingularMatrix",
            "matrix is singular and cannot be inverted or solved", "Erro de validacao:", "Pressao acima do limite.",
            "Repasse de excecao:", "Registrando erro antes do repasse.", "Industria:SensorInvalido",
            "Leitura do sensor invalida.", "Execucao concluida."]);
    }

    #[test]
    fn named_catch_exposes_identifiers_and_messages_for_builtin_errors() {
        for (body, identifier) in [
            ("disp(undefined);", "Sun:UndefinedVariable"),
            ("1 + \"text\";", "Sun:UnsupportedOperation"),
            ("sqrt();", "Sun:InvalidArgumentCount"),
            ("for index = 1:0:2; end", "Sun:InvalidForRange"),
            ("[1; 2, 3];", "Sun:RaggedMatrixRows"),
            ("A = [1]; A(2, 1);", "Sun:IndexOutOfBounds"),
            ("[1, 2] + [1, 2, 3];", "Sun:IncompatibleDimensions"),
            ("inv([1, 2; 2, 4]);", "Sun:SingularMatrix"),
            ("rethrow(1);", "Sun:InvalidArgument"),
        ] {
            let original = execute(&sun_parser::parse(body).unwrap()).unwrap_err();
            let source = format!("try; {body} catch failure; disp(failure.identifier); disp(failure.message); end");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec![identifier.to_string(), original.to_string()]);
        }
    }

    #[test]
    fn error_raises_default_or_custom_exceptions() {
        for (source, identifier, message) in [
            ("error(\"failure\");", "Sun:UserError", "failure"),
            ("error(\"Plant:Pressure:TooHigh\", \"Pressure exceeded\");", "Plant:Pressure:TooHigh", "Pressure exceeded"),
            ("error(\"literal %s\");", "Sun:UserError", "literal %s"),
        ] {
            let error = execute(&sun_parser::parse(source).unwrap()).unwrap_err();
            let exception = error.to_exception();
            assert_eq!(exception.identifier, identifier);
            assert_eq!(exception.message, message);
            assert_eq!(error.to_string(), format!("{identifier}: {message}"));
        }
    }

    #[test]
    fn named_catch_retains_exception_and_aliases_after_handler() {
        let module = sun_parser::parse("try; error(\"Plant:Failure\", \"failed\"); catch failure; saved = failure; end; failure = 0; disp(saved.identifier); disp(saved.message); disp(saved);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Plant:Failure", "failed", "Plant:Failure: failed"]);
    }

    #[test]
    fn named_catch_only_assigns_variable_when_an_error_occurs() {
        let module = sun_parser::parse("failure = 7; try; value = 1; catch failure; disp(undefined); end; disp(failure);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["7"]);
        let module = sun_parser::parse("try; value = 1; catch failure; end; disp(failure);").unwrap();
        assert!(matches!(execute(&module), Err(RuntimeError::UndefinedVariable(name)) if name == "failure"));
    }

    #[test]
    fn rethrow_preserves_original_builtin_identifier_and_message() {
        let original = execute(&sun_parser::parse("inv([1, 2; 2, 4]);").unwrap()).unwrap_err().to_exception();
        let module = sun_parser::parse("try; try; inv([1, 2; 2, 4]); catch inner; rethrow(inner); end; catch outer; rethrow(outer); end").unwrap();
        assert_eq!(execute(&module).unwrap_err().to_exception(), original);
    }

    #[test]
    fn rethrow_custom_exception_can_be_selected_by_switch() {
        let module = sun_parser::parse("try; try; error(\"Plant:Failure\", \"failed\"); catch inner; disp(inner.message); rethrow(inner); end; catch outer; switch outer.identifier; case \"Plant:Failure\"; disp(outer.message); otherwise; rethrow(outer); end; end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["failed", "failed"]);
    }

    #[test]
    fn nested_named_handlers_preserve_saved_exception_values() {
        let module = sun_parser::parse("try; error(\"outer\"); catch failure; saved = failure; try; error(\"inner\"); catch failure; disp(failure.message); end; disp(saved.message); disp(failure.message); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["inner", "outer", "inner"]);
    }

    #[test]
    fn named_handler_errors_propagate_as_new_exceptions() {
        let module = sun_parser::parse("try; try; error(\"original\"); catch inner; disp(inner.unknown); end; catch outer; disp(outer.identifier); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Sun:InvalidArgument"]);
    }

    #[test]
    fn rejects_invalid_error_arguments_identifiers_and_rethrow_values() {
        for source in ["error();", "error(1);", "error(\"Plant:Failure\", 1);", "error(1, \"message\");",
            "error(\"one\", \"two\", \"three\");", "error(\"\", \"message\");", "error(\"NoNamespace\", \"message\");",
            "error(\":Failure\", \"message\");", "error(\"Plant::Failure\", \"message\");", "error(\"Plant:1Failure\", \"message\");",
            "error(\"Plant:bad-id\", \"message\");", "rethrow();", "rethrow(1);", "rethrow(\"message\");", "rethrow([1]);",
            "rethrow(1, 2);", "disp((1).message);"] {
            let error = execute(&sun_parser::parse(source).unwrap()).unwrap_err();
            assert!(matches!(error, RuntimeError::InvalidArgument(_) | RuntimeError::Arity(_, _)), "unexpected error for {source}: {error}");
        }
    }

    #[test]
    fn named_catch_preserves_return_break_and_continue_behavior() {
        let module = sun_parser::parse("for index = 1:4; try; error(\"retry\"); catch failure; if index == 2; continue; elseif index == 4; break; end; disp(index); end; end; try; return; catch failure; disp(undefined); end; disp(undefined);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "3"]);
        let module = sun_parser::parse("try; error(\"stop\"); catch failure; disp(failure.message); return; end; disp(undefined);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["stop"]);
    }

    #[test]
    fn runs_try_catch_example() {
        let module = sun_parser::parse(include_str!("../../../examples/try_catch.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Tentativa de inversao:", "Matriz singular: usando identidade.",
            "[1, 0; 0, 1]", "Processamento por coluna:", "11", "Coluna ignorada por erro de indice.", "33", "Execucao concluida."]);
    }

    #[test]
    fn try_skips_catch_when_no_error_occurs() {
        let module = sun_parser::parse("try; value = 7; disp(value); catch; disp(undefined); end; try; catch; disp(undefined); end; disp(value);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["7", "7"]);
    }

    #[test]
    fn try_catches_runtime_error_categories() {
        for body in ["value = undefined;", "value = sqrt();", "value = inv([1, 2; 2, 4]);",
            "value = [1, 2] + [1, 2, 3];", "for index = 1:0:2; end", "value = 1 + \"text\";",
            "A = [1]; value = A(2, 1);", "if [1]; end", "switch [1]; end"] {
            let source = format!("try; {body} disp(99); catch; disp(7); end; disp(8);");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["7", "8"], "unexpected result for {body}");
        }
    }

    #[test]
    fn try_preserves_output_and_mutations_before_error_without_rollback() {
        let module = sun_parser::parse("A = [1, 2]; value = 1; try; disp(10); value = 7; A(1, 2) = 9; value = undefined; disp(99); catch; disp(value); disp(A); value = 8; end; disp(value);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["10", "7", "[1, 9]", "8"]);
    }

    #[test]
    fn try_can_suppress_an_error_with_an_empty_catch() {
        let module = sun_parser::parse("try; disp(undefined); catch; end; disp(3);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["3"]);
    }

    #[test]
    fn catch_errors_propagate_instead_of_reentering_the_same_handler() {
        let module = sun_parser::parse("try; disp(original_error); catch; disp(handler_error); end").unwrap();
        let error = execute(&module).unwrap_err();
        assert!(matches!(error, RuntimeError::UndefinedVariable(name) if name == "handler_error"));
    }

    #[test]
    fn outer_try_catches_errors_from_inner_catch() {
        let module = sun_parser::parse("try; try; disp(1); disp(undefined); catch; disp(2); disp(handler_error); end; disp(99); catch; disp(3); end; disp(4);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "3", "4"]);
    }

    #[test]
    fn inner_try_handles_error_without_triggering_outer_catch() {
        let module = sun_parser::parse("try; try; disp(undefined); catch; disp(2); end; disp(3); catch; disp(99); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["2", "3"]);
    }

    #[test]
    fn try_does_not_catch_return_and_catch_can_return() {
        for source in ["disp(1); try; return; catch; disp(undefined); end; disp(undefined);",
            "disp(1); try; disp(undefined); catch; return; end; disp(undefined);",
            "disp(1); for index = 1:2; try; switch index; case 1; if 1; return; end; end; catch; disp(undefined); end; end; disp(undefined);"] {
            assert_eq!(execute(&sun_parser::parse(source).unwrap()).unwrap(), vec!["1"]);
        }
    }

    #[test]
    fn try_does_not_catch_loop_control() {
        let module = sun_parser::parse("for index = 1:5; try; if index == 2; continue; elseif index == 4; break; end; disp(index); catch; disp(undefined); end; end; disp(index);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "3", "4"]);
    }

    #[test]
    fn catch_forwards_break_and_continue_to_enclosing_loop() {
        let module = sun_parser::parse("for index = 1:5; try; disp(undefined); catch; if index == 2; continue; elseif index == 4; break; end; disp(index); end; end; disp(index);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "3", "4"]);
    }

    #[test]
    fn try_handles_errors_from_nested_loops_and_resumes_after_handler() {
        let module = sun_parser::parse("try; for index = 1:3; disp(index); if index == 2; disp(undefined); end; end; disp(99); catch; disp(8); end; disp(index); try; while 1; break; end; disp(9); catch; disp(undefined); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "8", "2", "9"]);
    }

    #[test]
    fn try_preserves_implicit_output_rules_in_both_bodies() {
        let module = sun_parser::parse("try\n value = 1\n disp(undefined);\ncatch\n value = 2\n value; disp(value);\nend;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["value = 1", "value = 2", "2"]);
    }

    #[test]
    fn try_does_not_treat_nonfinite_results_as_errors() {
        let module = sun_parser::parse("try; value = 1 / 0; value = 0 / 0; disp(1); catch; disp(undefined); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1"]);
    }

    #[test]
    fn runs_return_example() {
        let module = sun_parser::parse(include_str!("../../../examples/return.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Busca por valor acima do limite:", "Primeiro valor encontrado:", "60"]);
    }

    #[test]
    fn return_ends_script_successfully_and_preserves_prior_output() {
        assert!(execute(&sun_parser::parse("return").unwrap()).unwrap().is_empty());
        let module = sun_parser::parse("value = 3\ndisp(value); return; disp(undefined);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["value = 3", "3"]);
    }

    #[test]
    fn return_propagates_from_each_conditional_branch() {
        for source in ["if 1; return; end", "if 0; elseif 1; return; end", "if 0; else; return; end",
            "switch 1; case 1; return; end", "switch 1; case 2; disp(undefined); otherwise; return; end"] {
            let source = format!("disp(1); {source}; disp(undefined);");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["1"]);
        }
    }

    #[test]
    fn return_ends_script_from_every_loop_form() {
        for source in ["counter = 0; while counter < 2; counter = counter + 1; return; end",
            "for index = 1:2; return; end", "for value = 7; return; end",
            "for value = [1, 2]; return; end", "for column = [1, 2; 3, 4]; return; end"] {
            let source = format!("disp(1); {source}; disp(undefined);");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["1"]);
        }
    }

    #[test]
    fn return_crosses_all_nested_blocks_instead_of_exiting_one_loop() {
        let module = sun_parser::parse("disp(1); for outer = 1:2; for column = [1, 2; 3, 4]; for value = 7; counter = 0; while counter < 2; counter = counter + 1; if value; switch value; case 7; disp(column); return; otherwise; disp(undefined); end; end; disp(undefined); end; disp(undefined); end; disp(undefined); end; disp(undefined); end; disp(undefined);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "[1; 3]"]);
    }

    #[test]
    fn return_in_unexecuted_code_does_not_stop_script() {
        let module = sun_parser::parse("if 0; return; end; if 1; elseif 1; return; else; return; end; while 0; return; end; for index = 2:1; return; end; switch 1; case 1; case 2; return; otherwise; return; end; disp(9);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["9"]);
    }

    #[test]
    fn return_does_not_swallow_prior_errors_or_terminate_later_executions() {
        assert!(matches!(execute(&sun_parser::parse("disp(undefined); return;").unwrap()), Err(RuntimeError::UndefinedVariable(_))));
        assert!(execute(&sun_parser::parse("return;").unwrap()).unwrap().is_empty());
        assert_eq!(execute(&sun_parser::parse("disp(2);").unwrap()).unwrap(), vec!["2"]);
    }

    #[test]
    fn runs_switch_example() {
        let module = sun_parser::parse(include_str!("../../../examples/switch.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Selecao numerica:", "Multiplicacao", "Selecao por texto:",
            "Resfriamento ativado", "Caso padrao:", "Opcao desconhecida", "Switch dentro de loop:", "1", "3"]);
    }

    #[test]
    fn switch_executes_only_the_first_matching_case() {
        let module = sun_parser::parse("switch 2; case 1; disp(undefined); case 2; disp(20); case 2; disp(undefined); case undefined; disp(99); otherwise; disp(undefined); end; disp(30);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["20", "30"]);
    }

    #[test]
    fn switch_matches_numbers_and_strings_without_type_coercion() {
        for (source, expected) in [
            ("switch -2; case -1 - 1; disp(1); end", "1"),
            ("switch \"2\"; case 2; disp(undefined); case '2'; disp(2); end", "2"),
            ("switch 2; case '2'; disp(undefined); otherwise; disp(3); end", "3"),
            ("switch \"Mode\"; case \"mode\"; disp(undefined); otherwise; disp(4); end", "4"),
            ("switch \"\"; case ''; disp(5); end", "5"),
            ("switch 0; case -0; disp(6); end", "6"),
        ] {
            assert_eq!(execute(&sun_parser::parse(source).unwrap()).unwrap(), vec![expected]);
        }
    }

    #[test]
    fn switch_uses_otherwise_or_does_nothing_when_no_case_matches() {
        let module = sun_parser::parse("switch 3; case 1; disp(undefined); otherwise; disp(30); end; switch 3; case 1; disp(undefined); end; switch 1; end; switch 1; otherwise; end; switch 1; case 1; case undefined; disp(undefined); end; disp(40);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["30", "40"]);
    }

    #[test]
    fn switch_supports_computed_expressions_and_shared_variables() {
        let module = sun_parser::parse("mode = 3; switch sum([1, 2]); case mode - 1; disp(undefined); case mode; mode = 9; end; disp(mode);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["9"]);
    }

    #[test]
    fn switch_preserves_implicit_output_and_supports_nesting() {
        let module = sun_parser::parse("switch 1\ncase 1\n value = 7\n if value; switch \"mode\"; case \"mode\"; disp(value); end; end\notherwise\n disp(undefined);\nend;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["value = 7", "7"]);
    }

    #[test]
    fn switch_forwards_break_and_continue_to_the_enclosing_for() {
        let module = sun_parser::parse("for index = 1:5; switch index; case 2; continue; case 4; break; otherwise; disp(index); end; end; disp(index); for index = [1, 2, 3]; switch index; case 1; continue; case 2; break; end; disp(undefined); end; disp(index);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "3", "4", "2"]);
    }

    #[test]
    fn switch_forwards_loop_control_through_nested_switches_to_while() {
        let module = sun_parser::parse("counter = 0; while counter < 5; counter = counter + 1; switch counter; case 1; continue; case 3; switch \"stop\"; case \"stop\"; if 1; break; end; end; otherwise; disp(counter); end; end; disp(counter); switch 1; case 1; while 1; break; end; disp(9); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["2", "3", "9"]);
    }

    #[test]
    fn switch_rejects_unsupported_selectors_even_with_no_cases() {
        for source in ["switch [1]; case 1; end", "switch [1, 2]; end", "switch disp(1); end"] {
            let error = execute(&sun_parser::parse(source).unwrap()).unwrap_err().to_string();
            assert!(error.contains("switch selector must be a number or string"));
        }
    }

    #[test]
    fn switch_validates_only_the_case_expressions_it_evaluates() {
        for source in ["switch 1; case [1]; end", "switch 1; case disp(1); end"] {
            let error = execute(&sun_parser::parse(source).unwrap()).unwrap_err().to_string();
            assert!(error.contains("switch case expression must be a number or string"));
        }
        let module = sun_parser::parse("switch 1; case 1; disp(1); case [1]; disp(undefined); end").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1"]);
    }

    #[test]
    fn switch_propagates_errors_from_evaluated_expressions_and_bodies() {
        for source in ["switch undefined; end", "switch 1; case undefined; end", "switch 1; case 1; disp(undefined); end",
            "switch 1; otherwise; disp(undefined); end"] {
            assert!(matches!(execute(&sun_parser::parse(source).unwrap()), Err(RuntimeError::UndefinedVariable(_))));
        }
    }

    #[test]
    fn switch_uses_numeric_equality_for_nonfinite_values() {
        let module = sun_parser::parse("switch 1 / 0; case 1 / 0; disp(1); end; switch 0 / 0; case 0 / 0; disp(undefined); otherwise; disp(2); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2"]);
    }

    #[test]
    fn runs_matrix_for_example() {
        let module = sun_parser::parse(include_str!("../../../examples/for_matrizes.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Colunas da matriz:", "[1; 10]", "[2; 20]", "[3; 30]",
            "Soma de todos os elementos:", "66", "Vetor linha:", "5", "10", "15", "Vetor coluna:", "[5; 10; 15]",
            "Vetor coluna transposto:", "5", "10", "15", "Somas por coluna com saltos:", "11", "33"]);
    }

    #[test]
    fn iterates_matrix_columns_in_matlab_order() {
        let module = sun_parser::parse("A = [1, 2, 3; 4, 5, 6]; for column = A; disp(column); end; disp(column);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1; 4]", "[2; 5]", "[3; 6]", "[3; 6]"]);
    }

    #[test]
    fn iterates_row_vectors_as_numbers_and_column_vectors_once() {
        let module = sun_parser::parse("total = 0; for value = [1, 2, 3]; total = total + value; end; disp(total); count = 0; for column = [1; 2; 3]; count = count + 1; disp(column); end; disp(count); for value = [7]; disp(value + 1); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["6", "[1; 2; 3]", "1", "8"]);
    }

    #[test]
    fn evaluates_matrix_for_source_once_before_body_mutations() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; for column = A; disp(column); A(1, 1) = 99; A = [0]; end; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1; 3]", "[2; 4]", "[0]"]);
    }

    #[test]
    fn matrix_for_variable_can_replace_source_variable() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; for A = A; disp(A); end; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1; 3]", "[2; 4]", "[2; 4]"]);
    }

    #[test]
    fn matrix_for_columns_are_independent_mutable_values() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; for column = A; column(1, 1) = 99; disp(column); column = 0; end; disp(A); disp(column);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[99; 3]", "[99; 4]", "[1, 2; 3, 4]", "0"]);
    }

    #[test]
    fn matrix_for_accepts_computed_sources() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; for column = A + A; disp(column); end; for value = transpose([1; 2; 3]); disp(value); end; for value = 2 + 3; disp(value); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[2; 6]", "[4; 8]", "1", "2", "3", "5"]);
    }

    #[test]
    fn matrix_for_supports_break_and_continue_through_conditionals() {
        let module = sun_parser::parse("for column = [1, 2, 3, 4; 10, 20, 30, 40]; if column(1, 1) == 2; continue; elseif column(1, 1) == 4; break; end; disp(sum(column)); end; disp(column);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["11", "33", "[4; 40]"]);
    }

    #[test]
    fn nests_matrix_for_with_ranges_and_while() {
        let module = sun_parser::parse("total = 0; for outer = [1, 2]; for inner = 1:3; total = total + outer * inner; end; while 1; for column = [1, 2; 3, 4]; break; end; break; end; end; disp(total); disp(column);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["18", "[1; 3]"]);
    }

    #[test]
    fn scalar_for_consumes_loop_control_and_skips_remaining_statements() {
        for keyword in ["break", "continue"] {
            let source = format!("for value = 7; {keyword}; disp(undefined); end; disp(value);");
            assert_eq!(execute(&sun_parser::parse(&source).unwrap()).unwrap(), vec!["7"]);
        }
    }

    #[test]
    fn matrix_for_preserves_implicit_output_rules() {
        let module = sun_parser::parse("for column = [1, 2; 3, 4]\n sum(column)\n sum(column);\nend;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["4", "6"]);
    }

    #[test]
    fn matrix_for_does_not_apply_finite_range_restrictions_to_elements() {
        let module = sun_parser::parse("count = 0; for value = [0 / 0, 1 / 0]; count = count + 1; end; for value = 0 / 0; count = count + 1; end; disp(count);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["3"]);
    }

    #[test]
    fn matrix_for_reports_invalid_sources_and_propagates_errors() {
        for source in ["for value = \"text\"; end", "for value = disp(1); end"] {
            let error = execute(&sun_parser::parse(source).unwrap()).unwrap_err().to_string();
            assert!(error.contains("for source must be a number or matrix"));
        }
        for source in ["for column = undefined; end", "for column = [1, 2]; disp(undefined); end"] {
            assert!(matches!(execute(&sun_parser::parse(source).unwrap()), Err(RuntimeError::UndefinedVariable(_))));
        }
        let module = sun_parser::parse("for column = []; end").unwrap();
        assert!(execute(&module).unwrap().is_empty());
    }

    #[test]
    fn runs_for_example() {
        let module = sun_parser::parse(include_str!("../../../examples/for.sun")).unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["Soma de 1 a 5:", "15", "Contagem regressiva:", "3", "2", "1",
            "Passo fracionario:", "0", "0.1", "0.2", "0.3", "Soma com saltos:", "18", "Matriz preenchida:", "[11, 12, 13; 21, 22, 23]"]);
    }

    #[test]
    fn executes_for_with_default_positive_and_negative_steps() {
        let module = sun_parser::parse("for index = 1:3; disp(index); end; for index = 2:2:6; disp(index); end; for index = 3:-1:1; disp(index); end; disp(index);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "3", "2", "4", "6", "3", "2", "1", "1"]);
    }

    #[test]
    fn evaluates_for_bounds_once_and_ignores_body_changes_to_index() {
        let module = sun_parser::parse("first = 1; step = 1; last = 3; for index = first:step:last; disp(index); first = 9; step = -1; last = 0; index = 99; end; disp(index);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "3", "99"]);
    }

    #[test]
    fn skips_empty_for_ranges_and_executes_singletons() {
        let module = sun_parser::parse("index = 42; for index = 3:1; disp(undefined); end; for index = 1:-1:3; disp(undefined); end; disp(index); for index = 2:2; disp(index); end; for index = 2:-1:2; disp(index); end; for index = 0:1:1e-20; disp(index); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["42", "2", "2", "0"]);
    }

    #[test]
    fn executes_fractional_for_ranges_without_losing_endpoints() {
        for (source, expected) in [
            ("for value = 0:0.1:0.3; disp(value); end", vec![0.0, 0.1, 0.2, 0.3]),
            ("for value = 1:-0.2:0; disp(value); end", vec![1.0, 0.8, 0.6, 0.4, 0.2, 0.0]),
            ("for value = 0:0.3:1; disp(value); end", vec![0.0, 0.3, 0.6, 0.9]),
        ] {
            let output = execute(&sun_parser::parse(source).unwrap()).unwrap();
            assert_eq!(output.len(), expected.len());
            for (actual, expected) in output.iter().zip(expected) {
                assert!((actual.parse::<f64>().unwrap() - expected).abs() < 1e-14);
            }
        }
    }

    #[test]
    fn for_break_and_continue_affect_only_the_innermost_loop() {
        let module = sun_parser::parse("total = 0; for outer = 1:2; for inner = 1:5; if inner == 2; continue; elseif inner == 4; break; end; total = total + inner; end; disp(outer); end; disp(total);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "8"]);
    }

    #[test]
    fn combines_for_and_while_with_loop_control() {
        let module = sun_parser::parse("for outer = 1:2; while 1; break; end; disp(outer); end; counter = 0; while counter < 2; counter = counter + 1; for inner = 1:3; continue; end; disp(counter); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "1", "2"]);
    }

    #[test]
    fn for_continue_advances_and_break_skips_the_rest_of_the_body() {
        let module = sun_parser::parse("for index = 1:3; continue; disp(undefined); end; disp(index); for index = 1:3; break; disp(undefined); end; disp(index);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["3", "1"]);
    }

    #[test]
    fn for_updates_matrices_and_preserves_implicit_output_rules() {
        let module = sun_parser::parse("A = [0; 0; 0]; for index = 1:3\n A(index, 1) = index * index;\n value = index\nend; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["value = 1", "value = 2", "value = 3", "[1; 4; 9]"]);
    }

    #[test]
    fn rejects_invalid_and_unrepresentable_for_ranges() {
        for range in ["1:0:3", "3:0:1", "1:-0:3", "0:(1 / 0):3", "(0 / 0):1", "1:(1 / 0)",
            "[1]:3", "1:[1]:3", "1:\"text\"", "disp(1):3", "0:1e-300:1", "0:1e-320:1e308",
            "1e16:1:(1e16 + 2)", "9007199254740990:1:9007199254740994"] {
            let source = format!("for index = {range}; end");
            let module = sun_parser::parse(&source).unwrap();
            assert!(matches!(execute(&module), Err(RuntimeError::InvalidForRange(_))), "unexpected result for {range}");
        }
    }

    #[test]
    fn handles_large_finite_for_bounds_without_intermediate_overflow() {
        let module = sun_parser::parse("for index = -1e308:1e308:1e308; disp(index / 1e308); end;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["-1", "0", "1"]);
    }

    #[test]
    fn propagates_errors_from_for_bounds_and_bodies() {
        for source in ["for index = undefined:3; end", "for index = 1:undefined:3; end", "for index = 1:undefined; end", "for index = 1:3; disp(undefined); end"] {
            let module = sun_parser::parse(source).unwrap();
            assert!(matches!(execute(&module), Err(RuntimeError::UndefinedVariable(_))));
        }
    }

    #[test]
    fn runs_control_flow_example() {
        let module = sun_parser::parse(include_str!("../../../examples/controle_fluxo.sun")).unwrap();
        let output = execute(&module).unwrap();
        assert_eq!(output.len(), 5);
        assert_eq!(&output[..4], ["Temperatura elevada", "Soma com continue e break:", "18", "Raiz de 2:"]);
        let approximation: f64 = output[4].parse().unwrap();
        assert!((approximation - 2.0_f64.sqrt()).abs() < 1e-10);
    }

    #[test]
    fn rejects_loop_control_in_direct_ast_outside_loops() {
        for keyword in ["break", "continue"] {
            let mut module = sun_parser::parse(&format!("while 1; {keyword}; end")).unwrap();
            let StmtKind::While { body, .. } = module.statements.remove(0).kind else { panic!("expected while") };
            let error = execute(&Module { statements: body }).unwrap_err().to_string();
            assert!(error.contains(&format!("{keyword} is only valid inside a loop")));
        }
    }

    #[test]
    fn executes_only_the_first_matching_conditional_branch() {
        for (source, expected) in [
            ("if -2; disp(1); elseif undefined; disp(2); else; disp(3); end", "1"),
            ("if 0; disp(undefined); elseif 0; disp(undefined); elseif 2; disp(2); elseif undefined; disp(4); else; disp(undefined); end", "2"),
            ("if 0; disp(undefined); elseif 0; disp(undefined); else; disp(3); end", "3"),
        ] {
            let module = sun_parser::parse(source).unwrap();
            assert_eq!(execute(&module).unwrap(), vec![expected]);
        }
    }

    #[test]
    fn supports_empty_and_nested_conditionals_with_shared_variables() {
        let module = sun_parser::parse("value = 1; if 0; disp(undefined); end; if 1; end; if 0; else; end; if value; if 0; else; value = 7; end; end; disp(value);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["7"]);
    }

    #[test]
    fn preserves_implicit_output_inside_conditionals() {
        let module = sun_parser::parse("if 1\n value = 3\n value; disp(value);\nend;\nif 1; disp(value); end").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["value = 3", "3", "3"]);
    }

    #[test]
    fn rejects_invalid_conditions_in_if_and_elseif() {
        for condition in ["[1]", "\"text\"", "1 / 0", "0 / 0", "disp(1)"] {
            for source in [format!("if {condition}; end"), format!("if 0; elseif {condition}; end")] {
                let module = sun_parser::parse(&source).unwrap();
                assert!(execute(&module).unwrap_err().to_string().contains("if condition must be a finite scalar number"));
            }
        }
    }

    #[test]
    fn short_circuits_logical_operators_and_normalizes_results() {
        let module = sun_parser::parse("disp(0 && undefined); disp(1 || undefined); disp(0 && disp(9)); disp(1 || disp(9)); disp(2 && -3); disp(0 || -3); disp(1 && 0); disp(0 || 0); disp(0 || 1 && 0); if 0 && undefined; disp(9); elseif 1 || undefined; disp(7); end; while 0 && undefined; disp(9); end").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["false", "true", "false", "true", "true", "true", "false", "false", "false", "7"]);
    }

    #[test]
    fn evaluates_required_logical_operands_and_rejects_invalid_types() {
        for source in ["1 && undefined", "0 || undefined"] {
            let module = sun_parser::parse(source).unwrap();
            assert!(matches!(execute(&module), Err(RuntimeError::UndefinedVariable(_))));
        }
        for source in ["[1] && 0", "\"text\" || 1", "1 && [1]", "0 || (1 / 0)", "(0 / 0) && 0", "0 || disp(1)"] {
            let module = sun_parser::parse(source).unwrap();
            assert!(execute(&module).unwrap_err().to_string().contains("logical operator condition must be a finite scalar number"));
        }
    }

    #[test]
    fn breaks_out_of_while_through_nested_conditionals() {
        let module = sun_parser::parse("counter = 0; while counter < 5; counter = counter + 1; if counter == 3; if 1; break; end; end; disp(counter); end; disp(counter);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "3"]);
    }

    #[test]
    fn continues_while_and_rechecks_the_condition() {
        let module = sun_parser::parse("counter = 0; while counter < 4; counter = counter + 1; if counter == 2; continue; elseif counter == 4; if 1; continue; end; end; disp(counter); end; disp(counter);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "3", "4"]);
    }

    #[test]
    fn loop_control_affects_only_the_innermost_loop() {
        let module = sun_parser::parse("outer = 0; total = 0; while outer < 2; outer = outer + 1; inner = 0; while inner < 4; inner = inner + 1; if inner == 1; continue; elseif inner == 3; break; end; total = total + 1; end; disp(outer); end; disp(total);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "2"]);
    }

    #[test]
    fn loop_control_skips_the_remaining_body() {
        for source in ["while 1; break; disp(undefined); end; disp(1);", "counter = 1; while counter; counter = 0; continue; disp(undefined); end; disp(1);"] {
            let module = sun_parser::parse(source).unwrap();
            assert_eq!(execute(&module).unwrap(), vec!["1"]);
        }
    }

    #[test]
    fn propagates_errors_from_selected_conditional_branches() {
        for source in ["if undefined; end", "if 1; disp(undefined); end", "if 0; else; disp(undefined); end"] {
            let module = sun_parser::parse(source).unwrap();
            assert!(matches!(execute(&module), Err(RuntimeError::UndefinedVariable(_))));
        }
    }

    #[test]
    fn executes_while_and_preserves_updated_variables() {
        let module = sun_parser::parse("counter = 1; total = 0; while counter <= 3\n disp(counter); total = total + counter; counter = counter + 1;\nend\ndisp(total); disp(counter);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["1", "2", "3", "6", "4"]);
    }

    #[test]
    fn skips_while_body_when_condition_is_initially_false() {
        let module = sun_parser::parse("while 0\n disp(undefined);\nend\nwhile 0; end; disp(42);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["42"]);
    }

    #[test]
    fn executes_nested_while_blocks() {
        let module = sun_parser::parse("outer = 1; total = 0; while outer <= 2; inner = 1; while inner <= 3; total = total + 1; inner = inner + 1; end; outer = outer + 1; end; disp(total);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["6"]);
    }

    #[test]
    fn preserves_implicit_output_rules_inside_while() {
        let module = sun_parser::parse("counter = 0; while counter < 2\n counter = counter + 1\n counter; disp(counter);\nend;").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["counter = 1", "1", "counter = 2", "2"]);
    }

    #[test]
    fn updates_matrices_and_accepts_nonzero_numeric_while_conditions() {
        let module = sun_parser::parse("A = [0; 0]; counter = -2; while counter; A(counter + 3, 1) = counter; counter = counter + 1; end; disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[-2; -1]"]);
    }

    #[test]
    fn rejects_non_scalar_and_non_finite_while_conditions() {
        for condition in ["[1]", "\"text\"", "1 / 0", "0 / 0", "disp(1)"] {
            let source = format!("while {condition}\nend");
            let module = sun_parser::parse(&source).unwrap();
            assert!(execute(&module).unwrap_err().to_string().contains("while condition must be a finite scalar number"));
        }
    }

    #[test]
    fn propagates_errors_from_while_conditions_and_bodies() {
        for source in ["while undefined\nend", "while 1\n disp(undefined);\nend"] {
            let module = sun_parser::parse(source).unwrap();
            assert!(matches!(execute(&module), Err(RuntimeError::UndefinedVariable(_))));
        }
    }

    #[test]
    fn evaluates_precedence_and_functions() {
        let module = sun_parser::parse("x = 2 + 3 * 4\ny = sqrt(x)").unwrap();
        let result = execute(&module).unwrap();
        assert_eq!(result, vec!["x = 14", "y = 3.7416573867739413"]);
    }

    #[test]
    fn evaluates_matrix_multiplication_and_elementwise_multiplication() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]\nB = A * A\nC = A .* A").unwrap();
        let result = execute(&module).unwrap();
        assert_eq!(result[1], "B = [7, 10; 15, 22]");
        assert_eq!(result[2], "C = [1, 4; 9, 16]");
    }

    #[test]
    fn evaluates_matrix_subtraction() {
        let module = sun_parser::parse("A = [5, 2; 3, 8]; B = [1, 4; 6, 2]; disp(A - B); disp(B - A); disp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[4, -2; -3, 6]", "[-4, 2; 3, -6]", "[5, 2; 3, 8]"]);
    }

    #[test]
    fn evaluates_matrix_subtraction_with_broadcasting() {
        let module = sun_parser::parse("A = [10, 20; 30, 40]; disp(A - [1, 2]); disp([1; 2] - A); disp([10; 20] - [1, 2]);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[9, 18; 29, 38]", "[-9, -19; -28, -38]", "[9, 8; 19, 18]"]);
    }

    #[test]
    fn rejects_matrix_subtraction_with_incompatible_dimensions() {
        let module = sun_parser::parse("[1, 2; 3, 4] - [1, 2, 3];").unwrap();
        assert!(matches!(execute(&module), Err(RuntimeError::Matrix(MatrixError::IncompatibleDimensions))));
    }

    #[test]
    fn evaluates_linear_algebra_functions() {
        let module = sun_parser::parse("A = [4, 7; 2, 6]\nT = transpose(A)\nd = det(A)\nI = inv(A)\nX = solve(A, [1; 0])").unwrap();
        let result = execute(&module).unwrap();
        assert_eq!(result[1], "T = [4, 2; 7, 6]");
        assert_eq!(result[2], "d = 10");
        assert_eq!(result[3], "I = [0.6000000000000001, -0.7000000000000001; -0.2, 0.4]");
        assert_eq!(result[4], "X = [0.6000000000000001; -0.2]");
    }

    #[test]
    fn reports_singular_matrix_for_inverse() {
        let module = sun_parser::parse("A = [1, 2; 2, 4]; B = inv(A);").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("singular"));
    }

    #[test]
    fn indexes_matrices_using_one_based_coordinates() {
        let module = sun_parser::parse("A = [1, 2; 3, 4];\ndisp(A(2, 1));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["3"]);
    }

    #[test]
    fn evaluates_matrix_indices_from_variables_and_expressions() {
        let module = sun_parser::parse("A = [1, 2; 3, 4];\nrow_index = 1 + 1;\ncolumn_index = row_index - 1;\ndisp(A(row_index, column_index));").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["3"]);
    }

    #[test]
    fn assigns_matrix_elements_with_literal_and_dynamic_indices() {
        let module = sun_parser::parse("A = [1, 2; 3, 4];\nrow_index = 2;\ncolumn_index = 1;\nA(row_index, column_index) = 99;\nA(1, 2) = 42;\ndisp(A);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["[1, 42; 99, 4]"]);
    }

    #[test]
    fn rejects_matrix_assignment_out_of_bounds() {
        let module = sun_parser::parse("A = [1, 2; 3, 4];\nA(3, 1) = 9;").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("out of bounds"));
    }

    #[test]
    fn rejects_fractional_dynamic_matrix_indices() {
        let module = sun_parser::parse("A = [1, 2; 3, 4];\nrow_index = 1.5;\ndisp(A(row_index, 1));").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("positive integer"));
    }

    #[test]
    fn rejects_zero_based_matrix_indices() {
        let module = sun_parser::parse("A = [1, 2; 3, 4];\ndisp(A(0, 1));").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("positive integer"));
    }

    #[test]
    fn suppresses_implicit_output_with_semicolon() {
        let module = sun_parser::parse("x = 42;\ndisp(x);").unwrap();
        assert_eq!(execute(&module).unwrap(), vec!["42"]);
    }

    #[test]
    fn reports_undefined_variables() {
        let module = sun_parser::parse("x = missing + 1;").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("undefined variable"));
    }
}

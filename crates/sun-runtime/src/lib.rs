//! Initial interpreter/runtime for Sun 0.1.

use std::collections::HashMap;
use sun_ast::{BinaryOp, Expr, ExprKind, Module, StmtKind, UnaryOp};
use sun_numeric::{MatrixError, SunMatrix};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    String(String),
    Matrix(SunMatrix),
    Void,
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => write!(f, "{n:.0}"),
            Self::Number(n) => write!(f, "{n}"),
            Self::String(s) => write!(f, "{s}"),
            Self::Matrix(m) => {
                write!(f, "[")?;
                for (ri, row) in m.to_rows().iter().enumerate() {
                    if ri > 0 { write!(f, "; ")?; }
                    for (ci, value) in row.iter().enumerate() {
                        if ci > 0 { write!(f, ", ")?; }
                        if value.fract() == 0.0 && value.abs() < 1e15 {
                            write!(f, "{value:.0}")?;
                        } else {
                            write!(f, "{value}")?;
                        }
                    }
                }
                write!(f, "]")
            }
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
    #[error(transparent)]
    Matrix(#[from] MatrixError),
}

pub fn execute(module: &Module) -> Result<Vec<String>, RuntimeError> {
    let mut env = HashMap::<String, Value>::new();
    let mut output = Vec::new();
    for statement in &module.statements {
        match &statement.kind {
            StmtKind::Assign { name, value } => {
                let result = eval(value, &mut env, &mut output)?;
                env.insert(name.clone(), result.clone());
                output.push(format!("{name} = {result}"));
            }
            StmtKind::Expr(expr) => {
                let result = eval(expr, &mut env, &mut output)?;
                if result != Value::Void { output.push(result.to_string()); }
            }
        }
    }
    Ok(output)
}

fn eval(expr: &Expr, env: &mut HashMap<String, Value>, output: &mut Vec<String>) -> Result<Value, RuntimeError> {
    match &expr.kind {
        ExprKind::Number(n) => Ok(Value::Number(*n)),
        ExprKind::String(s) => Ok(Value::String(s.clone())),
        ExprKind::Identifier(name) => env.get(name).cloned().ok_or_else(|| RuntimeError::UndefinedVariable(name.clone())),
        ExprKind::Matrix(rows) => {
            let mut values = Vec::with_capacity(rows.len());
            for row in rows {
                let mut current = Vec::with_capacity(row.len());
                for item in row {
                    match eval(item, env, output)? {
                        Value::Number(n) => current.push(n),
                        other => return Err(RuntimeError::Unsupported(format!("matrix elements must be numbers, got {other}"))),
                    }
                }
                values.push(current);
            }
            Ok(Value::Matrix(SunMatrix::from_rows(values)?))
        }
        ExprKind::Unary { op, operand } => {
            let value = eval(operand, env, output)?;
            match (op, value) {
                (UnaryOp::Positive, Value::Number(n)) => Ok(Value::Number(n)),
                (UnaryOp::Negative, Value::Number(n)) => Ok(Value::Number(-n)),
                (UnaryOp::Not, Value::Number(n)) => Ok(Value::Number((n == 0.0) as u8 as f64)),
                (op, value) => Err(RuntimeError::Unsupported(format!("unary {op:?} on {value}"))),
            }
        }
        ExprKind::Binary { left, op, right } => {
            let left = eval(left, env, output)?;
            let right = eval(right, env, output)?;
            binary(left, *op, right)
        }
        ExprKind::Call { callee, arguments } => {
            let ExprKind::Identifier(name) = &callee.kind else {
                return Err(RuntimeError::Unsupported("only named function calls are supported".into()));
            };
            let mut args = Vec::with_capacity(arguments.len());
            for arg in arguments { args.push(eval(arg, env, output)?); }
            call(name, args, output)
        }
    }
}

fn binary(left: Value, op: BinaryOp, right: Value) -> Result<Value, RuntimeError> {
    use BinaryOp::*;
    match (left, right) {
        (Value::Number(a), Value::Number(b)) => {
            let result = match op {
                Add => a + b, Subtract => a - b, Multiply | ElementMultiply => a * b,
                Divide | ElementDivide => a / b, Power => a.powf(b),
                Equal => (a == b) as u8 as f64, NotEqual => (a != b) as u8 as f64,
                Less => (a < b) as u8 as f64, LessEqual => (a <= b) as u8 as f64,
                Greater => (a > b) as u8 as f64, GreaterEqual => (a >= b) as u8 as f64,
                And => ((a != 0.0) && (b != 0.0)) as u8 as f64,
                Or => ((a != 0.0) || (b != 0.0)) as u8 as f64,
            };
            Ok(Value::Number(result))
        }
        (Value::Matrix(a), Value::Matrix(b)) => match op {
            Add => Ok(Value::Matrix(a.add(&b)?)),
            Multiply => Ok(Value::Matrix(a.matmul(&b)?)),
            ElementMultiply => Ok(Value::Matrix(a.elementwise_mul(&b)?)),
            _ => Err(RuntimeError::Unsupported(format!("matrix operator {op:?}"))),
        },
        (Value::Matrix(a), Value::Number(b)) if matches!(op, Multiply | ElementMultiply) => {
            Ok(Value::Matrix(matrix_map(&a, |x| x * b)?))
        }
        (Value::Number(a), Value::Matrix(b)) if matches!(op, Multiply | ElementMultiply) => {
            Ok(Value::Matrix(matrix_map(&b, |x| x * a)?))
        }
        (Value::Matrix(a), Value::Number(b)) if matches!(op, Divide | ElementDivide) => {
            Ok(Value::Matrix(matrix_map(&a, |x| x / b)?))
        }
        (a, b) => Err(RuntimeError::Unsupported(format!("operator {op:?} on {a} and {b}"))),
    }
}

fn matrix_map(matrix: &SunMatrix, f: impl Fn(f64) -> f64) -> Result<SunMatrix, MatrixError> {
    SunMatrix::from_rows(matrix.to_rows().into_iter().map(|row| row.into_iter().map(&f).collect()).collect())
}

fn call(name: &str, args: Vec<Value>, output: &mut Vec<String>) -> Result<Value, RuntimeError> {
    if name == "disp" {
        if args.len() != 1 { return Err(RuntimeError::Arity(name.into(), "one argument".into())); }
        output.push(args[0].to_string());
        return Ok(Value::Void);
    }
    if args.len() != 1 { return Err(RuntimeError::Arity(name.into(), "one argument".into())); }
    let arg = args.into_iter().next().unwrap();
    match (name, arg) {
        ("sqrt", Value::Number(n)) => Ok(Value::Number(n.sqrt())),
        ("sin", Value::Number(n)) => Ok(Value::Number(n.sin())),
        ("cos", Value::Number(n)) => Ok(Value::Number(n.cos())),
        ("tan", Value::Number(n)) => Ok(Value::Number(n.tan())),
        ("abs", Value::Number(n)) => Ok(Value::Number(n.abs())),
        ("exp", Value::Number(n)) => Ok(Value::Number(n.exp())),
        ("log", Value::Number(n)) => Ok(Value::Number(n.ln())),
        ("sum", Value::Matrix(m)) => Ok(Value::Number(m.to_rows().iter().flatten().sum())),
        ("sum", Value::Number(n)) => Ok(Value::Number(n)),
        (unknown, _) if !matches!(unknown, "sqrt" | "sin" | "cos" | "tan" | "abs" | "exp" | "log" | "sum") =>
            Err(RuntimeError::Unsupported(format!("unknown function '{unknown}'"))),
        (known, value) => Err(RuntimeError::Unsupported(format!("function '{known}' does not support {value}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluates_precedence_and_functions() {
        let module = sun_parser::parse("x = 2 + 3 * 4; y = sqrt(x);").unwrap();
        let result = execute(&module).unwrap();
        assert_eq!(result, vec!["x = 14", "y = 3.7416573867739413"]);
    }

    #[test]
    fn evaluates_matrix_multiplication_and_elementwise_multiplication() {
        let module = sun_parser::parse("A = [1, 2; 3, 4]; B = A * A; C = A .* A;").unwrap();
        let result = execute(&module).unwrap();
        assert_eq!(result[1], "B = [7, 10; 15, 22]");
        assert_eq!(result[2], "C = [1, 4; 9, 16]");
    }

    #[test]
    fn reports_undefined_variables() {
        let module = sun_parser::parse("x = missing + 1;").unwrap();
        assert!(execute(&module).unwrap_err().to_string().contains("undefined variable"));
    }
}

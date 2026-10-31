use num_complex::Complex64;
use sun_numeric::{maximum, minimum, SunMatrix, MAX_MATRIX_ELEMENTS};
use super::{RuntimeError, Value};

pub(super) fn scalar(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => Some(*number),
        Value::Bool(value) => Some(u8::from(*value) as f64),
        _ => None,
    }
}

fn dimension(value: &Value) -> Result<usize, RuntimeError> {
    match value {
        Value::Number(number) if number.is_finite() && number.fract() == 0.0 && *number >= 0.0 && *number <= MAX_MATRIX_ELEMENTS as f64 => Ok(*number as usize),
        _ => Err(RuntimeError::InvalidArgument("matrix dimensions must be nonnegative finite integers no greater than 16777216".into())),
    }
}

fn dimensions(name: &str, args: &[Value]) -> Result<(usize, usize), RuntimeError> {
    match args {
        [] => Ok((1, 1)),
        [Value::Matrix(shape)] if shape.element_count() == 2 && (shape.shape().0 == 1 || shape.shape().1 == 1) => {
            Ok((dimension(&Value::Number(shape.get_linear(1)?))?, dimension(&Value::Number(shape.get_linear(2)?))?))
        }
        [value] => { let size = dimension(value)?; Ok((size, size)) }
        [rows, cols] => Ok((dimension(rows)?, dimension(cols)?)),
        _ => Err(RuntimeError::Arity(name.into(), "zero, one size, a two-element size vector, or two dimensions".into())),
    }
}

fn unary_argument<'a>(name: &str, args: &'a [Value]) -> Result<&'a Value, RuntimeError> {
    if args.len() != 1 { return Err(RuntimeError::Arity(name.into(), "one argument".into())); }
    Ok(&args[0])
}

fn unsupported(name: &str, value: &Value) -> RuntimeError {
    RuntimeError::Unsupported(format!("function '{name}' does not support {value}"))
}

fn elementwise(name: &str, value: &Value, operation: fn(f64) -> f64) -> Result<Value, RuntimeError> {
    if let Some(number) = scalar(value) { return Ok(Value::Number(operation(number))); }
    match value {
        Value::Matrix(matrix) => Ok(Value::Matrix(matrix.map(operation))),
        _ => Err(unsupported(name, value)),
    }
}

fn pairwise(name: &str, left: &Value, right: &Value) -> Result<Value, RuntimeError> {
    let operation = if name == "min" { minimum } else { maximum };
    match (scalar(left), scalar(right), left, right) {
        (Some(left), Some(right), _, _) => Ok(Value::Number(operation(left, right))),
        (Some(number), _, _, Value::Matrix(matrix)) => Ok(Value::Matrix(matrix.map(|value| operation(number, value)))),
        (_, Some(number), Value::Matrix(matrix), _) => Ok(Value::Matrix(matrix.map(|value| operation(value, number)))),
        (_, _, Value::Matrix(left), Value::Matrix(right)) => Ok(Value::Matrix(if name == "min" { left.pairwise_minimum(right)? } else { left.pairwise_maximum(right)? })),
        _ => Err(RuntimeError::InvalidArgument(format!("{name} expects numeric scalars or matrices"))),
    }
}

pub(super) fn call(name: &str, args: &[Value]) -> Result<Value, RuntimeError> {
    if name == "complex" {
        if args.len() != 2 { return Err(RuntimeError::Arity(name.into(), "two real scalar arguments: complex(real, imaginary)".into())); }
        let (Some(real), Some(imaginary)) = (scalar(&args[0]), scalar(&args[1])) else {
            return Err(RuntimeError::InvalidArgument("complex expects two real scalar arguments".into()));
        };
        return Ok(Value::Complex(Complex64::new(real, imaginary)));
    }
    if matches!(name, "real" | "imag" | "conj") {
        let value = unary_argument(name, args)?;
        return match (name, value) {
            ("real", Value::Complex(value)) => Ok(Value::Number(value.re)),
            ("imag", Value::Complex(value)) => Ok(Value::Number(value.im)),
            ("conj", Value::Complex(value)) => Ok(Value::Complex(value.conj())),
            ("real", value) if scalar(value).is_some() => Ok(Value::Number(scalar(value).unwrap())),
            ("imag", value) if scalar(value).is_some() => Ok(Value::Number(0.0)),
            ("conj", value) if scalar(value).is_some() => Ok(Value::Number(scalar(value).unwrap())),
            _ => Err(unsupported(name, value)),
        };
    }
    if matches!(name, "zeros" | "ones" | "eye") {
        let (rows, cols) = dimensions(name, args)?;
        return Ok(Value::Matrix(if name == "eye" { SunMatrix::identity(rows, cols)? }
            else { SunMatrix::filled(rows, cols, if name == "ones" { 1.0 } else { 0.0 })? }));
    }
    if name == "solve" {
        if args.len() != 2 { return Err(RuntimeError::Arity(name.into(), "two matrix arguments: solve(A, B)".into())); }
        return match (&args[0], &args[1]) {
            (Value::Matrix(left), Value::Matrix(right)) => Ok(Value::Matrix(left.solve(right)?)),
            _ => Err(RuntimeError::Unsupported("solve expects two matrices".into())),
        };
    }
    if matches!(name, "min" | "max") && args.len() == 2 { return pairwise(name, &args[0], &args[1]); }
    let elementary: Option<fn(f64) -> f64> = match name {
        "sqrt" => Some(f64::sqrt), "sin" => Some(f64::sin), "cos" => Some(f64::cos),
        "tan" => Some(f64::tan), "abs" => Some(f64::abs), "exp" => Some(f64::exp), "log" => Some(f64::ln),
        _ => None,
    };
    if let Some(operation) = elementary {
        let value = unary_argument(name, args)?;
        if let Value::Complex(value) = value {
            return Ok(match name {
                "abs" => Value::Number(value.norm()),
                "sqrt" => Value::Complex(value.sqrt()),
                "sin" => Value::Complex(value.sin()),
                "cos" => Value::Complex(value.cos()),
                "tan" => Value::Complex(value.tan()),
                "exp" => Value::Complex(value.exp()),
                "log" => Value::Complex(value.ln()),
                _ => unreachable!(),
            });
        }
        if let Value::Number(number) = value {
            if (name == "sqrt" || name == "log") && *number < 0.0 {
                let value = Complex64::new(*number, 0.0);
                return Ok(Value::Complex(if name == "sqrt" { value.sqrt() } else { value.ln() }));
            }
        }
        return elementwise(name, value, operation);
    }
    if !matches!(name, "sum" | "mean" | "min" | "max" | "size" | "numel" | "isempty" | "transpose" | "det" | "inv") {
        return Err(RuntimeError::Unsupported(format!("unknown function '{name}'")));
    }
    let value = unary_argument(name, args)?;
    if matches!(name, "sum" | "mean" | "min" | "max") {
        if let Some(number) = scalar(value) { return Ok(Value::Number(number)); }
        return match value {
            Value::Matrix(matrix) => match name {
                "sum" => Ok(Value::Number(matrix.sum())),
                "mean" => Ok(Value::Number(matrix.mean())),
                _ => match if name == "min" { matrix.minimum() } else { matrix.maximum() } {
                    Some(number) => Ok(Value::Number(number)),
                    None => Ok(Value::Matrix(SunMatrix::filled(0, 0, 0.0)?)),
                },
            },
            _ => Err(unsupported(name, value)),
        };
    }
    match (name, value) {
        ("size", Value::Matrix(matrix)) => {
            let (rows, cols) = matrix.shape();
            Ok(Value::Matrix(SunMatrix::from_rows(vec![vec![rows as f64, cols as f64]])?))
        }
        ("size", Value::Number(_) | Value::Complex(_) | Value::Bool(_)) => Ok(Value::Matrix(SunMatrix::from_rows(vec![vec![1.0, 1.0]])?)),
        ("numel", Value::Matrix(matrix)) => Ok(Value::Number(matrix.element_count() as f64)),
        ("numel", Value::Number(_) | Value::Complex(_) | Value::Bool(_)) => Ok(Value::Number(1.0)),
        ("isempty", Value::Matrix(matrix)) => Ok(Value::Bool(matrix.is_empty())),
        ("isempty", Value::Number(_) | Value::Complex(_) | Value::Bool(_)) => Ok(Value::Bool(false)),
        ("transpose", Value::Matrix(matrix)) => Ok(Value::Matrix(matrix.transpose()?)),
        ("det", Value::Matrix(matrix)) => Ok(Value::Number(matrix.determinant()?)),
        ("inv", Value::Matrix(matrix)) => Ok(Value::Matrix(matrix.inverse()?)),
        _ => Err(unsupported(name, value)),
    }
}
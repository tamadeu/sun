use std::{borrow::Cow, io::{self, Read, Write}, path::Path};
use sun_numeric::{MatrixError, SunMatrix, MAX_MATRIX_ELEMENTS};
use super::{CancellationFlag, ExecutionContext, RuntimeError, Value};

const MAX_CSV_BYTES: usize = 64 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 1024 * 1024;
const MAX_COLUMNS: usize = 65_536;

#[derive(Clone, Copy, PartialEq)]
enum QuoteState { Start, Unquoted, Quoted, Closed, TrailingSpace }

struct CsvInput<Input> {
    input: Input, cancelled: Option<CancellationFlag>, byte_limit: usize,
    bytes: usize, record_bytes: usize, fields: usize, state: QuoteState,
    bom_remaining: usize, bom_present: bool, failure: Option<RuntimeError>,
}

impl<Input: Read> CsvInput<Input> {
    fn validate(&mut self, byte: u8) -> Result<(), RuntimeError> {
        self.bytes += 1;
        self.record_bytes += 1;
        if self.bytes > self.byte_limit { return Err(size_error()); }
        if self.record_bytes > MAX_RECORD_BYTES { return Err(RuntimeError::Csv("record exceeds the 1 MiB limit".into())); }
        if self.bytes == 1 && byte == 0xef { self.bom_remaining = 2; self.bom_present = true; return Ok(()); }
        if self.bom_remaining > 0 {
            let expected = if self.bom_remaining == 2 { 0xbb } else { 0xbf };
            if byte != expected { return Err(RuntimeError::Csv("invalid UTF-8 BOM".into())); }
            self.bom_remaining -= 1;
            return Ok(());
        }
        if self.state == QuoteState::Quoted {
            if byte == b'"' { self.state = QuoteState::Closed; }
            return Ok(());
        }
        if byte == b',' {
            self.fields += 1;
            if self.fields > MAX_COLUMNS { return Err(RuntimeError::Csv("record exceeds the 65536-column limit".into())); }
            self.state = QuoteState::Start;
        } else if matches!(byte, b'\r' | b'\n') {
            self.record_bytes = 0; self.fields = 1; self.state = QuoteState::Start;
        } else {
            self.state = match (self.state, byte) {
                (QuoteState::Start, b'"') | (QuoteState::Closed, b'"') => QuoteState::Quoted,
                (QuoteState::Unquoted, b'"') => return Err(RuntimeError::Csv("unexpected quote in unquoted field".into())),
                (QuoteState::Closed | QuoteState::TrailingSpace, b' ' | b'\t') => QuoteState::TrailingSpace,
                (QuoteState::Closed | QuoteState::TrailingSpace, _) => return Err(RuntimeError::Csv("unexpected character after closing quote".into())),
                _ => QuoteState::Unquoted,
            };
        }
        Ok(())
    }
}

impl<Input: Read> Read for CsvInput<Input> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() { return Ok(0); }
        loop {
            if self.cancelled.as_ref().is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Acquire)) {
                self.failure = Some(RuntimeError::Cancelled);
                return Err(io::Error::other("execution cancelled"));
            }
            let previous_bytes = self.bytes;
            let length = self.input.read(buffer)?;
            if length == 0 && (self.state == QuoteState::Quoted || self.bom_remaining > 0) {
                self.failure = Some(RuntimeError::Csv("unterminated quoted field or BOM".into()));
                return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid CSV ending"));
            }
            for &byte in &buffer[..length] {
                if let Err(error) = self.validate(byte) {
                    self.failure = Some(error);
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid or oversized CSV"));
                }
            }
            let skip = if self.bom_present { 3_usize.saturating_sub(previous_bytes).min(length) } else { 0 };
            if skip == length && length > 0 { continue; }
            if skip > 0 { buffer.copy_within(skip..length, 0); }
            return Ok(length - skip);
        }
    }
}

fn csv_error(error: csv::Error) -> RuntimeError {
    let message = error.to_string();
    match error.into_kind() {
        csv::ErrorKind::Io(error) => RuntimeError::Io(error),
        _ => RuntimeError::Csv(message),
    }
}

fn size_error() -> RuntimeError { RuntimeError::InvalidArgument("CSV exceeds the 64 MiB file limit".into()) }

fn path(value: &Value) -> Result<&Path, RuntimeError> {
    match value {
        Value::String(name) if !name.is_empty() => Ok(Path::new(name)),
        _ => Err(RuntimeError::InvalidArgument("CSV filename must be a nonempty string".into())),
    }
}

fn read_matrix(input: impl Read, context: &ExecutionContext<'_>, byte_limit: usize) -> Result<SunMatrix, RuntimeError> {
    let input = CsvInput { input, cancelled: context.cancelled.clone(), byte_limit, bytes: 0,
        record_bytes: 0, fields: 1, state: QuoteState::Start, bom_remaining: 0, bom_present: false, failure: None };
    let mut reader = csv::ReaderBuilder::new().has_headers(false).trim(csv::Trim::All)
        .from_reader(input);
    let mut record = csv::StringRecord::new();
    let mut values = Vec::new();
    let mut rows = 0_usize;
    let mut columns = 0;
    loop {
        context.check_cancelled()?;
        let result = reader.read_record(&mut record);
        context.check_cancelled()?;
        if let Some(error) = reader.get_mut().failure.take() { return Err(error); }
        if !result.map_err(csv_error)? { break; }
        let count = (rows + 1).checked_mul(record.len()).ok_or(MatrixError::SizeLimit)?;
        if record.len() > MAX_MATRIX_ELEMENTS || rows >= MAX_MATRIX_ELEMENTS || count > MAX_MATRIX_ELEMENTS {
            return Err(MatrixError::SizeLimit.into());
        }
        values.try_reserve(record.len()).map_err(|_| MatrixError::SizeLimit)?;
        for (column, field) in record.iter().enumerate() {
            if column % 4096 == 0 { context.check_cancelled()?; }
            let value = field.parse::<f64>().map_err(|_| RuntimeError::Csv(format!(
                "line {}, column {} must contain a number", record.position().map_or(rows as u64 + 1, csv::Position::line), column + 1)))?;
            values.push(value);
        }
        rows += 1;
        columns = record.len();
    }
    context.check_cancelled()?;
    Ok(SunMatrix::from_row_major(rows, columns, values)?)
}

struct BoundedBuffer { bytes: Vec<u8>, limit: usize, exceeded: bool }

impl Write for BoundedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            self.exceeded = true;
            return Err(io::Error::new(io::ErrorKind::InvalidData, "CSV output limit exceeded"));
        }
        self.bytes.try_reserve(bytes.len()).map_err(io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

fn write_matrix(matrix: &SunMatrix, context: &ExecutionContext<'_>, byte_limit: usize) -> Result<Vec<u8>, RuntimeError> {
    let (rows, cols) = matrix.shape();
    if matrix.is_empty() && (rows, cols) != (0, 0) {
        return Err(RuntimeError::InvalidArgument("CSV cannot preserve a shaped empty matrix; only 0x0 empties can be written".into()));
    }
    if cols > MAX_COLUMNS { return Err(RuntimeError::Csv("record exceeds the 65536-column limit".into())); }
    if rows > MAX_MATRIX_ELEMENTS || cols > MAX_MATRIX_ELEMENTS || matrix.element_count() > MAX_MATRIX_ELEMENTS {
        return Err(MatrixError::SizeLimit.into());
    }
    let mut writer = csv::WriterBuilder::new().has_headers(false)
        .from_writer(BoundedBuffer { bytes: Vec::new(), limit: byte_limit, exceeded: false });
    for row in 1..=rows {
        context.check_cancelled()?;
        let mut record_bytes = 0;
        for column in 1..=cols {
            if column % 4096 == 0 { context.check_cancelled()?; }
            let value = matrix.get(row, column)?;
            let field = if value == f64::INFINITY { "Inf".into() }
                else if value == f64::NEG_INFINITY { "-Inf".into() } else { value.to_string() };
            record_bytes += field.len() + usize::from(column > 1);
            if record_bytes + 1 > MAX_RECORD_BYTES { return Err(RuntimeError::Csv("record exceeds the 1 MiB limit".into())); }
            let result = writer.write_field(field);
            if writer.get_ref().exceeded { return Err(size_error()); }
            result.map_err(csv_error)?;
        }
        let result = writer.write_record(std::iter::empty::<&[u8]>());
        if writer.get_ref().exceeded { return Err(size_error()); }
        result.map_err(csv_error)?;
    }
    let result = writer.flush();
    if writer.get_ref().exceeded { return Err(size_error()); }
    result?;
    context.check_cancelled()?;
    let buffer = writer.into_inner().map_err(|error| RuntimeError::Io(error.into_error()))?;
    Ok(buffer.bytes)
}

pub(super) fn call(name: &str, args: &[Value], context: &mut ExecutionContext<'_>) -> Result<Value, RuntimeError> {
    context.check_cancelled()?;
    if name == "readmatrix" {
        if args.len() != 1 { return Err(RuntimeError::Arity(name.into(), "one CSV filename".into())); }
        let path = path(&args[0])?;
        let input = context.host.read_file(path)?;
        return Ok(Value::Matrix(read_matrix(input, context, MAX_CSV_BYTES)?));
    }
    if args.len() != 2 { return Err(RuntimeError::Arity(name.into(), "a numeric matrix and a CSV filename".into())); }
    let path = path(&args[1])?;
    let matrix = match &args[0] {
        Value::Matrix(matrix) => Cow::Borrowed(matrix),
        Value::Number(number) => Cow::Owned(SunMatrix::filled(1, 1, *number)?),
        _ => return Err(RuntimeError::InvalidArgument("writematrix expects a numeric matrix or scalar".into())),
    };
    let contents = write_matrix(&matrix, context, MAX_CSV_BYTES)?;
    context.check_cancelled()?;
    context.host.write_file_atomic(path, &contents)?;
    context.check_cancelled()?;
    Ok(Value::Void)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CollectingHost, Session};

    #[test]
    fn byte_limits_reject_oversized_input_and_output() {
        let mut host = CollectingHost { lines: Vec::new() };
        let mut session = Session::new();
        let context = ExecutionContext { host: &mut host, pause_enabled: true, index_end: None,
            cancelled: None, functions: &session.functions, call_depth: 0, graphics: &mut session.graphics,
            debug: crate::debug::DebugState::default(), function_sources: &session.function_sources };
        assert!(matches!(read_matrix(&b"1,2\n3,4\n"[..], &context, 4), Err(RuntimeError::InvalidArgument(_))));
        assert!(read_matrix(&b"1,2\n"[..], &context, 4).is_ok());
        let matrix = SunMatrix::from_rows(vec![vec![1.0,2.0]]).unwrap();
        assert!(matches!(write_matrix(&matrix, &context, 3), Err(RuntimeError::InvalidArgument(_))));
        assert_eq!(write_matrix(&matrix, &context, 4).unwrap(), b"1,2\n");
    }

    #[test]
    fn quote_and_bom_validation_handles_one_byte_reads() {
        struct Chunked<'a>(&'a [u8]);
        impl Read for Chunked<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let size = buffer.len().min(1);
                self.0.read(&mut buffer[..size])
            }
        }
        let mut host = CollectingHost { lines: Vec::new() };
        let mut session = Session::new();
        let context = ExecutionContext { host: &mut host, pause_enabled: true, index_end: None,
            cancelled: None, functions: &session.functions, call_depth: 0, graphics: &mut session.graphics,
            debug: crate::debug::DebugState::default(), function_sources: &session.function_sources };
        let input = b"\xef\xbb\xbf\"1\",\"2\"\r\n\"3\",\"4\"";
        assert_eq!(read_matrix(Chunked(input), &context, 100).unwrap().to_rows(), [[1.0,2.0],[3.0,4.0]]);
        assert!(matches!(read_matrix(Chunked(b"\"1\"0,2\n"), &context, 100), Err(RuntimeError::Csv(_))));
    }
}
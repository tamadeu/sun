use std::{fs, io::{self, BufRead, Write}, path::Path};
use serde_json::{json, Value as JsonValue};
use sun_lexer::LexError;
use sun_parser::{parse, ParseError};
use sun_runtime::{execute_with_report, validate_parallel_blocks, RuntimeHost, Value};

const PREVIEW_SIZE: usize = 10;
const WORKSPACE_LIMIT: usize = 200;

fn emit(output: &mut impl Write, event: JsonValue) -> io::Result<()> {
    serde_json::to_writer(&mut *output, &event).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

fn ready(output: &mut impl Write) -> io::Result<()> {
    emit(output, json!({"type": "ready", "protocol": 1, "version": env!("CARGO_PKG_VERSION")}))
}

fn numeric(number: f64) -> JsonValue {
    if number.is_nan() { json!("NaN") }
    else if number == f64::INFINITY { json!("Inf") }
    else if number == f64::NEG_INFINITY { json!("-Inf") }
    else { json!(number) }
}

fn variable(name: &str, value: &Value) -> JsonValue {
    match value {
        Value::Number(number) => json!({"name":name,"kind":"number","rows":1,"cols":1,"value":value.to_string(),"preview":[[numeric(*number)]],"truncated":false}),
        Value::Matrix(matrix) => {
            let (rows, cols) = matrix.shape();
            let preview: Vec<Vec<_>> = (1..=rows.min(PREVIEW_SIZE)).map(|row| {
                (1..=cols.min(PREVIEW_SIZE)).map(|col| numeric(matrix.get(row, col).expect("preview coordinates are in bounds"))).collect()
            }).collect();
            let summary = if matrix.is_empty() { "[]".into() } else { format!("{} elementos", matrix.element_count()) };
            json!({"name":name,"kind":"matrix","rows":rows,"cols":cols,"value":summary,"preview":preview,
                "truncated":!matrix.is_empty() && (rows > PREVIEW_SIZE || cols > PREVIEW_SIZE)})
        }
        other => {
            let kind = match other { Value::String(_) => "string", Value::Exception(_) => "exception", _ => "void" };
            let text = if matches!(other, Value::Void) { "void".to_string() } else { other.to_string() };
            let truncated = text.chars().count() > 200;
            json!({"name":name,"kind":kind,"rows":null,"cols":null,"value":text.chars().take(200).collect::<String>(),"preview":[],"truncated":truncated})
        }
    }
}

fn parse_diagnostic(source: &str, error: &ParseError) -> JsonValue {
    let offset = match error {
        ParseError::UnexpectedToken { offset, .. } | ParseError::LoopControlOutsideLoop { offset, .. } => *offset,
        ParseError::Lex(error) => match error {
            LexError::UnexpectedCharacter { offset, .. } | LexError::UnterminatedString { offset } | LexError::InvalidNumber { offset } => *offset,
        },
    };
    let prefix = source.get(..offset).unwrap_or(source);
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    json!({"type":"diagnostic","diagnostic":{"message":error.to_string(),"code":"Sun:SyntaxError","offset":offset,"line":line,"column":column}})
}

struct StudioHost<'a, Input: BufRead, Output: Write> { input: &'a mut Input, output: &'a mut Output }

impl<Input: BufRead, Output: Write> RuntimeHost for StudioHost<'_, Input, Output> {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        emit(self.output, json!({"type":"output","text":format!("{line}\n")}))
    }

    fn wait_for_input(&mut self) -> io::Result<()> {
        emit(self.output, json!({"type":"input","prompt":"Execucao pausada."}))?;
        let mut line = Vec::new();
        if self.input.read_until(b'\n', &mut line)? == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "interactive pause reached end of IDE input"));
        }
        emit(self.output, json!({"type":"resumed"}))
    }
}

fn run_source(source: &str, check: bool, input: &mut impl BufRead, output: &mut impl Write) -> io::Result<bool> {
    ready(output)?;
    let module = match parse(source) {
        Ok(module) => module,
        Err(error) => {
            emit(output, parse_diagnostic(source, &error))?;
            emit(output, json!({"type":"finished","ok":false}))?;
            return Ok(false);
        }
    };
    if let Err(error) = validate_parallel_blocks(&module) {
        emit(output, json!({"type":"diagnostic","diagnostic":{"message":error.to_string(),"code":error.identifier()}}))?;
        emit(output, json!({"type":"finished","ok":false}))?;
        return Ok(false);
    }
    if check {
        emit(output, json!({"type":"finished","ok":true}))?;
        return Ok(true);
    }
    let report = execute_with_report(&module, &mut StudioHost { input, output });
    let mut entries: Vec<_> = report.variables.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    let variables: Vec<_> = entries.iter().take(WORKSPACE_LIMIT).map(|(name, value)| variable(name, value)).collect();
    emit(output, json!({"type":"workspace","variables":variables}))?;
    if entries.len() > WORKSPACE_LIMIT {
        emit(output, json!({"type":"diagnostic","diagnostic":{"message":"Workspace limitado a 200 variaveis.","code":"Sun:WorkspaceLimit","severity":"warning"}}))?;
    }
    let ok = report.result.is_ok();
    if let Err(error) = report.result {
        emit(output, json!({"type":"diagnostic","diagnostic":{"message":error.to_string(),"code":error.identifier()}}))?;
    }
    emit(output, json!({"type":"finished","ok":ok}))?;
    Ok(ok)
}

pub fn run_file(path: &Path, check: bool) -> io::Result<bool> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    match fs::read_to_string(path) {
        Ok(source) => run_source(&source, check, &mut io::stdin().lock(), &mut output),
        Err(error) => {
            ready(&mut output)?;
            emit(&mut output, json!({"type":"diagnostic","diagnostic":{"message":error.to_string(),"code":"Sun:IOError"}}))?;
            emit(&mut output, json!({"type":"finished","ok":false}))?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn events(source: &str, check: bool, input: &[u8]) -> (bool, Vec<JsonValue>) {
        let mut output = Vec::new();
        let ok = run_source(source, check, &mut io::Cursor::new(input), &mut output).unwrap();
        let records = String::from_utf8(output).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        (ok, records)
    }

    #[test]
    fn streams_output_and_real_workspace_including_empty_shapes() {
        let (ok, records) = events("A = [1, 2; 3, 4]; E = A(2:1, :); disp(A);", false, b"");
        assert!(ok);
        assert_eq!(records[0]["protocol"], 1);
        assert_eq!(records[1]["text"], "[1, 2; 3, 4]\n");
        let variables = &records[2]["variables"];
        assert_eq!(variables[0]["name"], "A");
        assert_eq!(variables[0]["preview"], json!([[1.0, 2.0], [3.0, 4.0]]));
        assert_eq!(variables[1]["rows"], 0);
        assert_eq!(variables[1]["cols"], 2);
    }

    #[test]
    fn runtime_error_keeps_partial_workspace_and_identifier() {
        let (ok, records) = events("value = 7; error(\"Test:Failure\", \"failed\");", false, b"");
        assert!(!ok);
        assert_eq!(records[1]["variables"][0]["name"], "value");
        assert_eq!(records[2]["diagnostic"]["code"], "Test:Failure");
        assert_eq!(records.last().unwrap()["ok"], false);
    }

    #[test]
    fn check_does_not_execute_source_and_reports_positioned_parse_errors() {
        let (ok, records) = events("error(\"not executed\");", true, b"");
        assert!(ok);
        assert_eq!(records.len(), 2);
        let (ok, records) = events("disp(\"á\");\nvalue = ;", true, b"");
        assert!(!ok);
        assert_eq!(records[1]["diagnostic"]["line"], 2);
        assert_eq!(records[1]["diagnostic"]["column"], 9);
    }

    #[test]
    fn interactive_pause_requests_input_and_resumes() {
        let (ok, records) = events("disp(1); pause(); disp(2);", false, b"\n");
        assert!(ok);
        assert_eq!(records[1]["type"], "output");
        assert_eq!(records[2]["type"], "input");
        assert_eq!(records[3]["type"], "resumed");
        assert_eq!(records[4]["text"], "2\n");
        assert!(!events("pause();", false, b"").0);
    }

    #[test]
    fn nonfinite_values_are_losslessly_named_in_previews() {
        let (ok, records) = events("A = [0 / 0, 1 / 0, -1 / 0];", false, b"");
        assert!(ok);
        assert_eq!(records[1]["variables"][0]["preview"], json!([["NaN", "Inf", "-Inf"]]));
    }
}
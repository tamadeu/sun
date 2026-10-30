mod studio;
mod studio_session;

use clap::{Parser as ClapParser, Subcommand};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, is_raw_mode_enabled},
};
use std::{
    fs,
    io::{self, BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};
use sun_lexer::lex;
use sun_parser::{parse, ParseError};
use sun_runtime::{execute_with_host, validate_parallel_blocks, RuntimeHost, Session};
use sun_codegen_llvm::compile_to_llvm_ir;

#[derive(Debug, ClapParser)]
#[command(name = "sun", version, about = "Sun — scientific programming for everyone")]
struct Cli { #[command(subcommand)] command: Option<Command> }

struct ConsoleHost<Writer: Write> { writer: Writer }

impl<Writer: Write> RuntimeHost for ConsoleHost<Writer> {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        writeln!(self.writer, "{line}")?;
        self.writer.flush()
    }

    fn wait_for_input(&mut self) -> io::Result<()> {
        self.writer.flush()?;
        let stdin = io::stdin();
        if stdin.is_terminal() { wait_for_terminal_key() }
        else { wait_for_input_line(&mut stdin.lock()) }
    }
}

struct RawModeGuard { restore: bool }

impl RawModeGuard {
    fn new() -> io::Result<Self> {
        let already_enabled = is_raw_mode_enabled()?;
        if !already_enabled { enable_raw_mode()?; }
        Ok(Self { restore: !already_enabled })
    }

    fn restore(&mut self) -> io::Result<()> {
        if self.restore {
            disable_raw_mode()?;
            self.restore = false;
        }
        Ok(())
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) { let _ = self.restore(); }
}

fn pause_key_result(event: Event) -> Option<io::Result<()>> {
    let Event::Key(key) = event else { return None; };
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) { return None; }
    if matches!(key.code, KeyCode::Char('c' | 'C')) && key.modifiers.contains(KeyModifiers::CONTROL) {
        Some(Err(io::Error::new(io::ErrorKind::Interrupted, "interactive pause interrupted")))
    } else { Some(Ok(())) }
}

fn wait_for_terminal_key() -> io::Result<()> {
    let mut guard = RawModeGuard::new()?;
    let result = loop {
        if let Some(result) = pause_key_result(event::read()?) { break result; }
    };
    guard.restore()?;
    result
}

fn wait_for_input_line(input: &mut impl BufRead) -> io::Result<()> {
    let mut line = Vec::new();
    if input.read_until(b'\n', &mut line)? == 0 {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "interactive pause reached end of input"));
    }
    Ok(())
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Execute a .sun source file using the built-in runtime.
    Run { file: PathBuf },
    /// Validate the syntax of a .sun source file.
    Check { file: PathBuf },
    /// Execute or check source using the Sun Studio JSON event protocol.
    Studio { file: PathBuf, #[arg(long)] check: bool },
    /// Keep a Sun Studio workspace and accept protocol-v2 requests on stdin.
    StudioSession,
    /// Print the token stream for a source file.
    Lex { file: PathBuf },
    /// Create a starter Sun source file.
    New { name: Option<String> },
    /// Start an interactive Sun session.
    Repl,
    /// Compile a .sun file to a native executable using LLVM/Clang.
    Compile { file: PathBuf, #[arg(short, long)] output: Option<PathBuf> },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEvent;

    #[test]
    fn repl_defines_multiline_functions_and_calls_them_in_later_commands() {
        let mut input = io::Cursor::new("function result=square(value)\nlocal=value;\nresult=local*local;\nend\ndisp(square(5));\ntry; disp(local); catch failure; disp(failure.identifier); end\n:clear\nsquare(2);\n:quit\n");
        let mut output = Vec::new();
        let mut errors = Vec::new();
        repl_with_io(&mut input, &mut output, &mut errors, false).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("...>"));
        assert!(output.contains("25\n"));
        assert!(output.contains("Sun:UndefinedVariable"));
        assert!(String::from_utf8(errors).unwrap().contains("square"));
    }

    #[test]
    fn repl_executes_new_commands_without_replaying_history() {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut input = io::Cursor::new(b"value = 1; disp(100);\nvalue = value + 1;\ndisp(value);\n:quit\n");
        repl_with_io(&mut input, &mut output, &mut errors, false).unwrap();
        let text = String::from_utf8(output).unwrap();
        assert_eq!(text.matches("100\n").count(), 1);
        assert!(text.contains("2\n"));
        assert!(errors.is_empty());
    }

    #[test]
    fn repl_accumulates_only_the_incomplete_block_and_keeps_state_after_error() {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut input = io::Cursor::new(b"value = 0;\nwhile value < 2\nvalue = value + 1;\nend\nerror(\"stop\");\ndisp(value);\n:clear\ndisp(value);\n:quit\n");
        repl_with_io(&mut input, &mut output, &mut errors, false).unwrap();
        let text = String::from_utf8(output).unwrap();
        let errors = String::from_utf8(errors).unwrap();
        assert!(text.contains("...>"));
        assert!(text.contains("2\n"));
        assert!(text.contains("Workspace cleared."));
        assert!(errors.contains("stop"));
        assert!(errors.contains("undefined variable 'value'"));
    }

    #[test]
    fn repl_pause_consumes_input_without_executing_it_as_source() {
        let mut output = Vec::new();
        let mut errors = Vec::new();
        let mut input = io::Cursor::new(b"value = 7; pause(); value = value + 1;\n\ndisp(value);\n:quit\n");
        repl_with_io(&mut input, &mut output, &mut errors, false).unwrap();
        assert!(String::from_utf8(output).unwrap().contains("8\n"));
        assert!(errors.is_empty());
    }

    #[test]
    fn pause_accepts_keys_but_ignores_releases_and_non_key_events() {
        for code in [KeyCode::Char('x'), KeyCode::Enter, KeyCode::Left] {
            assert!(pause_key_result(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))).unwrap().is_ok());
        }
        assert!(pause_key_result(Event::Resize(80, 24)).is_none());
        let release = KeyEvent::new_with_kind(KeyCode::Char('x'), KeyModifiers::NONE, KeyEventKind::Release);
        assert!(pause_key_result(Event::Key(release)).is_none());
    }

    #[test]
    fn pause_ctrl_c_reports_interruption() {
        let event = Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
        assert_eq!(pause_key_result(event).unwrap().unwrap_err().kind(), io::ErrorKind::Interrupted);
    }

    #[test]
    fn piped_pause_consumes_one_line_and_reports_eof() {
        let mut input = io::Cursor::new(b"\ncontinue\n");
        wait_for_input_line(&mut input).unwrap();
        assert_eq!(input.position(), 1);
        wait_for_input_line(&mut input).unwrap();
        assert_eq!(wait_for_input_line(&mut input).unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[derive(Default)]
    struct TrackingWriter { bytes: Vec<u8>, flushes: usize }

    impl Write for TrackingWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    #[test]
    fn console_host_flushes_each_output_line() {
        let module = parse("value = 1\npause(0); disp(value);").unwrap();
        let mut host = ConsoleHost { writer: TrackingWriter::default() };
        execute_with_host(&module, &mut host).unwrap();
        assert_eq!(host.writer.bytes, b"value = 1\n1\n");
        assert_eq!(host.writer.flushes, 2);
    }

    #[test]
    fn console_host_keeps_output_before_uncaught_error() {
        let module = parse("disp(1); error(\"stop\"); disp(2);").unwrap();
        let mut host = ConsoleHost { writer: TrackingWriter::default() };
        assert!(execute_with_host(&module, &mut host).is_err());
        assert_eq!(host.writer.bytes, b"1\n");
        assert_eq!(host.writer.flushes, 1);
    }
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("sun: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Some(Command::StudioSession) => studio_session::run()?,
        Some(Command::Studio { file, check }) => {
            if !studio::run_file(&file, check)? { std::process::exit(1); }
        }
        Some(Command::Run { file }) => {
            let source = fs::read_to_string(&file)?;
            let module = parse(&source)?;
            let stdout = io::stdout();
            let mut host = ConsoleHost { writer: stdout.lock() };
            execute_with_host(&module, &mut host)?;
        }
        Some(Command::Check { file }) => {
            let source = fs::read_to_string(&file)?;
            let module = parse(&source)?;
            validate_parallel_blocks(&module)?;
            println!("Syntax OK: {} statement(s)", module.statements.len());
        }
        Some(Command::Lex { file }) => {
            let source = fs::read_to_string(&file)?;
            for token in lex(&source)? {
                println!("{:?} @ {}..{}", token.kind, token.span.start, token.span.end);
            }
        }
        Some(Command::New { name }) => create_example(name.as_deref().unwrap_or("hello.sun"))?,
        Some(Command::Repl) => repl()?,
        Some(Command::Compile { file, output }) => compile_file(&file, output.as_deref())?,
        None => {
            println!("Sun {} — scientific programming language", env!("CARGO_PKG_VERSION"));
            println!("Try: sun new hello.sun");
            println!("     sun run hello.sun");
            println!("     sun compile hello.sun");
            println!("     sun check hello.sun");
            println!("     sun repl");
        }
    }
    Ok(())
}

fn compile_file(file: &Path, output: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(file)?;
    let module = parse(&source)?;
    let ir = compile_to_llvm_ir(&module)?;

    let output_path = output.map(PathBuf::from).unwrap_or_else(|| {
        let mut path = file.to_path_buf();
        path.set_extension(if cfg!(windows) { "exe" } else { "" });
        path
    });
    let ir_path = std::env::temp_dir().join(format!("sun-{}-{}.ll", std::process::id(),
        output_path.file_stem().and_then(|s| s.to_str()).unwrap_or("output")));
    fs::write(&ir_path, ir)?;
    // Embed the small C runtime with the executable so generated programs do
    // not depend on a separately installed Sun runtime library.
    let runtime_path = ir_path.with_extension("runtime.c");
    fs::write(&runtime_path, include_str!("../../../runtime/sun_runtime.c"))?;

    let compiler = std::env::var_os("SUN_CLANG").unwrap_or_else(|| "clang".into());
    let result = ProcessCommand::new(compiler)
        .arg("-x").arg("ir").arg(&ir_path)
        .arg("-x").arg("c").arg(&runtime_path)
        .arg("-O2").arg("-lm").arg("-o").arg(&output_path)
        .output();
    let _ = fs::remove_file(&ir_path);
    let _ = fs::remove_file(&runtime_path);
    let result = result.map_err(|error| format!(
        "could not start Clang/LLVM ({error}). Install Clang 18+ or set SUN_CLANG to its executable path."
    ))?;
    if !result.status.success() {
        return Err(format!("LLVM compilation failed:\n{}", String::from_utf8_lossy(&result.stderr)).into());
    }
    println!("Compiled {} -> {}", file.display(), output_path.display());
    Ok(())
}

fn create_example(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(name);
    if path.exists() { return Err(format!("{} already exists", path.display()).into()); }
    let source = "% Welcome to Sun!\nx = 2 + 3 * 4;\ndisp(x);\n";
    fs::write(path, source)?;
    println!("Created {}", path.display());
    println!("Run it with: sun run {}", path.display());
    println!("Compile it with: sun compile {}", path.display());
    Ok(())
}

fn repl() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    repl_with_io(&mut stdin.lock(), &mut io::stdout().lock(), &mut io::stderr().lock(), stdin.is_terminal())?;
    Ok(())
}

struct ReplHost<'a, Input: BufRead, Output: Write> { input: &'a mut Input, output: &'a mut Output, terminal: bool }

impl<Input: BufRead, Output: Write> RuntimeHost for ReplHost<'_, Input, Output> {
    fn write_line(&mut self, line: &str) -> io::Result<()> {
        writeln!(self.output, "{line}")?;
        self.output.flush()
    }
    fn wait_for_input(&mut self) -> io::Result<()> {
        self.output.flush()?;
        if self.terminal { wait_for_terminal_key() } else { wait_for_input_line(self.input) }
    }
}

fn repl_with_io(input: &mut impl BufRead, output: &mut impl Write, errors: &mut impl Write, terminal: bool) -> io::Result<()> {
    writeln!(output, "Sun interactive session - :quit exits, :clear resets, :cancel discards an unfinished block.")?;
    let mut host = ReplHost { input, output, terminal };
    let mut session = Session::new();
    let mut source = String::new();
    loop {
        write!(host.output, "{}", if source.is_empty() { "sun> " } else { "...> " })?;
        host.output.flush()?;
        let mut line = String::new();
        if host.input.read_line(&mut line)? == 0 {
            if !source.is_empty() { if let Err(error) = parse(&source) { writeln!(errors, "sun: {error}")?; } }
            break;
        }
        let input = line.trim();
        if input == ":quit" || input == ":q" { break; }
        if input == ":clear" { session.clear(); source.clear(); writeln!(host.output, "Workspace cleared.")?; continue; }
        if input == ":cancel" { source.clear(); continue; }
        if input.is_empty() && source.is_empty() { continue; }
        source.push_str(&line);
        match parse(&source) {
            Ok(module) => {
                if let Err(error) = session.execute(&module, &mut host) { writeln!(errors, "sun: {error}")?; }
                source.clear();
            }
            Err(ParseError::UnexpectedToken { offset, .. }) if offset == source.len() => {}
            Err(error) => { writeln!(errors, "sun: {error}")?; source.clear(); }
        }
    }
    Ok(())
}

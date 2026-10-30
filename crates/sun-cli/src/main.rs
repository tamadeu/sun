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
use sun_parser::parse;
use sun_runtime::{execute, execute_with_host, validate_parallel_blocks, RuntimeHost};
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
    println!("Sun interactive session — enter an expression, or type :quit to exit.");
    let stdin = io::stdin();
    let mut source = String::new();
    loop {
        print!("sun> ");
        io::stdout().flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 { break; }
        let input = line.trim();
        if input == ":quit" || input == ":q" { break; }
        if input.is_empty() { continue; }
        source.push_str(&line);
        match parse(&source) {
            Ok(module) => match execute(&module) {
                Ok(results) => if let Some(result) = results.last() { println!("{result}"); },
                Err(error) => { eprintln!("{error}"); source.clear(); }
            },
            Err(error) => { eprintln!("{error}"); source.clear(); }
        }
    }
    Ok(())
}

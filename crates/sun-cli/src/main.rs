use clap::{Parser as ClapParser, Subcommand};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
};
use sun_lexer::lex;
use sun_parser::parse;
use sun_runtime::execute;

#[derive(Debug, ClapParser)]
#[command(name = "sun", version, about = "Sun — scientific programming for everyone")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
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
    /// Compile a .sun file to a native executable (not yet available).
    Compile { file: PathBuf, #[arg(short, long)] output: Option<PathBuf> },
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
            for line in execute(&module)? { println!("{line}"); }
        }
        Some(Command::Check { file }) => {
            let source = fs::read_to_string(&file)?;
            let module = parse(&source)?;
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
        Some(Command::Compile { file, output }) => {
            let _ = (file, output);
            return Err("native compilation is not available yet; use 'sun run <file.sun>' to execute programs. Native LLVM code generation is a separate milestone.".into());
        }
        None => {
            println!("Sun {} — scientific programming language", env!("CARGO_PKG_VERSION"));
            println!("Try: sun new hello.sun");
            println!("     sun run hello.sun");
            println!("     sun check hello.sun");
            println!("     sun repl");
        }
    }
    Ok(())
}

fn create_example(name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = Path::new(name);
    if path.exists() { return Err(format!("{} already exists", path.display()).into()); }
    let source = "% Welcome to Sun!\nx = 2 + 3 * 4;\nA = [1, 2; 3, 4];\nB = A * A;\ndisp(x);\ndisp(B);\n";
    fs::write(path, source)?;
    println!("Created {}", path.display());
    println!("Run it with: sun run {}", path.display());
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

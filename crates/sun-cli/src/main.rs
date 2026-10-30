use clap::{Parser as ClapParser, Subcommand};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};
use sun_lexer::lex;
use sun_parser::parse;
use sun_runtime::execute;
use sun_codegen_llvm::compile_to_llvm_ir;

#[derive(Debug, ClapParser)]
#[command(name = "sun", version, about = "Sun — scientific programming for everyone")]
struct Cli { #[command(subcommand)] command: Option<Command> }

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

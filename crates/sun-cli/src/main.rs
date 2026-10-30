use clap::{Parser as ClapParser, Subcommand};
use std::{fs, path::PathBuf};
use sun_lexer::lex;
use sun_parser::parse;

#[derive(Debug, ClapParser)]
#[command(name = "sun", version, about = "Sun scientific programming language")]
struct Cli { #[command(subcommand)] command: Option<Command> }

#[derive(Debug, Subcommand)]
enum Command {
    /// Tokenize a source file and print the token stream.
    Lex { file: PathBuf },
    /// Parse a source file and print whether parsing succeeded.
    Check { file: PathBuf },
}

fn main() {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("sun: {error}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Some(Command::Lex { file }) => {
            let source = fs::read_to_string(&file)?;
            for token in lex(&source)? {
                println!("{:?} @ {}..{}", token.kind, token.span.start, token.span.end);
            }
        }
        Some(Command::Check { file }) => {
            let source = fs::read_to_string(&file)?;
            let module = parse(&source)?;
            println!("Syntax OK: {} statement(s)", module.statements.len());
        }
        None => {
            println!("Sun 0.1 — compiler bootstrap");
            println!("Commands: sun lex <file.sun>, sun check <file.sun>");
            println!("Execution and LLVM code generation are the next milestone.");
        }
    }
    Ok(())
}

# Sun

Sun is an experimental scientific programming language inspired by MATLAB, written in Rust and designed around an LLVM backend.

## Current status

The repository is in the 0.1 bootstrap stage. It contains source spans, an initial lexer, an AST/parser foundation, dense f64 matrix operations with public 1-based indexing, a CLI scaffold and an Inkwell/LLVM backend scaffold.

The LLVM scaffold is not yet a complete compiler or JIT. Semantic analysis, Sun IR, execution, the REPL and plotting remain follow-up milestones.

## Requirements

- Stable Rust, edition 2024 (Rust 1.85 or newer).
- For the LLVM backend: LLVM 18 development libraries and llvm-config on PATH.

## Build and test

    cargo test
    cargo run -p sun -- check examples/matrices.sun
    cargo run -p sun -- lex examples/matrices.sun

The default workspace test set excludes sun-codegen-llvm until LLVM is installed. To test that crate explicitly, run cargo test -p sun-codegen-llvm.

See docs/ARCHITECTURE.md for the architecture and milestones.

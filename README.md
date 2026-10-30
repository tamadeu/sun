# Sun

Sun is an experimental scientific programming language inspired by MATLAB, written in Rust. It is designed around matrix-first computing, 1-based public indexing, and a planned LLVM native backend.

## Install a released binary

Linux (x86-64) and macOS (Apple Silicon or Intel):

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/tamadeu/sun/main/install.sh | sh
```

Windows PowerShell (x86-64):

```powershell
irm https://raw.githubusercontent.com/tamadeu/sun/main/install.ps1 | iex
```

The installer downloads the latest published release. If no release exists yet, build from source using the instructions below. Linux/macOS installs to `~/.local/bin` by default; set `SUN_INSTALL_DIR` to change it. Windows installs to `%USERPROFILE%\.sun\bin`.

## Quick start

Create a sample program and run it:

```sh
sun new hello.sun
sun run hello.sun
```

Example source:

```matlab
% Basic arithmetic and matrix operations
x = 2 + 3 * 4;
A = [1, 2; 3, 4];
B = A * A;
C = A .* A;
disp(x);
disp(B);
disp(C);
```

Other commands:

```sh
sun --version
sun --help
sun check hello.sun
sun lex hello.sun
sun repl
```

The `sun run` command executes programs with the built-in interpreter. `sun compile` generates native executables through LLVM/Clang. Native compilation currently supports scalar arithmetic, dense matrix literals, matrix addition, matrix multiplication, element-wise multiplication, scalar multiplication/division, `disp(...)`, and 1-based matrix reads using `A(row, column)`. Native indices must currently be non-negative integer literals; matrix element assignment and general math functions are not yet supported by the native backend. The interpreter supports a broader set of runtime operations.

## Build from source

Requirements: stable Rust (1.85+). LLVM is not required to build or run the current interpreter.

```sh
git clone https://github.com/tamadeu/sun.git
cd sun
cargo test
cargo run -p sun -- new hello.sun
cargo run -p sun -- run hello.sun
```

To install the CLI from a local checkout:

```sh
cargo install --path crates/sun-cli
```

## Releases

Pushing a version tag such as `v0.1.0` runs tests, builds release binaries for Linux x86-64, macOS Apple Silicon/Intel, and Windows x86-64, then attaches archives to a GitHub Release. The install scripts download these release assets. Until the first release is published, use the source build instructions above.

## Current status

This is an early bootstrap, not yet a full MATLAB replacement. The lexer, parser, matrix primitives, interpreter, scalar LLVM code generation, and initial native matrix code generation are implemented. Semantic analysis, a stable intermediate representation, matrix element assignment, dynamic native indexing, a production-quality persistent REPL, and plotting remain future milestones.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for design notes.

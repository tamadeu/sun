# Sun 0.1 architecture

## Compilation pipeline

source -> lexer -> parser -> AST -> semantic analysis -> Sun IR -> LLVM IR -> JIT/AOT

The AST and Sun IR remain independent from Inkwell. The numeric runtime owns matrix semantics; generated code should call stable runtime functions for matrix operations rather than embedding matrix storage details in LLVM IR.

## Initial decisions

- Rust workspace with independently testable crates.
- Dense f64 matrices for the first release.
- Public matrix indexing is 1-based.
- LLVM is the intended backend; the initial scaffold targets LLVM 18 through Inkwell.
- The initial CLI exposes lex and check; execution is not advertised until JIT and runtime ABI are implemented.
- MATLAB-style comments begin with %.

## Initial syntax

    A = [1, 2; 3, 4];
    B = A * A;
    C = A .* A;

## Milestones

1. Lexer, source spans, AST and parser.
2. LLVM scalar expressions and verified JIT path.
3. Persistent runtime context and REPL.
4. Matrix literals, runtime calls, 1-based indexing and matrix operations.
5. Functions, control flow, math standard library and diagnostics.
6. AOT output and plotting API.

## LLVM prerequisites

The backend crate is configured for LLVM 18 through Inkwell. Install LLVM 18 development files and ensure llvm-config is on PATH before building that crate. Default workspace members temporarily exclude the backend until LLVM is configured.

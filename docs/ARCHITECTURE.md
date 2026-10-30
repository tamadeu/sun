# Sun 0.1 architecture

## Current execution model

source -> lexer -> parser -> AST -> interpreter/runtime

The CLI can currently execute the supported subset through the built-in interpreter. Native execution is planned as:

source -> lexer -> parser -> semantic analysis -> Sun IR -> LLVM IR -> native executable/JIT

The AST and Sun IR should remain independent from Inkwell. The numeric runtime owns matrix semantics; generated code should call stable runtime functions for matrix operations rather than embedding matrix storage details in LLVM IR.

## Initial decisions

- Rust workspace with independently testable crates.
- Dense f64 matrices for the first release.
- Public matrix indexing is 1-based.
- LLVM is the intended native backend; the scaffold targets LLVM 18 through Inkwell.
- The interpreter currently supports numeric expressions, variables, matrix literals, matrix addition/multiplication, element-wise multiplication, scalar multiplication/division, and basic math functions.
- A trailing semicolon suppresses the implicit result; `disp(...)` still writes output.
- MATLAB-style comments begin with %.

## Initial syntax

    A = [1, 2; 3, 4];
    B = A * A;
    C = A .* A;
    disp(B);

## Milestones

1. Lexer, source spans, AST and parser.
2. Interpreter execution for scalar expressions and matrix operations.
3. Packaging, installation scripts, and cross-platform release automation.
4. Semantic analysis and a stable Sun IR.
5. LLVM scalar expressions, native executable generation, and JIT path.
6. Persistent REPL state, broader standard library, and diagnostics.
7. Functions, control flow, indexing, and plotting API.

## LLVM prerequisites

The backend crate is configured for LLVM 18 through Inkwell. Install LLVM 18 development files and ensure llvm-config is on PATH before building that crate. Default workspace members temporarily exclude the backend, so the interpreter and CLI do not require LLVM.

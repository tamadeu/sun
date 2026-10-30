# Sun 0.1 architecture

## Current execution model

source -> lexer -> parser -> AST -> interpreter/runtime

Native execution currently follows:

source -> lexer -> parser -> AST -> LLVM IR -> native executable

Semantic analysis and a stable Sun IR remain planned layers between the AST and LLVM.

Native scalar math calls cross a C ABI bundled into each executable (`runtime/sun_runtime.c`). The runtime now also provides an opaque heap-backed `SunMatrix` ABI with 1-based access, shape queries, arithmetic operations, and explicit ownership. The LLVM generator uses the heap-backed matrix ABI. The numeric crate also exposes borrowed, read-only matrix views for rectangular regions and transposition; these views retain the source allocation through Rust lifetimes and do not copy element data.

## Initial decisions

- Rust workspace with independently testable crates.
- Dense f64 matrices for the first release.
- Public matrix indexing is 1-based.
- LLVM is the native backend; the CLI invokes Clang 18+ to compile generated LLVM IR.
- The interpreter supports numeric expressions, variables, matrix literals, matrix addition and element-wise multiplication with 2D singleton-dimension broadcasting, matrix multiplication, scalar multiplication/division, 1-based matrix reads via `A(row, column)`, and basic math functions, materialized transpose, determinant, inverse, and linear-system solving (`solve(A, B)`) using partial-pivoting Gaussian elimination.
- The LLVM backend supports scalar arithmetic, fixed-shape dense matrix literals/operations, matrix display, compile-time literal 1-based matrix reads, and native math calls through the C runtime ABI.
- A trailing semicolon suppresses the implicit result; `disp(...)` still writes output.
- MATLAB-style comments begin with %.

## Initial syntax

    A = [1, 2; 3, 4];
    B = A * A;
    C = A .* A;
    disp(B);
    disp(A(2, 1));

## Milestones

1. Lexer, source spans, AST and parser.
2. Interpreter execution for scalar expressions and matrix operations.
3. Packaging, installation scripts, and cross-platform release automation.
4. Semantic analysis and a stable Sun IR.
5. LLVM scalar expressions and native executable generation.
6. LLVM fixed-shape matrix code generation, matrix operations, display, and 1-based literal indexing.
7. Initial native C runtime ABI for math functions and self-contained executable linking.\n8. Heap-backed matrices, runtime matrix operations/indexing, element assignment, optimized kernels, user functions, control flow, JIT, and plotting.

## LLVM prerequisites

Install Clang/LLVM 18+ and ensure `clang` is on PATH, or set `SUN_CLANG` to the compiler executable. The Rust backend emits textual LLVM IR, so building the workspace does not require linking LLVM libraries.

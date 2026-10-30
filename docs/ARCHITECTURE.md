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
- The interpreter executes `while ... end` and `if ... elseif ... else ... end` blocks with a shared variable environment and finite scalar numeric conditions. Zero is false; nonzero is true. Conditions are re-evaluated before each loop iteration, and only the first matching conditional branch executes. Logical `&&` and `||` short-circuit and use the same scalar condition rules.
- The parser shares block parsing across modules, loops, and conditional branches. It validates loop context for `break` and `continue`, including in unexecuted branches. Block closing keywords belong to the nearest matching construct.
- Execution propagates `Normal`, `Break`, `Continue`, and `Return` separately from runtime errors. Conditionals and switches forward control flow; the innermost `for` or `while` consumes `Break` and `Continue` but forwards `Return`. The script executor consumes `Return` as successful termination while preserving earlier output. Return takes no value; function-call boundaries and native return compilation remain future work.
- The interpreter supports `for variable = start[:step]:stop` with finite scalar bounds evaluated once. The default step is 1; zero steps are errors. Numeric ranges generate values by iteration index using fused multiply-add, with a bounded quotient-relative rounding tolerance at endpoints, without materializing vectors. Iteration indices must fit the exact integer range of `f64`, and repeated iteration values are rejected. Empty ranges do not assign the variable. General colon expressions and native control flow remain future work.
- A separate `ForEach` AST form represents `for variable = source` without a colon. The interpreter evaluates the numeric source once, retains an owned snapshot, and iterates its columns using the existing matrix view API. Row-vector columns become numbers; multirow columns become independent matrices. Scalar sources iterate once. Empty matrices are still rejected by the numeric core. Both for forms consume loop control identically and are rejected explicitly by the native backend.
- MATLAB-style comments begin with %.
- `IndexEnd` represents `end` used in argument expressions, separately from block delimiters. Scalar matrix reads and assignments evaluate each index under a temporary dimension context: element count for one index, rows/columns for two. Nested indices override and restore it, including on errors. Ordinary function arguments within an index inherit its dimension. Assignment values use their own context. Index validation remains shared for reads and writes. Slice assignments, linear slices, growth and native index-end code generation remain future work.
- `IndexAll` and `IndexRange` represent direct index selectors. Bidimensional slice reads evaluate finite integer bounds in the corresponding axis context and reuse the numeric progression machinery. Endpoints are validated before materializing index lists. `SunMatrix::select` validates the selected coordinates and creates an independent ndarray-backed matrix in requested order. Scalar-only reads remain numbers; reads containing a selector remain matrices. Empty selections follow the existing nonempty-matrix policy and fail explicitly.
- `SunMatrix` owns the 1-based column-major mapping used by `get_linear` and `set_linear`, independently of its ndarray storage order. Linear bounds failures carry index and element count and map to the existing `Sun:IndexOutOfBounds` identifier. `LinearIndexAssign` represents scalar single-index writes without changing the existing bidimensional AST form. Reads use the shared call/index syntax; both linear forms are currently interpreter-only.
- Runtime effects are supplied through `RuntimeHost`: output lines, timed pauses and optional interactive input. `execute_with_host` runs with injected effects; the existing `execute` adapter collects lines while retaining real timed pause behavior. Hosts without input support report an I/O error for enabled interactive waits. The CLI writes and flushes each line before continuing. Tests record effects without wall-clock waits or human input. Host I/O failures map to `Sun:IOError`.
- `ExecutionContext` owns an initially enabled pause flag per execution. `pause("on"/"off")` returns the previous state; `pause("query")` returns the current state. Disabled pauses skip host waits but still validate arguments. Blocks share this state; subsequent executions reset it. `pause(seconds)` accepts finite nonnegative `Duration` values, and `pause()` or an unbound bare `pause` requests interactive input.
- The CLI uses Crossterm for terminal key events with a raw-mode restoration guard, and buffered line input for pipes. Ctrl+C during a key wait reports interrupted I/O, and EOF in a pipe reports unexpected EOF. Native pause compilation and a persistent non-replaying REPL remain future work.
- The interpreter supports `try ... catch [name] ... end` with a required handler and optional exception variable. Only `RuntimeError` results enter catch; normal execution and `Return`/`Break`/`Continue` flow bypass it. Both bodies share the existing environment and output, without rollback. Handler errors propagate outward.
- `SunException` values carry an identifier and message, exposed through read-only `FieldAccess` expressions. Built-in errors map to stable `Sun:...` identifiers; `error(message)` or `error(identifier, message)` creates an exception. `rethrow(exception)` preserves both fields without adding formatting prefixes. Named handlers assign their exception variable only on failure and do not create a separate scope. Stack traces, causes, MATLAB-style formatting and native exception handling remain future work.
- The interpreter evaluates a `switch` selector once and evaluates numeric or string case expressions sequentially until the first exact, same-type match. Only that body executes, with an optional final `otherwise` when no case matches. There is no fall-through. Switch blocks forward loop control rather than consume it. Matrices, cell-array case alternatives and native switch compilation remain unsupported.

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

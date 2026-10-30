# Sun

Sun is an experimental scientific programming language inspired by MATLAB, written in Rust. It is designed around matrix-first computing, 1-based public indexing, and a planned LLVM native backend.

Native compilation bundles a small C runtime ABI (`runtime/sun_runtime.c`) into each executable. The first ABI functions provide `sqrt`, `sin`, `cos`, `exp`, and `log`; matrix allocation and runtime indexing will move behind this boundary in a subsequent milestone.

## Install a released binary

Linux (x86-64):

```sh
curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/tamadeu/sun/main/install.sh | sh
```

Windows PowerShell (x86-64):

```powershell
irm https://raw.githubusercontent.com/tamadeu/sun/main/install.ps1 | iex
```

Prebuilt macOS releases are temporarily disabled. On macOS, build from source using the instructions below. The Linux installer installs to `~/.local/bin` by default; set `SUN_INSTALL_DIR` to change it. Windows installs to `%USERPROFILE%\\.sun\\bin`.

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

The `sun run` command executes programs with the built-in interpreter. `sun compile` generates native executables through LLVM/Clang. Native compilation currently supports scalar arithmetic, dense matrix literals, matrix addition, matrix multiplication, element-wise multiplication, scalar multiplication/division, `disp(...)`, and 1-based matrix reads using `A(row, column)`. Native indices must currently be non-negative integer literals; matrix element assignment and general math functions are not yet supported by the native backend. The numeric core also exposes read-only zero-copy views for rectangular matrix regions and transposition. These are currently a Rust library capability, not yet a user-facing Sun syntax feature. The interpreter supports `transpose(A)`, `det(A)`, `inv(A)`, and `solve(A, B)` for dense matrices, as well as 2D singleton-dimension broadcasting for matrix addition and element-wise multiplication; native LLVM broadcasting is not yet implemented. The interpreter supports a broader set of runtime operations.

## While loops

The interpreter supports `while ... end`, including nested loops:

```matlab
counter = 1;
total = 0;
while counter <= 5
	total = total + counter;
	counter = counter + 1;
end
disp(total);
```

This prints `15`. The condition is evaluated before every iteration and must be
a finite scalar number: zero is false and any other value is true. Variables
updated in the body remain available after the loop. Semicolons inside the body
suppress implicit output as usual; `disp` always prints. The header must end with
a newline or semicolon, and every loop needs its own `end`. `while` and `end`
are reserved keywords.

Run the complete example from a local checkout:

```sh
cargo run -p sun -- check examples/while.sun
cargo run -p sun -- run examples/while.sun
```

Use source files for multiline blocks; the current REPL only reads one line at
a time. `sun compile` does not support loops yet.
There is no iteration limit: ensure that the condition eventually becomes false
or the body executes `break`, or interrupt an infinite loop with Ctrl+C.

## Conditionals and loop control

The interpreter supports `if`, any number of `elseif` branches, and an optional
`else`. Only the first matching branch executes; later conditions and bodies
are not evaluated. Each conditional block needs its own `end`.

```matlab
counter = 0;
while counter < 10
	counter = counter + 1;
	if counter == 3
		continue;
	elseif counter == 7
		break;
	else
		disp(counter);
	end
end
```

This prints `1`, `2`, `4`, `5`, and `6`. `break` exits the innermost loop;
`continue` skips the remaining body and re-evaluates a `while` condition or
advances a `for` to its next iteration value.
Both statements are rejected outside loops during `sun check`, even in branches
that would never execute. They take no arguments. Semicolons inside branches
control implicit output independently of any semicolon after `end`.

As with `while`, conditions must be finite scalar numbers, not matrices or
strings. This is currently a subset of MATLAB's condition semantics. The
operators `&&` and `||` short-circuit: the right operand is evaluated only when
needed, and evaluated operands must be finite scalar numbers. Results are `0`
or `1`. These rules also apply outside conditional headers.

`if`, `elseif`, `else`, `break`, and `continue` are reserved keywords. Headers
must end with a newline or semicolon, including `else`. Native compilation of
conditionals and loop control is not supported yet; use `sun run`.

```sh
cargo run -p sun -- check examples/controle_fluxo.sun
cargo run -p sun -- run examples/controle_fluxo.sun
```

## For loops with ranges

The interpreter supports `for variable = start:stop` and
`for variable = start:step:stop`, including nested loops and `break`/`continue`:

```matlab
total = 0;
for index = 1:5
	total = total + index;
end
disp(total);

for value = 1:-0.2:0
	disp(value);
end
```

The default step is `1`. Start, step and stop are evaluated once, from left to
right, before entering the loop. They must be finite scalar numbers, and step
must not be zero. The stop value is included when reached by the progression;
a small floating-point tolerance handles fractional endpoints such as
`0:0.1:0.3`. A step pointing away from stop produces no iterations. Currently,
an empty range leaves the loop variable unchanged, or undefined if it did not
already exist.

Each iteration assigns the next value independently of changes made to the
loop variable or range bounds inside the body. The variable remains available
after the loop; a body assignment remains visible until the next iteration
overwrites it. Header assignments are not printed implicitly. Semicolons in
the body retain their usual output behavior.

Ranges are generated without allocating a vector or repeatedly accumulating
the step. Ranges exceeding the exact iteration-index capacity of `f64`, or
steps too small to produce distinct iteration values, report a runtime error.
`for` is a reserved keyword. Every block needs `end`, and the header must end
with a newline or semicolon.

Ranges are supported only in `for` headers, not general expressions such as
`values = 1:5`. Native compilation of `for` remains future work.

```sh
cargo run -p sun -- check examples/for.sun
cargo run -p sun -- run examples/for.sun
```

## For loops over matrix columns

The interpreter also accepts `for variable = source`, where source evaluates
to a number or a nonempty numeric matrix:

```matlab
A = [1, 2, 3; 10, 20, 30];
for column = A
	disp(column);
end
```

This prints `[1; 10]`, `[2; 20]`, and `[3; 30]`, in that order. Iteration is by
columns, not by individual elements or storage order. A row vector such as
`[5, 10, 15]` produces a scalar number per iteration. A column vector such as
`[5; 10; 15]` produces one iteration containing the entire column; use
`transpose(...)` to iterate its individual values. A scalar source, including
a `1x1` matrix, produces one numeric iteration.

Source expressions, including calls such as `transpose(A)` and arithmetic
such as `A + A`, are evaluated exactly once before entering the body. The
evaluated matrix is retained as a snapshot. Mutating or reassigning the original
variable does not change future iterations. Each column is copied into an
independent mutable loop variable; changing it does not mutate the source.
Only the current column is materialized at each iteration.

`break`, `continue`, nested loops, and implicit output follow the same rules
as range loops. The last body value of the loop variable remains available
after the loop. Strings and void results are not iterable. Unlike range
bounds, matrix elements and scalar sources may contain non-finite numbers.
Empty matrices (`[]`) are still rejected by the numeric core; this form does
not yet implement MATLAB's empty-array iteration. Native compilation is not
supported.

```sh
cargo run -p sun -- check examples/for_matrizes.sun
cargo run -p sun -- run examples/for_matrizes.sun
```

## Switch selection

The interpreter supports `switch`, `case`, and an optional final `otherwise`:

```matlab
mode = "cooling";
switch mode
	case "heating"
		disp("Heating enabled");
	case "cooling"
		disp("Cooling enabled");
	otherwise
		disp("Unknown mode");
end
```

The selector is evaluated once. Case expressions are evaluated in order until
the first match, and only that case body executes. There is no fall-through,
so cases do not need `break`. If nothing matches, `otherwise` runs when present;
otherwise the statement does nothing. Empty case bodies and empty switches
are accepted. Variables assigned in the selected body remain available afterward.

Selectors and evaluated case expressions must be scalar numbers or strings.
Numeric equality is exact, with no tolerance; strings compare case-sensitively.
Numbers and strings do not match each other. Numeric infinity follows ordinary
equality, while NaN never matches, including itself. Matrices, including `1x1`
matrices, and cell-array alternatives such as `case {1, 2}` are not supported.

`break` and `continue` inside a switch act on the enclosing loop, not the switch.
They are rejected by `sun check` if no loop encloses the statement. Nested
switches, loops, and conditionals are supported. Each switch needs `end`;
headers and labels end with a newline or semicolon. `switch`, `case`, and
`otherwise` are reserved keywords. Native compilation is not yet supported.

```sh
cargo run -p sun -- check examples/switch.sun
cargo run -p sun -- run examples/switch.sun
```

## Returning from scripts

`return` ends the current script normally, without an argument or result value:

```matlab
for value = [10, 40, 60, 80]
	if value > 50
		disp(value);
		return;
	end
end
disp("No matching value found");
```

This prints only `60`. Unlike `break`, `return` exits every enclosing loop,
conditional, and switch, not just the innermost loop. Output produced before
returning is preserved, and `sun run` exits successfully. Existing runtime
errors are not converted into successful returns. A return in an unexecuted
branch has no effect.

`return` is a reserved keyword and must be followed by a newline, semicolon,
or the end of the file. `return 42` and `return()` are not supported. Source
after a return must still be syntactically valid: the whole script is parsed
before execution. Use `sun run` for these scripts; the current REPL replays
accumulated source and does not yet provide proper script-call boundaries.
User-defined functions and native compilation of `return` remain future work.

```sh
cargo run -p sun -- check examples/return.sun
cargo run -p sun -- run examples/return.sun
```

## Handling runtime errors

The interpreter supports `try ... catch ... end` with an optional exception variable:

```matlab
A = [1, 2; 2, 4];
try
	result = inv(A);
catch
	disp("Could not invert the matrix");
	result = [1, 0; 0, 1];
end
disp(result);
```

The first runtime error in the try body skips its remaining statements and
enters the catch body. If no error occurs, catch is skipped. After a successful
handler, execution continues after `end`. Variable updates, matrix mutations,
and output produced before a caught error are preserved; there is no rollback.
An empty catch suppresses the error without performing further actions.

Nested handlers are supported. An error raised in a catch body propagates to
an enclosing try, if any; it does not re-enter the same catch. `return`, `break`,
and `continue` are normal control flow, not errors, and pass through either
body unchanged. Loop-control statements still require an enclosing loop.

Only Sun runtime errors are caught, such as undefined variables, invalid
arguments, incompatible matrix dimensions, and singular matrices. Syntax and
lexing errors are detected before execution and cannot be caught. Host panics
and process termination are not handled by this construct.
Results such as infinity or NaN are not errors by themselves: `1 / 0` does not
enter catch unless a subsequent operation rejects the result.

Both `catch` and `end` are required. The `try` and `catch` headers must end with
a newline or semicolon, and both keywords are reserved. Native compilation of
try/catch is not implemented yet. No `finally` clause is supported.

```sh
cargo run -p sun -- check examples/try_catch.sun
cargo run -p sun -- run examples/try_catch.sun
```

## Named exceptions

A named handler exposes an exception's read-only string fields, `identifier`
and `message`. Use `switch` to handle known identifiers and rethrow others:

```matlab
try
	result = inv([1, 2; 2, 4]);
catch failure
	switch failure.identifier
		case "Sun:SingularMatrix"
			disp(failure.message);
		otherwise
			rethrow(failure);
	end
end
```

The exception variable is assigned only when an error enters that handler.
It shares the script environment and remains available after the block. If
no error occurs, an existing variable of that name is unchanged; a previously
undefined variable remains undefined. Nested handlers using the same name
overwrite that variable. Save a copy first when the outer exception is needed.
Exceptions can be copied or reassigned as values, but fields cannot be assigned.
Field access currently applies only to exceptions, not arbitrary structures.

`error("message")` raises an exception with identifier `Sun:UserError`.
`error("Plant:PressureTooHigh", "message")` uses a custom identifier. Identifiers
must have at least two colon-separated ASCII components; each starts with a
letter and contains only letters, digits, or underscores. Both arguments must
be strings. Messages are literal: formatting arguments and interpolation are
not supported. `error` and `rethrow` are built-in functions, not keywords.

`rethrow(failure)` raises an existing exception value, including a saved copy,
without changing its identifier or message. A new error in a handler replaces
the propagated error; it is not automatically chained to the original one.
An uncaught explicit error exits `sun run` with a nonzero status.

Built-in error identifiers:

| Category | Identifier |
| --- | --- |
| Undefined variable | `Sun:UndefinedVariable` |
| Unsupported operation | `Sun:UnsupportedOperation` |
| Argument count | `Sun:InvalidArgumentCount` |
| Invalid argument or exception field | `Sun:InvalidArgument` |
| Runtime host I/O failure | `Sun:IOError` |
| Invalid range | `Sun:InvalidForRange` |
| Empty matrix | `Sun:EmptyMatrix` |
| Ragged matrix rows | `Sun:RaggedMatrixRows` |
| Matrix index out of bounds (linear or 2D) | `Sun:IndexOutOfBounds` |
| Incompatible dimensions | `Sun:IncompatibleDimensions` |
| Singular matrix | `Sun:SingularMatrix` |

Exception objects currently carry only identifier and message, not stack traces
or causes. `MException` construction, MATLAB-compatible formatting and native
exception handling remain future work. `disp(failure)` prints the identifier
and message; `disp(failure.message)` prints only the message.

```sh
cargo run -p sun -- check examples/excecoes.sun
cargo run -p sun -- run examples/excecoes.sun
```

## Timed pauses

The interpreter supports `pause(seconds)` with a finite, nonnegative scalar
number of seconds, including fractional durations:

```matlab
disp("Starting");
pause(0.1);
disp("Finished");
```

`sun run` emits and flushes each output line immediately, so `Starting` is
visible before the pause. Earlier output also remains visible if a later
statement raises an uncaught error. This streaming behavior applies to `disp`
and implicit results throughout the script, not just around pauses.

`pause(0)` is valid and does not wait. Timed pauses do not return a result or
print anything by themselves. Negative, non-finite or unrepresentable numeric
durations and invalid argument types raise `Sun:InvalidArgument`; extra
arguments raise `Sun:InvalidArgumentCount`. Host I/O failures raise `Sun:IOError`. These errors
can be handled with `try/catch` as usual. Timing has nanosecond representation
but actual wake-up accuracy depends on the operating system and scheduling.

The runtime exposes `execute_with_host(module, host)` and `RuntimeHost` for
embedding. Hosts write output, perform timed pauses, and optionally wait for
input; tests replace these effects without real waits or human interaction.
The existing `execute(module)` API still collects output and performs timed
pauses. Enabled interactive pauses require a host implementing `wait_for_input`;
the default implementation reports `Sun:IOError` rather than blocking.

Use script files for this feature: the current REPL still replays accumulated
source. Native pause compilation is not implemented yet. `pause` is a built-in
function, not a reserved keyword.

```sh
cargo run -p sun -- check examples/pause.sun
cargo run -p sun -- run examples/pause.sun
```

## Pause controls and input

`pause()` waits for a key in an interactive terminal, without requiring Enter.
Bare `pause` has the same effect when no variable named `pause` exists; an
existing variable takes precedence for bare identifier expressions.

```matlab
previous = pause("off");
pause(60);
pause();
disp(pause("query"));
pause(previous);
disp("Press a key to continue");
pause;
```

Each execution starts with pauses enabled (`"on"`). `pause("off")` disables
both timed and interactive waits; `pause("on")` re-enables them. Both return
the previous state, so it can be saved and restored. `pause("query")` returns
the current state without changing it. All blocks and handlers share that
state within a script; separate calls to the executor reset it. State calls
follow normal implicit-output rules, so use `;` when discarding their result.
States are case-sensitive, and arguments are still validated when pauses are
disabled. Use the parenthesized string forms, not command syntax `pause off`.

The CLI flushes output before reading input. Terminal input uses Crossterm,
restoring the previous terminal mode after the pause. Key-release, mouse and
resize events do not resume execution. Ctrl+C during this key wait reports
an interrupted `Sun:IOError`, which can be handled by `try/catch`.

For piped or redirected input, each enabled interactive pause consumes one
line. End-of-input reports `Sun:IOError` instead of waiting indefinitely.
Disable pauses to run unattended, or supply one line per interactive pause.

```sh
cargo run -p sun -- run examples/pause_interativa.sun
printf '\n\n' | cargo run -p sun -- run examples/pause_interativa.sun
```

## End in matrix indices

The interpreter supports `end` in two-dimensional scalar matrix indices:

```matlab
A = [1, 2, 3; 4, 5, 6];
disp(A(end, 1));
disp(A(1, end));
A(end, end) = 99;
disp(A(end - 1, end - 1));
```

In the first index, `end` is the number of rows; in the second, it is the
number of columns. It uses the target matrix's current shape, not the shape
of a previous assignment. Arithmetic such as `end - 1` is supported in both
reads and assignments. Index results must still be positive finite integers
within the matrix bounds. `end + 1` does not grow a matrix automatically.

Nested indexing establishes its own dimensions. In `A(B(end, 1), end)`, the
inner `end` refers to B's rows, and the final `end` refers to A's columns.
Function arguments inside an index inherit the active dimension, as in
`A(1, sqrt(end))`, provided the resulting index is valid. The temporary context
is restored even when evaluation fails and enters a catch handler. Assignment
values do not inherit the left-hand index context; matrix reads on the right
establish their own context.

`end` continues to close blocks and is not a standalone numeric variable.
The parser permits it inside call arguments because matrix indexing and
function calls share syntax; the runtime rejects it when no matrix-index
context exists, such as `disp(end)` or `sqrt(end)` outside an index.

The bidimensional form uses `A(row, column)` with a named matrix target.
Single-index access and bidimensional slice reads are also available, as
described below. Matrix growth, empty matrices, and native compilation of
index `end` remain unsupported.

```sh
cargo run -p sun -- check examples/indices_end.sun
cargo run -p sun -- run examples/indices_end.sun
```

## Linear matrix indexing

The interpreter supports `A(index)` for reads and assignments, using 1-based
column-major order as in MATLAB, independently of internal matrix storage:

```matlab
A = [1, 2, 3; 4, 5, 6];
disp(A(2));
disp(A(end));
disp(A(end - 1));
A(2) = 40;
disp(A);
```

The output is `4`, `6`, `3`, and `[1, 2, 3; 40, 5, 6]`. A single-index `end`
is the total number of elements. With two indices, `end` still refers to rows
or columns. Row vectors, column vectors, scalar matrices, and materialized
transposes use the same column-major rule. The current matrix shape determines
the element count on each access.

Index expressions may use variables, arithmetic, function calls and nested
matrix accesses. Nested single-index and bidimensional accesses each establish
and restore their own `end` context, including when evaluation raises an error.
The right-hand side of an assignment does not inherit its target's index
context; its own matrix accesses establish the appropriate dimensions.

Indices must be finite positive integers and must not exceed the element
count. Out-of-range access raises `Sun:IndexOutOfBounds`. Writes require an
existing matrix and a numeric scalar value. They preserve the matrix shape:
`A(end + 1) = value` does not append an element. Linear slices, vector or
logical-array indices, deletion, matrix growth and native linear indexing are
not implemented yet. Ordinary named function calls keep their existing meaning.

```sh
cargo run -p sun -- check examples/indices_lineares.sun
cargo run -p sun -- run examples/indices_lineares.sun
```

## Matrix slice reads

The interpreter supports bidimensional slice reads with complete axes and
integer ranges:

```matlab
A = [1, 2, 3; 4, 5, 6; 7, 8, 9];
disp(A(2:end, :));
disp(A(:, end));
disp(A(end, :));
disp(A(1:2:end, end:-1:1));
```

`:` selects the whole axis. `start:stop` uses step 1, and `start:step:stop`
supports positive or negative integer steps. Only reached values are selected;
the stop value need not occur in the progression. Start, step and stop must
be finite integers, and step cannot be zero. Every selected index must be
positive and within the corresponding matrix dimension. Invalid bounds are
checked before allocating the index list.

Each axis resolves `end` independently, including arithmetic, function calls
and nested matrix reads inside bounds. Index contexts are restored after both
successful evaluation and errors. Selection order follows the requested row
and column progressions, including reversed axes.

A read containing `:` or a range returns a materialized matrix, even for a
single selected element. Scalar-only reads keep returning a number. A selected
row has shape `1xN`; a selected column has shape `Nx1`. Results are independent
copies: modifying a variable holding a slice does not modify the source.

This stage supports reads only. Slice assignments, linear slices such as
`A(:)` or `A(2:end)`, vector/logical indexing, and native slice compilation are
not implemented. Ranges and `:` are direct matrix-index syntax, not general
expression values. Empty selections still raise `Sun:EmptyMatrix`, because
the numeric core currently requires nonempty matrices. Fractional range
components are not accepted, even when a particular progression could yield
only integer indices.

```sh
cargo run -p sun -- check examples/fatias.sun
cargo run -p sun -- run examples/fatias.sun
```

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

Pushing a version tag such as `v0.1.0` runs tests and builds release binaries for Linux x86-64 and Windows x86-64, then attaches archives to a GitHub Release. macOS builds are temporarily disabled. The install scripts download these release assets. Until the first release is published, use the source build instructions above.

## Current status

This is an early bootstrap, not yet a full MATLAB replacement. The lexer, parser, matrix primitives, interpreter, scalar LLVM code generation, and initial native matrix code generation are implemented. Semantic analysis, a stable intermediate representation, matrix element assignment, dynamic native indexing, a production-quality persistent REPL, and plotting remain future milestones.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for design notes.

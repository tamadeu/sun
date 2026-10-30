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

The REPL accepts multiline blocks, showing `...>` until the block is complete.
`sun compile` does not support loops yet.
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

As with `while`, conditions must be booleans or finite scalar numbers, not matrices or
strings. This is currently a subset of MATLAB's condition semantics. The
operators `&&` and `||` short-circuit: the right operand is evaluated only when
needed, and evaluated operands must be booleans or finite scalar numbers.
Comparisons and logical operators return real booleans, displayed as `true`
or `false`. These rules also apply outside conditional headers.

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

Outside loop headers, ranges such as `values = 1:5` materialize row vectors.
Loop-header ranges retain lazy generation. Native compilation of ranges and
`for` remains future work.

```sh
cargo run -p sun -- check examples/for.sun
cargo run -p sun -- run examples/for.sun
```

## For loops over matrix columns

The interpreter also accepts `for variable = source`, where source evaluates
to a number or a numeric matrix:

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
Sources with zero elements, including `0xN` and `Nx0` matrices, execute no
iterations and leave the loop variable unchanged. Native compilation is not
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
before execution. In the REPL and Studio console, `return` ends only the current
command, preserving the session for subsequent commands.
Functions consume `return` at the current call boundary. Native compilation of
`return` remains future work.

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
| Unsupported or unsafe parallel-loop form | `Sun:InvalidParFor` |
| Runtime host I/O failure | `Sun:IOError` |
| Invalid range | `Sun:InvalidForRange` |
| Legacy nonempty-operation error | `Sun:EmptyMatrix` |
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

The REPL executes each completed command once; earlier pauses and output are
not replayed. Native pause compilation is not implemented yet. `pause` is a built-in
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

Each new session starts with pauses enabled (`"on"`). `pause("off")` disables
both timed and interactive waits; `pause("on")` re-enables them. Both return
the previous state, so it can be saved and restored. `pause("query")` returns
the current state without changing it. All blocks and handlers share that
state within a script. REPL and Studio commands retain it until the session is
cleared; standalone executor calls start fresh. State calls
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
described below. Matrix growth and native compilation of
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
`A(end + 1) = value` does not append an element. Numeric vector indices are
supported, including stored intervals and expressions returning vectors.
Logical-array indices, deletion, matrix growth and native linear indexing are
not implemented yet. Ordinary named function calls keep their existing meaning.

```sh
cargo run -p sun -- check examples/indices_lineares.sun
cargo run -p sun -- run examples/indices_lineares.sun
```

## Matrix slices

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

Bidimensional slice assignments are also supported, as described below.
Linear slices such as `A(:)` and `A(2:end)` are also supported, as described
below. Numeric vector indexing is also supported; logical indexing and native
slice compilation are not implemented. Ranges are general expression values,
while bare `:` remains direct matrix-index syntax. Empty selections preserve their axis dimensions, as
described below. Fractional range
components are not accepted, even when a particular progression could yield
only integer indices.

```sh
cargo run -p sun -- check examples/fatias.sun
cargo run -p sun -- run examples/fatias.sun
```

## Assigning matrix slices

Bidimensional slice assignments accept numeric scalar fills or a matrix whose
shape exactly matches the selected rows and columns:

```matlab
A = [1, 2, 3; 4, 5, 6; 7, 8, 9];
A(:, end) = 0;
A(1, 1:2) = [10, 20];
A(2:end, 1:2) = [40, 50; 70, 80];
disp(A);
```

Scalar numbers and `1x1` matrices fill every selected position. Other matrices
must match both dimensions and orientation exactly: no implicit reshaping or
row/column broadcasting is performed. Mismatches raise
`Sun:IncompatibleDimensions`. Steps, reversed axes, nested indices, and `end`
use the same selection rules as reads. Values map to positions in the order
specified by the row and column selectors.

The right-hand side is fully evaluated before writing, so overlapping
assignments such as `A(:, 1:2) = A(:, 2:3)` and `A(end:-1:1, :) = A` use a
snapshot of the source values. Target bounds and value shape are validated
before the first write; validation failures do not partially modify the
destination. This does not roll back earlier side effects of evaluating
expressions or previous statements. The right-hand side resolves its own
`end` contexts rather than inheriting the target's dimensions.

Targets must already be matrices. Writes preserve shape, do not grow or delete
elements, and follow the existing indexed-assignment output behavior. Empty
selections accept scalar or empty values without modifying the target. Linear slice assignments use element counts rather
than exact shape, as described below. Native slice compilation remains unsupported.

```sh
cargo run -p sun -- check examples/atribuicao_fatias.sun
cargo run -p sun -- run examples/atribuicao_fatias.sun
```

## Linear slices

Single-index slices follow the same 1-based column-major order as scalar
linear indexing:

```matlab
A = [1, 2, 3; 4, 5, 6];
disp(A(:));
disp(A(2:end));
disp(A(end:-1:1));
A(1:2:end) = 0;
```

`A(:)` always returns a column containing all elements, here
`[1; 4; 2; 5; 3; 6]`. A range such as `A(2:end)` returns a row when A is a
matrix or row vector, here `[4, 2, 5, 3, 6]`; if A is a column vector, the
result remains a column. Slices always produce independent matrix values,
including when selecting one element. Scalar-only `A(index)` still returns a
number. The `end` value is the current total element count, even inside bounds
containing nested matrix reads or function calls.

Linear slice assignments accept a scalar number, a `1x1` matrix, or a matrix
with exactly as many elements as the selection. Unlike bidimensional writes,
the RHS shape and orientation need not match: its values are read by columns.
For example, assigning a `3x2` RHS to `A(:)` for a `2x3` target preserves A's
`2x3` shape and copies the six RHS values in column-major order. Count mismatches
raise `Sun:IncompatibleDimensions`.

Writes validate all indices and the RHS count before mutating the destination.
The RHS is evaluated first as an independent value, so overlapping operations
such as `A(2:end) = A(1:end - 1)` are safe. Scalar fills, reversed ranges and
integer steps are supported without growing or reshaping the target.

The existing slice restrictions still apply: components must be finite
integers, step must be nonzero, and selected indices must be positive and in bounds.
Empty selections are valid matrix values. Logical indices, deletion,
growth and native linear
slice compilation remain unsupported.

```sh
cargo run -p sun -- check examples/fatias_lineares.sun
cargo run -p sun -- run examples/fatias_lineares.sun
```

## Empty matrices and shape queries

The interpreter supports `[]` as a `0x0` numeric matrix. Empty selections retain
their dimensions, and all empty matrices display as `[]`:

```matlab
A = [1, 2, 3; 4, 5, 6];
no_rows = A(2:1, :);
no_columns = A(:, 2:1);
disp(size(no_rows));
disp(size(no_columns));
disp(isempty(no_rows));
disp(numel(no_columns));
```

This prints `[0, 3]`, `[2, 0]`, `true`, and `0`. `size(value)` returns a two-element
row matrix containing rows and columns. `numel(value)` returns the element
count, and `isempty(value)` returns boolean `true` if there are no elements, else
`false`. These initial forms accept exactly one boolean, numeric scalar or matrix argument;
a scalar has size `[1, 1]` and one element. Dimension-specific size queries,
multiple outputs, and string/object queries are not implemented.

Transposition, scalar multiplication/division, compatible singleton-dimension
broadcasting, and matrix products preserve empty shapes. In particular,
`Mx0 * 0xN` produces an `MxN` matrix of zeros. `sum([])` is positive zero,
`det([])` is `1`, and `inv([])` is `0x0`. Solving a compatible system with zero
equations or zero RHS columns returns the correctly shaped empty result.
Incompatible dimensions remain errors.

Every zero-element source skips a `for` body, including `0xN` sources, and
leaves an existing loop variable unchanged. Conditions require booleans or finite
scalar numbers: use `if isempty(value)` rather than a matrix condition.
`end` on a zero-length axis evaluates to zero, so scalar indexing such as
`empty(end)` is invalid. `empty(:)` is `0x1`; range slices preserve the existing
row/column orientation rules. A nonempty index on either axis is still checked,
even if the other axis is empty.

Assignments selecting zero elements accept a scalar fill or any empty numeric
matrix as a no-op. Non-scalar, nonempty RHS values are rejected. No deletion
semantics are introduced: assigning `[]` to a nonempty selection fails with
`Sun:IncompatibleDimensions` and leaves the target unchanged. No operation
grows the matrix implicitly.

`[]` and empty slices no longer raise `Sun:EmptyMatrix`; that identifier remains
reserved for compatibility. Native empty matrices, native shape queries,
matrix growth, and deletion are not supported yet.

```sh
cargo run -p sun -- check examples/matrizes_vazias.sun
cargo run -p sun -- run examples/matrizes_vazias.sun
```

## Initial parallel loops

The interpreter supports a conservative `parfor` subset using Rayon workers:

```matlab
input = [2, 3, 4, 5];
output = [0, 0, 0, 0];
parfor index = 1:4
	squared = input(index) * input(index);
	output(index) = squared;
end
disp(output);
```

This prints `[4, 9, 16, 25]`. Iteration order is unspecified. The range is
evaluated once and must have finite integer bounds with step `1` or `-1`.
Only the plain `parfor variable = start[:step]:stop ... end` form is supported.
Rayon uses the available worker pool; `RAYON_NUM_THREADS` can configure its
default size. A single available worker remains valid, but speedups are not
guaranteed, especially for small loops.

The initial body supports only local temporary assignments ending with `;`
and numeric scalar writes to preallocated matrices. Accepted output patterns
are `output(index)`, `output(index, literal)` and `output(literal, index)`,
where `index` is exactly the iteration variable and `literal` is a positive
integer literal. Each output matrix must use one consistent pattern throughout
the loop. At least one output is required. Neither slices nor computed output
indices are accepted yet.

Output matrices are write-only inside the body. Read-only input matrices and
scalars are broadcast snapshots, copied once per worker partition. Reading
matrix elements does not clone the full input on every access. Temporaries
must be assigned before use in each iteration, must not already exist outside
the loop, and are not exported afterward. The outer iteration variable is
also left unchanged. All syntactically referenced inputs must exist, even if
an expression could short-circuit or the iteration range is empty.

Numerical expressions, input matrix reads/slices, and the supported pure math,
linear-algebra and shape-query functions are allowed. Reductions across
iterations, reads of output matrices, modification of the loop variable,
`disp`, `pause`, `error`, `rethrow`, nested control blocks, nested `parfor`,
`break`, `continue`, and `return` are rejected. Compute shared reductions after
the loop, for example `sum(output)`. This analysis is intentionally conservative
and may reject independent programs outside this first subset.

`sun check` validates structural independence and pure expressions. Execution
performs that validation before any script statements, including for loops in
unexecuted branches. Such static failures cannot be caught by a script's
`try/catch`. Binding, range and target-bound checks happen before starting
workers and can be caught normally. Constraint failures use `Sun:InvalidParFor`;
ordinary matrix and expression errors retain their existing identifiers.

Workers stage scalar writes without modifying the script environment. Results
are applied only when every iteration succeeds; a worker error leaves all
output matrices unchanged. Earlier script effects are not rolled back, and
when several workers fail the selected error is not guaranteed to be the first
one in sequential iteration order. Staging and input snapshots require extra
memory. Native compilation, reduction variables, sliced-output blocks and
MATLAB worker-limit syntax remain future work. Both script files and completed
REPL blocks execute without replaying previous commands.

```sh
cargo run -p sun -- check examples/parfor.sun
RAYON_NUM_THREADS=2 cargo run -p sun -- run examples/parfor.sun
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

This is an early bootstrap, not yet a full MATLAB replacement. The lexer, parser, matrix primitives and assignment, interpreter, user-defined functions, persistent REPL, numeric CSV, initial 2D/3D plotting in Studio, scalar LLVM code generation, and initial native matrix code generation are implemented. Semantic analysis, a stable intermediate representation, dynamic native indexing, and richer graphics remain future milestones.

### Interpreter debugger

Sun Studio can debug editor buffers, including unsaved scripts, using the real
Rust interpreter. Click the editor glyph margin or press F9 to toggle a
breakpoint, then use Debug/F5. Execution stops before the first executable
instruction; Continue/F5 runs to a breakpoint or completion. F10 steps over a
call, F11 steps into user functions, and Shift+F11 steps out. Ctrl/Cmd+F5 keeps
ordinary Run behavior. The pause button requests a stop at the next instruction
boundary, and Shift+F5/Stop cancels execution.

The Debug panel shows the call stack and variables of the selected frame.
Selecting a caller inspects its suspended scope without changing where the
program will resume. Matrices retain their full dimensions with a bounded 5x5
preview; inspection never evaluates Sun expressions or mutates variables.
The global Workspace still refreshes on completion; select the script frame
to inspect its current values while paused. Each frame reports up to 100
variables, names up to 200 characters and bounded text previews. The existing
32-call recursion limit permits a script frame plus 32 function frames.

Functions retain their original source across session commands. Source
identities are SHA-256 hashes of source name and contents, not positions in
recent command history. If the editor has changed since a function definition,
the debugger opens a read-only source snapshot instead of overwriting edits or
highlighting lines in the wrong version. Editor text is read-only during a
debug run. Breakpoints apply to exact source versions; comments, delimiters and
other nonexecutable lines are shown as unverified. A breakpoint can still stop
inside a call when stepping over or out.

Stepping is statement-based: several instructions on one line can produce
several stops on that line. Loop headers are revisited, including empty loops.
Parallel loops are atomic to this debugger; worker-body breakpoints and native
code debugging are not implemented. Long individual numeric/filesystem calls
may not pause immediately. Cancellation unwinds function locals while retaining
completed script assignments and effects; forced engine termination loses the
session explicitly. Errors caught by Sun try/catch unwind frames normally.
Conditional breakpoints, hit counts, watches, expression evaluation, variable
editing, stop-on-exception and time-travel debugging remain future work.

Open `examples/depuracao.sun`, set a breakpoint on line 7, and debug it to inspect
`valor`, `local` and `matriz` inside `dobro`. Its final result is 8. Checks and
ordinary runs do not activate breakpoints. The integrated debugger requires a
session engine advertising the `debugger` capability; an older engine is
rejected without discarding its existing Workspace.

### 2D figures

Run plotting source in Sun Studio, or embed a `RuntimeHost` with figure support:

```matlab
x = 0:0.1:6;
plot(x, sin(x));
title("Seno");
xlabel("Tempo (s)");
ylabel("Amplitude");
legend("Seno");
grid(true);
axis([0, 6, -1.2, 1.2]);
```

`plot(x,y)` accepts equal-length nonempty numeric vectors (row or column), or
two numeric scalars. With a nonvector matrix `y`, each column is a curve and
its row count must match the vector `x`. For example:

```matlab
curvas = [transpose(sin(x)), transpose(cos(x))];
plot(x, curvas);
legend("Seno", "Cosseno");
```

Rust validates coordinates, dimensions and commands, retains one active figure
in the session, and emits typed figure events. Studio renders these events with
locally bundled Plotly.js; it does not evaluate Sun source or recreate language
semantics in JavaScript. Actual curve data is separate from the bounded
Workspace previews and is not silently truncated. The initial limits are 16
curves and 10,000 total points per figure. NaN/infinite coordinates become gaps;
every curve must retain at least one finite coordinate pair. Labels are plain
strings limited to 512 UTF-8 bytes and are escaped for display.

`title`, `xlabel` and `ylabel` each accept one string after a figure exists.
`legend` accepts one string per curve, or `"on"`/`"off"` to show/hide it.
`grid(true)` / `grid(false)` and `grid("on")` / `grid("off")` set the grid;
`grid()` toggles it. `axis([xmin,xmax,ymin,ymax])` sets finite increasing bounds,
and `axis("auto")` restores automatic bounds, including after interactive zoom.
Use parenthesized calls, not command syntax such as `grid on`.

Console commands and editor scripts share the same figure. A new `plot` replaces
it and resets labels/settings; later metadata commands update it without
retransmitting points. Changes to labels/theme preserve interactive zoom, while
explicit axis commands reset the relevant view. Plotly provides zoom, pan,
legend interaction, axis reset and PNG export. Large legends scroll, and long
display labels may be abbreviated; the original text is available in the
figure tooltip. `clf()` clears the figure. Session reset, engine replacement,
renderer loss and application shutdown discard it; cooperative cancellation or
a later runtime error preserves completed figure effects.

Plotting commands return no value and are forbidden inside `parfor`. `check`
does not create or modify figures. A plain terminal host reports an explicit
error directing users to Sun Studio rather than silently discarding graphics.
The native backend also rejects these commands. Multiple simultaneous figures,
`hold`, subplots, style strings, logarithmic axes and animations
are not implemented in this initial version.

Open and execute `examples/graficos.sun` in Studio. Syntax can also be checked:

```sh
cargo run -p sun -- check examples/graficos.sun
```

### 3D curves and surfaces

Sun Studio supports `plot3(x,y,z)` and `surf` through locally bundled Three.js.
The Rust runtime validates data and builds curve connectivity or surface
triangles; JavaScript only projects and renders the resulting figure events.
No Sun expressions are evaluated in the renderer. Existing 2D figures continue
using Plotly and do not load the Three.js renderer until needed.

```matlab
t = 0:0.1:12;
plot3(cos(t), sin(t), t);
title("Helice");
zlabel("Altura");
grid(true);
```

`plot3` accepts three equal-length numeric vectors or three scalars. For several
curves, X/Y/Z may instead be matrices of the same shape, with one curve per
column. Mixing coordinate vectors with nonvector matrices is not supported.
Nonfinite triples produce gaps, and each curve needs a finite point. Existing
limits of 16 curves and 10,000 total input points apply.

Surfaces accept `surf(Z)`, `surf(x,y,Z)` with x/y vectors matching the columns
and rows of Z, or `surf(X,Y,Z)` with equal-shaped coordinate matrices. Z needs
at least 2x2 cells and at most 10,000 grid points. The one-argument form uses
1-based column/row coordinates. Rust triangulates adjacent cells, omits
triangles touching nonfinite data, rejects fully degenerate surfaces and
removes vertices not referenced by any surviving face. This keeps the visible
bounds independent of isolated unusable samples.

```matlab
x = -2:0.2:2;
y = transpose(x);
Z = x .* x + y .* y;
surf(x, y, Z);
title("Paraboloide");
xlabel("X"); ylabel("Y"); zlabel("Z");
grid(true);
axis([-2, 2, -2, 2, 0, 8]);
```

Titles, labels, legend, grid, `clf()` and session lifetime are shared with 2D
figures. `zlabel` requires 3D, and `axis` takes six increasing bounds in 3D:
`[xmin,xmax,ymin,ymax,zmin,zmax]`. `axis("auto")` restores data bounds. A new
plot or explicit axis command restores the initial camera; metadata/theme
changes preserve an interactive camera. Orbit controls support rotation, zoom
and pan, with buttons for camera reset and PNG export. Export includes title,
axis labels, legend and the surface height-color scale, up to 2x viewport
resolution capped at 4096 pixels per dimension.

Rendering maps each axis independently into a unit cube; it does not imply
equal physical axis scales. Camera projection is perspective. View clipping
uses double precision before Float32 GPU conversion to preserve intersections
with very large coordinate magnitudes. Tick and color-scale precision adapts
to distinguish nearby values. Text may be shortened and crowded ticks hidden
to fit narrow views; full metadata remains in the figure tooltip.

WebGL2 is required, through a GPU or a working software implementation. An
unavailable/lost context reports an explicit error rather than substituting a
fake chart. Contexts, controls, geometry and materials are released on clear,
session reset and 2D/3D replacement. The headless Linux E2E harness uses
the application's explicit software-rendering mode; normal launches do not
enable unsafe software-rendering flags unless the user opts in. If Chromium
reports `Error creating WebGL context`, save work, close Studio and try
`sun-studio --software-rendering`, or use the error panel's software/restart
action. The confirmation explains the CPU/security tradeoff and loss of
unsaved/session state. This setting requires restarting Studio, not just its
Rust engine. `--automatic-rendering` selects the normal default again.
Surface color maps and point picking are not configurable yet. `mesh`,
`meshgrid`, `scatter3`, transparency options, equal-axis aspect, orthographic
projection, `hold`, subplots and multiple active figures remain future work.
3D effects are interpreter-only and forbidden in `parfor`, like 2D effects.

Open `examples/graficos_3d.sun` or `examples/superficie_3d.sun` in Studio.
Syntax checking remains available without a graphics host:

```sh
cargo run -p sun -- check examples/graficos_3d.sun
cargo run -p sun -- check examples/superficie_3d.sun
```

### Numeric CSV import and export

The interpreter uses the Rust `csv` crate for numeric matrix files:

```matlab
A = readmatrix("dados.csv");
writematrix(A * 2, "resultado.csv");
```

`readmatrix(filename)` returns a numeric matrix, including `1x1` for a single
field. `writematrix(matrix, filename)` accepts a numeric matrix or scalar and
returns no value; use it as a statement. Both require explicit filenames.
Files use UTF-8, comma separators, decimal points and no headers. LF/CRLF,
an optional UTF-8 BOM and quoted numeric fields are accepted; surrounding
field whitespace is trimmed. Rows must have the same number of fields, and
every field must parse as `f64`. Headers, text, empty/missing fields, malformed
quotes and invalid UTF-8 raise `Sun:InvalidCSV`, not inferred or padded data.
Blank lines are ignored. Scientific notation and explicit NaN/Inf values are
supported. Export uses LF, preserves finite `f64` round-trip precision and
writes nonfinite values as `NaN`, `Inf` and `-Inf`.

An empty file imports as `0x0`; writing `[]` creates an empty file. Other empty
shapes such as `0x3` or `3x0` cannot be preserved by plain numeric CSV and are
rejected before writing. The initial implementation limits files to 64 MiB,
records to 1 MiB, columns to 65,536 and matrices to the existing 16,777,216
element/dimension limit. Reading validates record limits before parser buffer
growth and accumulates numbers in one fallible row-major buffer.

Paths are relative to the process working directory. `sun run` and `sun repl`
use the terminal's directory; Studio uses the opened script's directory, then
the selected project folder, then the engine checkout. Absolute paths also work.
Missing files, inaccessible paths and filesystem failures raise `Sun:IOError`.
Errors are catchable and a failed import does not replace the destination
variable. `check` does not open or modify CSV files.

Export replaces an existing regular file atomically using a temporary file in
the same directory; invalid data, serialization failure or cancellation before
commit does not replace it with partial content. Existing permissions are
preserved; new files use the temporary-file library's private default.
Symbolic-link destinations, directories and special files are rejected rather
than replaced. Input must resolve to a regular file; reading a symlink to a
regular file is permitted. Cancellation is checked between reads, fields and
records and before committing output. An OS filesystem operation already
blocked inside a host call is not guaranteed to stop immediately; Studio's
existing forced-stop fallback still applies. Completed exports are not rolled
back by a later script error.

These functions perform filesystem effects through overridable `RuntimeHost`
methods. Sun scripts are trusted code and may access paths outside Studio's
file explorer; renderer IPC file authorization is not an interpreter sandbox.
Embedded hosts can deny or redirect file access. CSV effects are rejected in
`parfor`, and the native backend reports interpreter guidance. Delimiter/locale
options, append mode, tables with headers and mixed-type data remain future work.

The example below creates or replaces `matriz_exportada.csv` and
`matriz_dobrada.csv` in the current directory:

```sh
cargo run -p sun -- check examples/csv.sun
cargo run -p sun -- run examples/csv.sun
```

### Numeric base

`true` and `false` are reserved boolean literals, distinct from numeric values.
Comparisons, `!`, `&&`, `||` and `isempty` return booleans. Conditions accept
booleans and finite scalar numbers. In arithmetic, reductions, elementary
functions and numeric matrix literals/assignments, booleans convert to `0/1`.
Matrices still store `f64`: `[true, false]` is the numeric matrix `[1, 0]`, not a
logical array. Boolean masks, logical matrices and elementwise matrix
comparisons are not implemented. `switch` matches booleans by the same-type
rule rather than coercing them to numbers.

`start:stop` and `start:step:stop` produce row vectors in expressions, including
function arguments and matrix concatenation such as `[1:3, 7]`. Step defaults
to one; descending ranges need a negative step. Bounds and step must be finite
numeric scalars, and step cannot be zero. Empty ranges have shape `1x0`.
Arithmetic binds more tightly than colon, which binds more tightly than
comparisons. Loop-header ranges remain lazy; ordinary expressions materialize
their values. Numeric vectors can index matrices, and index expressions are
evaluated once. Direct range selectors require integer components and bare
`:` is valid only for indexing; logical masking is not supported.

`zeros`, `ones` and `eye` accept no arguments (a `1x1` matrix), one scalar size
(a square matrix), two dimensions, or a two-element size vector. For example:

```matlab
Z = zeros(2, 3);
U = ones(size(Z));
I = eye(2, 3);
samples = 0:0.5:2;
roots = sqrt([1, 4; 9, 16]);
average = mean(samples);
clipped = max(roots, 2);
```

Dimensions must be nonnegative finite integers; zero dimensions retain their
shape. Constructors, materialized ranges and checked expanding operations
limit dimensions and element counts to 16,777,216, reporting an error before
oversized allocations. This is not a total session-memory quota.

`sum`, `mean`, `min` and `max` reduce **all elements**, preserving Sun's existing
`sum` convention rather than MATLAB's default per-column reduction. Scalars
are unchanged. `sum([])` is `0`, `mean([])` is `NaN`, and `min([])` / `max([])`
are `0x0` matrices. NaNs propagate. The two-argument forms `min(A,B)` and
`max(A,B)` compare elementwise, supporting scalar expansion and compatible 2D
singleton broadcasting. Axis/dimension arguments and missing-value policies
are not implemented.

`sqrt`, `sin`, `cos`, `tan`, `abs`, `exp` and `log` operate on scalars or map
each matrix element, preserving shape, including `0xN`/`Nx0` matrices. They use
real `f64` behavior: invalid real domains produce `NaN`, and infinities are
retained; complex arithmetic is not implemented. These additions are available
in the interpreter and Studio; native boolean/range/construction/reduction and
matrix elementary-function support remains limited and unsupported constructs
are rejected rather than approximated.

```sh
cargo run -p sun -- check examples/base_numerica.sun
cargo run -p sun -- run examples/base_numerica.sun
```

### User-defined functions

Functions accept positional arguments and declare zero or one named output:

```matlab
answer = square(5);
notify("done");

function result = square(value)
	result = value * value;
end

function notify(text)
	disp(text);
end
```

Declarations must be at module level, with parentheses and a closing `end`.
They are registered before the script runs, so calls may precede declarations
and functions can call one another. Each call gets local variables containing
only its parameters; it cannot read or modify the caller's variables. Arguments,
including matrices, are passed by value. Built-ins remain available, and a user
function can shadow a built-in. A same-named matrix variable takes precedence
as indexing; other same-named variables are not callable.

The named output must be assigned before normal completion or `return`;
otherwise the call raises `Sun:UnassignedOutput`. A function without an output
can be called as a statement but not used as a value (`Sun:NoOutput`). Argument
counts must match exactly. Recursion and mutual recursion are supported up to
32 simultaneous calls; exceeding this raises catchable `Sun:RecursionLimit`.
Errors propagate to the caller's `try/catch`; cancellation bypasses handlers.
Output, pause state and cancellation use the current session host.

REPL and Studio definitions persist across commands. A later command may
replace a definition, while duplicate names within one module are rejected.
Check validates source without registering or replacing definitions. Clearing
the session removes functions as well as variables. Studio lists function
signatures in the Workspace, separately from local variables.

Nested functions, closures, anonymous functions, multiple outputs, optional
arguments and automatic lookup of function files are not implemented. Load
definitions by executing their source in the session. User-function calls
inside `parfor` are rejected until their effects can be analyzed; a function
may itself contain a supported `parfor`. Native compilation of functions is
not supported yet and reports interpreter guidance.

```sh
cargo run -p sun -- check examples/functions.sun
cargo run -p sun -- run examples/functions.sun
```

### Persistent sessions

`sun repl` retains variables and pause settings between commands without
re-executing earlier source. Enter multiline blocks through their closing `end`.
`:clear` clears variables/functions and resets pause settings, `:cancel` discards an
unfinished block, and `:quit` exits. Runtime errors preserve earlier successful
assignments; syntax errors do not execute the command.

Sun Studio shares one in-memory session between console commands and editor
scripts. Clear Session resets variables and functions, while Clear Console only removes the
transcript. Cooperative Stop preserves completed assignments and bypasses
`catch`; a numerical operation that cannot stop promptly may require killing
the engine, which explicitly clears the Workspace. Sessions are not saved to
disk or reconstructed by replay after shutdown or engine replacement.

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for design notes.

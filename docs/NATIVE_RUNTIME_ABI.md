# Sun native runtime matrix ABI
The C runtime exposes an opaque heap-backed `SunMatrix`. Consumers must not depend on its layout. Use `sun_matrix_create` / `sun_matrix_free` and the access/operation functions below.

- `sun_matrix_create(rows, cols)`, `sun_matrix_free(matrix)`
- `sun_matrix_rows(matrix)`, `sun_matrix_cols(matrix)`
- `sun_matrix_get(matrix, row, col, &status)`, `sun_matrix_get_indexed(matrix, row, col)`, `sun_matrix_set(matrix, row, col, value)`, `sun_matrix_set_indexed(matrix, row, col, value)`
- `sun_matrix_add(a, b)`, `sun_matrix_element_multiply(a, b)`, `sun_matrix_multiply(a, b)`
- `sun_matrix_scale(matrix, scalar)`, `sun_matrix_divide_scalar(matrix, scalar)`
- `sun_matrix_print(matrix)`

Indices at the ABI boundary are 1-based. The low-level integer accessor returns status codes for null and out-of-range access. `sun_matrix_get_indexed` accepts numeric indices, validates that both are finite positive integers within the matrix shape, and terminates the native program with a diagnostic on invalid input. Constructors and operations return an owned matrix or NULL for invalid shapes, division by zero, or allocation failure. The caller owns every returned matrix. The runtime also maintains an internal registry of live matrices: `sun_matrix_free` unregisters an explicitly released matrix, and `sun_runtime_cleanup` releases all remaining registered matrices. `sun_runtime_check_matrix` converts a NULL result into a diagnostic failure after cleaning up all live matrices.

The LLVM generator uses this ABI for matrix creation, arithmetic, display, and indexing. Every matrix-producing call is checked with `sun_runtime_check_matrix`; a NULL result triggers cleanup and a diagnostic rather than allowing a null pointer to be dereferenced. The generated entry point calls `sun_runtime_cleanup` once on normal return. Fatal runtime validation errors also clean up all registered matrices before exiting. Literal indices are checked at compile time; variable and computed indices are validated by the runtime.


Indexed assignment uses `A(row, column) = value`. The interpreter mutates the existing matrix value; the native backend calls `sun_matrix_set_indexed`, which validates finite positive integer coordinates and matrix bounds before writing. Indexed assignment requires an existing matrix variable and a numeric value.


## Native ownership model

The C runtime registry is the single owner of heap-allocated native matrices. Assigning a matrix variable to another variable creates an alias to the same pointer; it does not create another allocation or another cleanup entry. Reassigning a variable does not lose the previous allocation because it remains registered until explicit free or program cleanup. Matrix-producing operations return newly registered allocations. The generated program does not emit per-variable frees, avoiding double-free when variables alias the same matrix. The registry is process-global and intended for the current single-threaded generated-program execution model; it is not a thread-safe general-purpose multi-program allocator.

# Sun native runtime matrix ABI
The C runtime exposes an opaque heap-backed `SunMatrix`. Consumers must not depend on its layout. Use `sun_matrix_create` / `sun_matrix_free` and the access/operation functions below.

- `sun_matrix_create(rows, cols)`, `sun_matrix_free(matrix)`
- `sun_matrix_rows(matrix)`, `sun_matrix_cols(matrix)`
- `sun_matrix_get(matrix, row, col, &status)`, `sun_matrix_get_indexed(matrix, row, col)`, `sun_matrix_set(matrix, row, col, value)`, `sun_matrix_set_indexed(matrix, row, col, value)`
- `sun_matrix_add(a, b)`, `sun_matrix_element_multiply(a, b)`, `sun_matrix_multiply(a, b)`
- `sun_matrix_scale(matrix, scalar)`, `sun_matrix_divide_scalar(matrix, scalar)`
- `sun_matrix_print(matrix)`

Indices at the ABI boundary are 1-based. The low-level integer accessor returns status codes for null and out-of-range access. `sun_matrix_get_indexed` accepts numeric indices, validates that both are finite positive integers within the matrix shape, and terminates the native program with a diagnostic on invalid input. Constructors and operations return an owned matrix or NULL for invalid shapes, division by zero, or allocation failure. The caller owns every returned matrix.

The LLVM generator uses this ABI for matrix creation, arithmetic, display, and indexing. Literal indices are checked at compile time; variable and computed indices are validated by `sun_matrix_get_indexed` at runtime.


Indexed assignment uses `A(row, column) = value`. The interpreter mutates the existing matrix value; the native backend calls `sun_matrix_set_indexed`, which validates finite positive integer coordinates and matrix bounds before writing. Indexed assignment requires an existing matrix variable and a numeric value.

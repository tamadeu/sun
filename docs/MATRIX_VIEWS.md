# Zero-copy matrix views

The `sun-numeric` crate provides borrowed, read-only views over dense `SunMatrix` values.

## Rectangular views

`SunMatrix::view(row_start, row_end, col_start, col_end)` creates a view over an inclusive 1-based rectangle. The returned `MatrixView` borrows its source, so Rust prevents the source matrix from being dropped while the view remains in use.

```rust
let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
let column = matrix.view(1, 2, 2, 2).unwrap();
assert_eq!(column.to_rows(), vec![vec![2.0], vec![5.0]]);
```

## Transpose views

`MatrixView::transpose()` swaps the logical axes without allocating a new matrix or copying elements. View indexing remains 1-based.

## Current limits

- Views are read-only; mutation and copy-on-write semantics are not introduced.
- A view is tied to the source matrix lifetime and cannot outlive it.
- This is currently a Rust numeric-library API. Sun source syntax and LLVM/C runtime view handles are not part of this milestone.
- `to_rows()` deliberately materializes owned rows; ordinary view creation and transposition do not copy element data.
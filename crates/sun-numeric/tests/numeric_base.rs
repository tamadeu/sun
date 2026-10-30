use sun_numeric::{MatrixError, SunMatrix, MAX_MATRIX_ELEMENTS};

#[test]
fn constructors_preserve_shapes_and_check_allocation_sizes() {
    assert_eq!(SunMatrix::filled(0, 3, 1.0).unwrap().shape(), (0, 3));
    assert_eq!(SunMatrix::filled(3, 0, 1.0).unwrap().shape(), (3, 0));
    assert_eq!(SunMatrix::identity(2, 3).unwrap().to_rows(), [[1.0,0.0,0.0], [0.0,1.0,0.0]]);
    for (rows, cols) in [(usize::MAX, 2), (MAX_MATRIX_ELEMENTS, 2), (0, usize::MAX)] {
        assert_eq!(SunMatrix::filled(rows, cols, 0.0).unwrap_err(), MatrixError::SizeLimit);
    }
}

#[test]
fn reductions_have_explicit_empty_and_nan_behavior() {
    let matrix = SunMatrix::from_rows(vec![vec![1.0,8.0], vec![-3.0,2.0]]).unwrap();
    assert_eq!(matrix.sum(), 8.0);
    assert_eq!(matrix.mean(), 2.0);
    assert_eq!(matrix.minimum(), Some(-3.0));
    assert_eq!(matrix.maximum(), Some(8.0));
    let empty = SunMatrix::filled(0, 3, 0.0).unwrap();
    assert_eq!(empty.sum(), 0.0);
    assert!(empty.mean().is_nan());
    assert_eq!(empty.minimum(), None);
    assert_eq!(empty.maximum(), None);
    let nan = SunMatrix::from_rows(vec![vec![1.0,f64::NAN]]).unwrap();
    assert!(nan.minimum().unwrap().is_nan());
    assert!(nan.maximum().unwrap().is_nan());
}

#[test]
fn result_sizes_are_validated_before_expanding_zero_shapes_or_repeated_indices() {
    let left = SunMatrix::filled(MAX_MATRIX_ELEMENTS, 0, 0.0).unwrap();
    let right = SunMatrix::filled(0, MAX_MATRIX_ELEMENTS, 0.0).unwrap();
    assert_eq!(left.matmul(&right).unwrap_err(), MatrixError::SizeLimit);
    let scalar = SunMatrix::filled(1, 1, 1.0).unwrap();
    let indices = vec![1; 4097];
    assert_eq!(scalar.select(&indices, &indices).unwrap_err(), MatrixError::SizeLimit);
    let row = SunMatrix::filled(1, 4097, 1.0).unwrap();
    let column = SunMatrix::filled(4097, 1, 1.0).unwrap();
    assert_eq!(row.pairwise_minimum(&column).unwrap_err(), MatrixError::SizeLimit);
}
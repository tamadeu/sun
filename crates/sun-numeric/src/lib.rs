//! Dense f64 matrix primitives. Public Sun indexing is 1-based.

use ndarray::Array2;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MatrixError {
    #[error("a matrix cannot be empty")]
    Empty,
    #[error("all matrix rows must have the same number of columns")]
    RaggedRows,
    #[error("matrix index ({row}, {col}) is out of bounds for shape {rows}x{cols}")]
    IndexOutOfBounds { row: usize, col: usize, rows: usize, cols: usize },
    #[error("matrix dimensions are incompatible")]
    IncompatibleDimensions,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SunMatrix { data: Array2<f64> }

impl SunMatrix {
    pub fn from_rows(rows: Vec<Vec<f64>>) -> Result<Self, MatrixError> {
        let row_count = rows.len();
        let col_count = rows.first().map_or(0, Vec::len);
        if row_count == 0 || col_count == 0 { return Err(MatrixError::Empty); }
        if rows.iter().any(|row| row.len() != col_count) { return Err(MatrixError::RaggedRows); }
        let flat = rows.into_iter().flatten().collect();
        let data = Array2::from_shape_vec((row_count, col_count), flat)
            .expect("validated dimensions must match data length");
        Ok(Self { data })
    }

    pub fn shape(&self) -> (usize, usize) { (self.data.nrows(), self.data.ncols()) }

    /// Reads a value using Sun's 1-based indexing convention.
    pub fn get(&self, row: usize, col: usize) -> Result<f64, MatrixError> {
        let (rows, cols) = self.shape();
        if row == 0 || col == 0 || row > rows || col > cols {
            return Err(MatrixError::IndexOutOfBounds { row, col, rows, cols });
        }
        Ok(self.data[(row - 1, col - 1)])
    }

    pub fn add(&self, other: &Self) -> Result<Self, MatrixError> {
        if self.shape() != other.shape() { return Err(MatrixError::IncompatibleDimensions); }
        Ok(Self { data: &self.data + &other.data })
    }

    pub fn elementwise_mul(&self, other: &Self) -> Result<Self, MatrixError> {
        if self.shape() != other.shape() { return Err(MatrixError::IncompatibleDimensions); }
        Ok(Self { data: &self.data * &other.data })
    }

    pub fn matmul(&self, other: &Self) -> Result<Self, MatrixError> {
        if self.data.ncols() != other.data.nrows() { return Err(MatrixError::IncompatibleDimensions); }
        Ok(Self { data: self.data.dot(&other.data) })
    }

    pub fn to_rows(&self) -> Vec<Vec<f64>> {
        self.data.rows().into_iter().map(|row| row.to_vec()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn sample() -> SunMatrix {
        SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap()
    }

    #[test]
    fn indexes_from_one() {
        let matrix = sample();
        assert_eq!(matrix.get(1, 1).unwrap(), 1.0);
        assert_eq!(matrix.get(2, 2).unwrap(), 4.0);
        assert!(matrix.get(0, 1).is_err());
    }

    #[test]
    fn multiplies_matrices() {
        assert_eq!(sample().matmul(&sample()).unwrap().to_rows(), vec![vec![7.0, 10.0], vec![15.0, 22.0]]);
    }

    #[test]
    fn rejects_ragged_rows() {
        assert_eq!(SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0]]).unwrap_err(), MatrixError::RaggedRows);
    }

    #[test]
    fn elementwise_multiplication_is_distinct() {
        let result = sample().elementwise_mul(&sample()).unwrap();
        assert_abs_diff_eq!(result.get(2, 1).unwrap(), 9.0);
        assert_ne!(result.to_rows(), sample().matmul(&sample()).unwrap().to_rows());
    }
}

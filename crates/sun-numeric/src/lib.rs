//! Dense f64 matrix primitives. Public Sun indexing is 1-based.

use ndarray::{s, Array2, ArrayView2};
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

    /// Creates a read-only, zero-copy view over an inclusive 1-based rectangle.
    pub fn view(&self, row_start: usize, row_end: usize, col_start: usize, col_end: usize)
        -> Result<MatrixView<'_>, MatrixError>
    {
        let (rows, cols) = self.shape();
        if row_start == 0 || col_start == 0 || row_start > row_end
            || col_start > col_end || row_end > rows || col_end > cols
        {
            return Err(MatrixError::IndexOutOfBounds { row: row_end, col: col_end, rows, cols });
        }
        Ok(MatrixView { data: self.data.slice(s![row_start - 1..row_end, col_start - 1..col_end]) })
    }

    /// Reads a value using Sun's 1-based indexing convention.
    pub fn get(&self, row: usize, col: usize) -> Result<f64, MatrixError> {
        let (rows, cols) = self.shape();
        if row == 0 || col == 0 || row > rows || col > cols {
            return Err(MatrixError::IndexOutOfBounds { row, col, rows, cols });
        }
        Ok(self.data[(row - 1, col - 1)])
    }

    /// Mutates a value using Sun's 1-based indexing convention.
    pub fn set(&mut self, row: usize, col: usize, value: f64) -> Result<(), MatrixError> {
        let (rows, cols) = self.shape();
        if row == 0 || col == 0 || row > rows || col > cols {
            return Err(MatrixError::IndexOutOfBounds { row, col, rows, cols });
        }
        self.data[(row - 1, col - 1)] = value;
        Ok(())
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

/// A read-only borrowed view. Cloning and transposing copy only view metadata.
#[derive(Debug, Clone)]
pub struct MatrixView<'a> { data: ArrayView2<'a, f64> }

impl<'a> MatrixView<'a> {
    pub fn shape(&self) -> (usize, usize) { (self.data.nrows(), self.data.ncols()) }

    /// Reads using Sun's 1-based indexing convention.
    pub fn get(&self, row: usize, col: usize) -> Result<f64, MatrixError> {
        let (rows, cols) = self.shape();
        if row == 0 || col == 0 || row > rows || col > cols {
            return Err(MatrixError::IndexOutOfBounds { row, col, rows, cols });
        }
        Ok(self.data[(row - 1, col - 1)])
    }

    /// Returns a transposed view without allocating or copying elements.
    pub fn transpose(self) -> MatrixView<'a> { MatrixView { data: self.data.reversed_axes() } }

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
    fn creates_zero_copy_rectangular_views() {
        let matrix = sample();
        let view = matrix.view(1, 2, 2, 2).unwrap();
        assert_eq!(view.shape(), (2, 1));
        assert_eq!(view.to_rows(), vec![vec![2.0], vec![4.0]]);
        assert!(matrix.view(0, 1, 1, 1).is_err());
        assert!(matrix.view(2, 1, 1, 1).is_err());
    }

    #[test]
    fn transposes_views_without_changing_the_source() {
        let matrix = sample();
        let transposed = matrix.view(1, 2, 1, 2).unwrap().transpose();
        assert_eq!(transposed.shape(), (2, 2));
        assert_eq!(transposed.to_rows(), vec![vec![1.0, 3.0], vec![2.0, 4.0]]);
        assert_eq!(matrix.to_rows(), vec![vec![1.0, 2.0], vec![3.0, 4.0]]);
        assert_eq!(transposed.get(2, 1).unwrap(), 2.0);
    }

    #[test]
    fn indexes_from_one() {
        let matrix = sample();
        assert_eq!(matrix.get(1, 1).unwrap(), 1.0);
        assert_eq!(matrix.get(2, 2).unwrap(), 4.0);
        assert!(matrix.get(0, 1).is_err());
    }

    #[test]
    fn mutates_elements_using_one_based_indices() {
        let mut matrix = sample();
        matrix.set(2, 1, 99.0).unwrap();
        assert_eq!(matrix.get(2, 1).unwrap(), 99.0);
        assert!(matrix.set(0, 1, 0.0).is_err());
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

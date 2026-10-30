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
    #[error("matrix is singular and cannot be inverted or solved")]
    SingularMatrix,
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

    /// Adds matrices using 2D singleton-dimension broadcasting.
    pub fn broadcast_add(&self, other: &Self) -> Result<Self, MatrixError> {
        self.broadcast_binary(other, |a, b| a + b)
    }

    /// Multiplies elements using 2D singleton-dimension broadcasting.
    pub fn broadcast_elementwise_mul(&self, other: &Self) -> Result<Self, MatrixError> {
        self.broadcast_binary(other, |a, b| a * b)
    }

    fn broadcast_binary(&self, other: &Self, f: impl Fn(f64, f64) -> f64) -> Result<Self, MatrixError> {
        let (ar, ac) = self.shape();
        let (br, bc) = other.shape();
        if (ar != br && ar != 1 && br != 1) || (ac != bc && ac != 1 && bc != 1) {
            return Err(MatrixError::IncompatibleDimensions);
        }
        let rows = ar.max(br);
        let cols = ac.max(bc);
        let mut values = Vec::with_capacity(rows);
        for r in 0..rows {
            let mut row = Vec::with_capacity(cols);
            for c in 0..cols {
                row.push(f(self.data[(if ar == 1 { 0 } else { r }, if ac == 1 { 0 } else { c })],
                           other.data[(if br == 1 { 0 } else { r }, if bc == 1 { 0 } else { c })]));
            }
            values.push(row);
        }
        Self::from_rows(values)
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

    /// Returns a materialized transpose (distinct from the zero-copy MatrixView transpose).
    pub fn transpose(&self) -> Result<Self, MatrixError> {
        let (rows, cols) = self.shape();
        let mut out = vec![vec![0.0; rows]; cols];
        for r in 0..rows { for c in 0..cols { out[c][r] = self.data[(r, c)]; } }
        Self::from_rows(out)
    }

    /// Computes the determinant using Gaussian elimination with partial pivoting.
    pub fn determinant(&self) -> Result<f64, MatrixError> {
        let (rows, cols) = self.shape();
        if rows != cols { return Err(MatrixError::IncompatibleDimensions); }
        let mut a = self.to_rows();
        let mut det = 1.0;
        for col in 0..rows {
            let pivot = (col..rows).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs())).unwrap();
            if a[pivot][col].abs() <= f64::EPSILON { return Ok(0.0); }
            if pivot != col { a.swap(pivot, col); det = -det; }
            let p = a[col][col]; det *= p;
            for row in (col + 1)..rows {
                let factor = a[row][col] / p;
                for j in (col + 1)..rows { a[row][j] -= factor * a[col][j]; }
            }
        }
        Ok(det)
    }

    /// Solves A X = B using Gaussian elimination with partial pivoting.
    pub fn solve(&self, rhs: &Self) -> Result<Self, MatrixError> {
        let (n, cols) = self.shape();
        let (rhs_rows, rhs_cols) = rhs.shape();
        if n != cols || rhs_rows != n { return Err(MatrixError::IncompatibleDimensions); }
        let mut a = self.to_rows();
        let mut b = rhs.to_rows();
        for col in 0..n {
            let pivot = (col..n).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs())).unwrap();
            if a[pivot][col].abs() <= f64::EPSILON { return Err(MatrixError::SingularMatrix); }
            if pivot != col { a.swap(pivot, col); b.swap(pivot, col); }
            let p = a[col][col];
            for j in col..n { a[col][j] /= p; }
            for j in 0..rhs_cols { b[col][j] /= p; }
            for row in 0..n {
                if row == col { continue; }
                let factor = a[row][col];
                for j in col..n { a[row][j] -= factor * a[col][j]; }
                for j in 0..rhs_cols { b[row][j] -= factor * b[col][j]; }
            }
        }
        Self::from_rows(b)
    }

    pub fn inverse(&self) -> Result<Self, MatrixError> {
        let (n, cols) = self.shape();
        if n != cols { return Err(MatrixError::IncompatibleDimensions); }
        let identity = Self::from_rows((0..n).map(|r| (0..n).map(|c| if r == c { 1.0 } else { 0.0 }).collect()).collect())?;
        self.solve(&identity)
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
    fn computes_transpose_determinant_inverse_and_linear_solution() {
        let a = SunMatrix::from_rows(vec![vec![4.0, 7.0], vec![2.0, 6.0]]).unwrap();
        assert_eq!(a.transpose().unwrap().to_rows(), vec![vec![4.0, 2.0], vec![7.0, 6.0]]);
        assert_abs_diff_eq!(a.determinant().unwrap(), 10.0);
        let inv = a.inverse().unwrap();
        assert_abs_diff_eq!(inv.get(1, 1).unwrap(), 0.6);
        assert_abs_diff_eq!(inv.get(1, 2).unwrap(), -0.7);
        let rhs = SunMatrix::from_rows(vec![vec![1.0], vec![0.0]]).unwrap();
        let x = a.solve(&rhs).unwrap();
        assert_abs_diff_eq!(x.get(1, 1).unwrap(), 0.6);
        assert_abs_diff_eq!(x.get(2, 1).unwrap(), -0.2);
    }

    #[test]
    fn rejects_singular_and_non_square_linear_algebra_inputs() {
        let singular = SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![2.0, 4.0]]).unwrap();
        assert_eq!(singular.inverse().unwrap_err(), MatrixError::SingularMatrix);
        assert_eq!(SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0]]).unwrap().determinant().unwrap_err(), MatrixError::IncompatibleDimensions);
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

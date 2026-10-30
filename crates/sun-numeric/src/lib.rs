//! Dense f64 matrix primitives. Public Sun indexing is 1-based.

use ndarray::{s, Array2, ArrayView2, Axis};
use thiserror::Error;

pub const MAX_MATRIX_ELEMENTS: usize = 16_777_216;

pub fn minimum(left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() { f64::NAN } else { left.min(right) }
}

pub fn maximum(left: f64, right: f64) -> f64 {
    if left.is_nan() || right.is_nan() { f64::NAN } else { left.max(right) }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MatrixError {
    #[error("matrix dimensions or allocation exceed the 16777216-element limit")]
    SizeLimit,
    #[error("a matrix cannot be empty")]
    Empty,
    #[error("all matrix rows must have the same number of columns")]
    RaggedRows,
    #[error("matrix index ({row}, {col}) is out of bounds for shape {rows}x{cols}")]
    IndexOutOfBounds { row: usize, col: usize, rows: usize, cols: usize },
    #[error("matrix linear index {index} is out of bounds for {elements} elements")]
    LinearIndexOutOfBounds { index: usize, elements: usize },
    #[error("matrix dimensions are incompatible")]
    IncompatibleDimensions,
    #[error("matrix is singular and cannot be inverted or solved")]
    SingularMatrix,
}

fn checked_elements(rows: usize, cols: usize) -> Result<usize, MatrixError> {
    let count = rows.checked_mul(cols).ok_or(MatrixError::SizeLimit)?;
    if rows > MAX_MATRIX_ELEMENTS || cols > MAX_MATRIX_ELEMENTS || count > MAX_MATRIX_ELEMENTS {
        return Err(MatrixError::SizeLimit);
    }
    Ok(count)
}

#[derive(Debug, Clone, PartialEq)]
pub struct SunMatrix { data: Array2<f64> }

impl SunMatrix {
    pub fn filled(rows: usize, cols: usize, value: f64) -> Result<Self, MatrixError> {
        let count = checked_elements(rows, cols)?;
        let mut values = Vec::new();
        values.try_reserve_exact(count).map_err(|_| MatrixError::SizeLimit)?;
        values.resize(count, value);
        let data = Array2::from_shape_vec((rows, cols), values).map_err(|_| MatrixError::SizeLimit)?;
        Ok(Self { data })
    }

    pub fn identity(rows: usize, cols: usize) -> Result<Self, MatrixError> {
        let mut matrix = Self::filled(rows, cols, 0.0)?;
        for index in 0..rows.min(cols) { matrix.data[(index, index)] = 1.0; }
        Ok(matrix)
    }

    pub fn concatenate(blocks: &[Self], axis: usize) -> Result<Self, MatrixError> {
        if axis > 1 { return Err(MatrixError::IncompatibleDimensions); }
        let views: Vec<_> = blocks.iter().filter(|matrix| matrix.shape() != (0, 0)).map(|matrix| matrix.data.view()).collect();
        let Some(first) = views.first() else { return Self::filled(0, 0, 0.0); };
        let mut shape = [first.nrows(), first.ncols()];
        for view in &views[1..] {
            let next = [view.nrows(), view.ncols()];
            if shape[1 - axis] != next[1 - axis] { return Err(MatrixError::IncompatibleDimensions); }
            shape[axis] = shape[axis].checked_add(next[axis]).ok_or(MatrixError::SizeLimit)?;
        }
        checked_elements(shape[0], shape[1])?;
        let data = ndarray::concatenate(Axis(axis), &views).map_err(|_| MatrixError::IncompatibleDimensions)?;
        Ok(Self { data })
    }

    pub fn from_row_major(rows: usize, cols: usize, values: Vec<f64>) -> Result<Self, MatrixError> {
        if checked_elements(rows, cols)? != values.len() { return Err(MatrixError::IncompatibleDimensions); }
        let data = Array2::from_shape_vec((rows, cols), values).map_err(|_| MatrixError::IncompatibleDimensions)?;
        Ok(Self { data })
    }

    pub fn from_rows(rows: Vec<Vec<f64>>) -> Result<Self, MatrixError> {
        let row_count = rows.len();
        let col_count = rows.first().map_or(0, Vec::len);
        if rows.iter().any(|row| row.len() != col_count) { return Err(MatrixError::RaggedRows); }
        let flat = rows.into_iter().flatten().collect();
        let data = Array2::from_shape_vec((row_count, col_count), flat)
            .expect("validated dimensions must match data length");
        Ok(Self { data })
    }

    pub fn shape(&self) -> (usize, usize) { (self.data.nrows(), self.data.ncols()) }

    pub fn element_count(&self) -> usize { self.data.len() }

    pub fn is_empty(&self) -> bool { self.data.is_empty() }

    pub fn map(&self, transform: impl Fn(f64) -> f64) -> Self {
        Self { data: self.data.mapv(transform) }
    }

    pub fn sum(&self) -> f64 { if self.is_empty() { 0.0 } else { self.data.iter().copied().sum() } }

    pub fn mean(&self) -> f64 { self.sum() / self.element_count() as f64 }

    pub fn minimum(&self) -> Option<f64> { self.data.iter().copied().reduce(minimum) }

    pub fn maximum(&self) -> Option<f64> { self.data.iter().copied().reduce(maximum) }

    pub fn pairwise_minimum(&self, other: &Self) -> Result<Self, MatrixError> { self.broadcast_binary(other, minimum) }

    pub fn pairwise_maximum(&self, other: &Self) -> Result<Self, MatrixError> { self.broadcast_binary(other, maximum) }

    fn validate_selection(&self, row_indices: &[usize], column_indices: &[usize]) -> Result<(), MatrixError> {
        let (rows, cols) = self.shape();
        for &row in row_indices {
            if row == 0 || row > rows {
                return Err(MatrixError::IndexOutOfBounds { row, col: column_indices.first().copied().unwrap_or(1), rows, cols });
            }
        }
        for &col in column_indices {
            if col == 0 || col > cols {
                return Err(MatrixError::IndexOutOfBounds { row: row_indices.first().copied().unwrap_or(1), col, rows, cols });
            }
        }
        Ok(())
    }

    pub fn select(&self, row_indices: &[usize], column_indices: &[usize]) -> Result<Self, MatrixError> {
        self.validate_selection(row_indices, column_indices)?;
        checked_elements(row_indices.len(), column_indices.len())?;
        let data = Array2::from_shape_fn((row_indices.len(), column_indices.len()), |(row, col)| {
            self.data[(row_indices[row] - 1, column_indices[col] - 1)]
        });
        Ok(Self { data })
    }

    pub fn fill_selection(&mut self, row_indices: &[usize], column_indices: &[usize], value: f64) -> Result<(), MatrixError> {
        self.validate_selection(row_indices, column_indices)?;
        for &col in column_indices {
            for &row in row_indices { self.data[(row - 1, col - 1)] = value; }
        }
        Ok(())
    }

    pub fn assign_selection(&mut self, row_indices: &[usize], column_indices: &[usize], values: &Self) -> Result<(), MatrixError> {
        self.validate_selection(row_indices, column_indices)?;
        let scalar = values.shape() == (1, 1);
        if row_indices.is_empty() || column_indices.is_empty() {
            return if scalar || values.is_empty() { Ok(()) } else { Err(MatrixError::IncompatibleDimensions) };
        }
        if !scalar && values.shape() != (row_indices.len(), column_indices.len()) {
            return Err(MatrixError::IncompatibleDimensions);
        }
        for (col_offset, &col) in column_indices.iter().enumerate() {
            for (row_offset, &row) in row_indices.iter().enumerate() {
                let value = if scalar { values.data[(0, 0)] } else { values.data[(row_offset, col_offset)] };
                self.data[(row - 1, col - 1)] = value;
            }
        }
        Ok(())
    }

    fn validate_linear_selection(&self, indices: &[usize]) -> Result<(), MatrixError> {
        for &index in indices { self.linear_coordinates(index)?; }
        Ok(())
    }

    pub fn select_linear(&self, indices: &[usize], force_column: bool) -> Result<Self, MatrixError> {
        self.validate_linear_selection(indices)?;
        let rows = self.data.nrows();
        let column = force_column || self.data.ncols() == 1;
        let shape = if column { (indices.len(), 1) } else { (1, indices.len()) };
        let data = Array2::from_shape_fn(shape, |(row, col)| {
            let position = if column { row } else { col };
            let offset = indices[position] - 1;
            self.data[(offset % rows, offset / rows)]
        });
        Ok(Self { data })
    }

    pub fn fill_linear_selection(&mut self, indices: &[usize], value: f64) -> Result<(), MatrixError> {
        self.validate_linear_selection(indices)?;
        let rows = self.data.nrows();
        for &index in indices {
            let offset = index - 1;
            self.data[(offset % rows, offset / rows)] = value;
        }
        Ok(())
    }

    pub fn assign_linear_selection(&mut self, indices: &[usize], values: &Self) -> Result<(), MatrixError> {
        self.validate_linear_selection(indices)?;
        let scalar = values.element_count() == 1;
        if !scalar && values.element_count() != indices.len() { return Err(MatrixError::IncompatibleDimensions); }
        let rows = self.data.nrows();
        let source_rows = values.data.nrows();
        for (position, &index) in indices.iter().enumerate() {
            let source_offset = if scalar { 0 } else { position };
            let offset = index - 1;
            self.data[(offset % rows, offset / rows)] = values.data[(source_offset % source_rows, source_offset / source_rows)];
        }
        Ok(())
    }

    fn linear_coordinates(&self, index: usize) -> Result<(usize, usize), MatrixError> {
        let elements = self.element_count();
        if index == 0 || index > elements {
            return Err(MatrixError::LinearIndexOutOfBounds { index, elements });
        }
        let rows = self.data.nrows();
        Ok(((index - 1) % rows + 1, (index - 1) / rows + 1))
    }

    pub fn get_linear(&self, index: usize) -> Result<f64, MatrixError> {
        let (row, col) = self.linear_coordinates(index)?;
        self.get(row, col)
    }

    pub fn set_linear(&mut self, index: usize, value: f64) -> Result<(), MatrixError> {
        let (row, col) = self.linear_coordinates(index)?;
        self.set(row, col, value)
    }

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

    pub fn broadcast_subtract(&self, other: &Self) -> Result<Self, MatrixError> {
        self.broadcast_binary(other, |left, right| left - right)
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
        let rows = if ar == 1 { br } else { ar };
        let cols = if ac == 1 { bc } else { ac };
        checked_elements(rows, cols)?;
        let data = Array2::from_shape_fn((rows, cols), |(row, col)| {
            f(self.data[(if ar == 1 { 0 } else { row }, if ac == 1 { 0 } else { col })],
              other.data[(if br == 1 { 0 } else { row }, if bc == 1 { 0 } else { col })])
        });
        Ok(Self { data })
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
        checked_elements(self.data.nrows(), other.data.ncols())?;
        Ok(Self { data: self.data.dot(&other.data) })
    }

    /// Returns a materialized transpose (distinct from the zero-copy MatrixView transpose).
    pub fn transpose(&self) -> Result<Self, MatrixError> {
        Ok(Self { data: self.data.t().to_owned() })
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
        if n == 0 || rhs_cols == 0 { return Ok(rhs.clone()); }
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

    #[test]
    fn constructs_empty_matrices_and_preserves_empty_selection_shapes() {
        let empty = SunMatrix::from_rows(Vec::new()).unwrap();
        assert_eq!(empty.shape(), (0, 0));
        assert_eq!(empty.element_count(), 0);
        let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        assert_eq!(matrix.select(&[], &[1, 2]).unwrap().shape(), (0, 2));
        assert_eq!(matrix.select(&[1, 2], &[]).unwrap().shape(), (2, 0));
        assert_eq!(SunMatrix::from_rows(vec![vec![], vec![]]).unwrap().shape(), (2, 0));
        assert_eq!(SunMatrix::from_rows(vec![vec![], vec![1.0]]).unwrap_err(), MatrixError::RaggedRows);
    }

    #[test]
    fn empty_matrices_preserve_shape_through_transpose_and_map() {
        let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        let zero_rows = matrix.select(&[], &[1, 2]).unwrap();
        let zero_cols = matrix.select(&[1, 2], &[]).unwrap();
        assert!(zero_rows.is_empty());
        assert_eq!(zero_rows.transpose().unwrap().shape(), (2, 0));
        assert_eq!(zero_cols.transpose().unwrap().shape(), (0, 2));
        assert_eq!(zero_rows.map(|_| panic!("empty map must not visit elements")).shape(), (0, 2));
        assert_eq!(zero_cols.map(|value| value * 2.0).shape(), (2, 0));
        assert_eq!(zero_rows.select_linear(&[], true).unwrap().shape(), (0, 1));
        assert_eq!(zero_rows.select_linear(&[], false).unwrap().shape(), (1, 0));
    }

    #[test]
    fn empty_broadcasting_preserves_zero_dimensions() {
        let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        let zero_rows = matrix.select(&[], &[1, 2]).unwrap();
        let zero_cols = matrix.select(&[1, 2], &[]).unwrap();
        let row = matrix.select(&[1], &[1, 2]).unwrap();
        let column = matrix.select(&[1, 2], &[1]).unwrap();
        let scalar = SunMatrix::from_rows(vec![vec![2.0]]).unwrap();
        assert_eq!(zero_rows.broadcast_add(&row).unwrap().shape(), (0, 2));
        assert_eq!(row.broadcast_subtract(&zero_rows).unwrap().shape(), (0, 2));
        assert_eq!(zero_rows.broadcast_elementwise_mul(&scalar).unwrap().shape(), (0, 2));
        assert_eq!(scalar.broadcast_add(&zero_rows).unwrap().shape(), (0, 2));
        assert_eq!(zero_cols.broadcast_add(&column).unwrap().shape(), (2, 0));
        assert_eq!(column.broadcast_elementwise_mul(&zero_cols).unwrap().shape(), (2, 0));
        assert_eq!(zero_rows.broadcast_add(&matrix).unwrap_err(), MatrixError::IncompatibleDimensions);
    }

    #[test]
    fn empty_multiplication_and_linear_algebra_preserve_shapes() {
        let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        let zero_rows = matrix.select(&[], &[1, 2]).unwrap();
        let zero_cols = matrix.select(&[1, 2], &[]).unwrap();
        assert_eq!(zero_cols.matmul(&zero_rows).unwrap().to_rows(), vec![vec![0.0, 0.0], vec![0.0, 0.0]]);
        assert_eq!(zero_rows.matmul(&matrix).unwrap().shape(), (0, 2));
        assert_eq!(matrix.matmul(&zero_cols).unwrap().shape(), (2, 0));
        let empty = SunMatrix::from_rows(Vec::new()).unwrap();
        assert_eq!(empty.determinant().unwrap(), 1.0);
        assert_eq!(empty.inverse().unwrap().shape(), (0, 0));
        assert_eq!(empty.solve(&zero_rows).unwrap().shape(), (0, 2));
        assert_eq!(matrix.solve(&zero_cols).unwrap().shape(), (2, 0));
        assert_eq!(zero_rows.determinant().unwrap_err(), MatrixError::IncompatibleDimensions);
    }

    #[test]
    fn empty_selections_accept_scalar_or_empty_writes_without_mutation() {
        let original = SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        let mut matrix = original.clone();
        let empty = SunMatrix::from_rows(Vec::new()).unwrap();
        matrix.fill_selection(&[], &[1, 2], 99.0).unwrap();
        matrix.assign_selection(&[], &[1, 2], &empty).unwrap();
        matrix.fill_linear_selection(&[], 99.0).unwrap();
        matrix.assign_linear_selection(&[], &empty).unwrap();
        assert_eq!(matrix, original);
        assert_eq!(matrix.assign_selection(&[], &[1], &original).unwrap_err(), MatrixError::IncompatibleDimensions);
        assert_eq!(matrix.assign_linear_selection(&[], &original).unwrap_err(), MatrixError::IncompatibleDimensions);
        assert_eq!(matrix, original);
    }

    #[test]
    fn empty_axes_do_not_bypass_validation_of_other_indices() {
        let empty = SunMatrix::from_rows(Vec::new()).unwrap();
        assert!(empty.get(1, 1).is_err());
        assert!(empty.get_linear(1).is_err());
        assert!(empty.select(&[1], &[]).is_err());
        assert!(empty.select(&[], &[1]).is_err());
        let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        assert!(matrix.select(&[], &[3]).is_err());
        assert!(matrix.select(&[3], &[]).is_err());
    }

    #[test]
    fn linear_selections_preserve_column_order_and_result_orientation() {
        let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        assert_eq!(matrix.select_linear(&[6, 2, 3], false).unwrap().to_rows(), vec![vec![6.0, 4.0, 2.0]]);
        let mut column = matrix.select_linear(&[6, 2, 3], true).unwrap();
        assert_eq!(column.to_rows(), vec![vec![6.0], vec![4.0], vec![2.0]]);
        column.set(1, 1, 99.0).unwrap();
        assert_eq!(matrix.get_linear(6).unwrap(), 6.0);
        let row = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0]]).unwrap();
        assert_eq!(row.select_linear(&[3, 1], false).unwrap().shape(), (1, 2));
        assert_eq!(row.select_linear(&[3, 1], true).unwrap().shape(), (2, 1));
        let column = SunMatrix::from_rows(vec![vec![1.0], vec![2.0], vec![3.0]]).unwrap();
        assert_eq!(column.select_linear(&[3, 1], false).unwrap().to_rows(), vec![vec![3.0], vec![1.0]]);
    }

    #[test]
    fn linear_selection_assignment_uses_rhs_column_order_and_preserves_target_shape() {
        let mut matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        let values = SunMatrix::from_rows(vec![vec![60.0, 20.0], vec![30.0, 10.0]]).unwrap();
        matrix.assign_linear_selection(&[6, 2, 3, 1], &values).unwrap();
        assert_eq!(matrix.to_rows(), vec![vec![10.0, 20.0, 3.0], vec![30.0, 5.0, 60.0]]);
        matrix.fill_linear_selection(&[1, 5], 9.0).unwrap();
        let scalar = SunMatrix::from_rows(vec![vec![0.0]]).unwrap();
        matrix.assign_linear_selection(&[2, 3], &scalar).unwrap();
        assert_eq!(matrix.shape(), (2, 3));
        assert_eq!(matrix.to_rows(), vec![vec![9.0, 0.0, 9.0], vec![0.0, 5.0, 60.0]]);
    }

    #[test]
    fn linear_selection_errors_do_not_partially_mutate_target() {
        let original = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        let scalar = SunMatrix::from_rows(vec![vec![99.0]]).unwrap();
        for indices in [vec![1, 2, 7], vec![1, 0], vec![1, usize::MAX]] {
            let mut matrix = original.clone();
            assert!(matrix.select_linear(&indices, false).is_err());
            assert!(matrix.fill_linear_selection(&indices, 99.0).is_err());
            assert_eq!(matrix, original);
            assert!(matrix.assign_linear_selection(&indices, &scalar).is_err());
            assert_eq!(matrix, original);
        }
        let mut matrix = original.clone();
        let wrong_count = SunMatrix::from_rows(vec![vec![7.0, 8.0]]).unwrap();
        assert_eq!(matrix.assign_linear_selection(&[1, 2, 3], &wrong_count).unwrap_err(), MatrixError::IncompatibleDimensions);
        assert_eq!(matrix, original);
    }

    #[test]
    fn fills_selected_coordinates_without_changing_shape() {
        let mut matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        matrix.fill_selection(&[2, 1], &[3, 1], 0.0).unwrap();
        assert_eq!(matrix.shape(), (2, 3));
        assert_eq!(matrix.to_rows(), vec![vec![0.0, 2.0, 0.0], vec![0.0, 5.0, 0.0]]);
    }

    #[test]
    fn assigns_in_requested_order_and_expands_singleton_values() {
        let mut matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        let values = SunMatrix::from_rows(vec![vec![60.0, 40.0], vec![30.0, 10.0]]).unwrap();
        matrix.assign_selection(&[2, 1], &[3, 1], &values).unwrap();
        assert_eq!(matrix.to_rows(), vec![vec![10.0, 2.0, 30.0], vec![40.0, 5.0, 60.0]]);
        let scalar = SunMatrix::from_rows(vec![vec![9.0]]).unwrap();
        matrix.assign_selection(&[1, 2], &[2], &scalar).unwrap();
        assert_eq!(matrix.to_rows(), vec![vec![10.0, 9.0, 30.0], vec![40.0, 9.0, 60.0]]);
    }

    #[test]
    fn selection_assignment_errors_do_not_partially_mutate_target() {
        let original = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        let scalar = SunMatrix::from_rows(vec![vec![99.0]]).unwrap();
        for (rows, cols) in [(vec![1, 3], vec![1]), (vec![1], vec![1, 4])] {
            let mut matrix = original.clone();
            assert!(matrix.fill_selection(&rows, &cols, 99.0).is_err());
            assert_eq!(matrix, original);
            assert!(matrix.assign_selection(&rows, &cols, &scalar).is_err());
            assert_eq!(matrix, original);
        }
        let mut matrix = original.clone();
        let wrong_shape = SunMatrix::from_rows(vec![vec![7.0, 8.0]]).unwrap();
        assert_eq!(matrix.assign_selection(&[1, 2], &[3], &wrong_shape).unwrap_err(), MatrixError::IncompatibleDimensions);
        assert_eq!(matrix, original);
    }

    #[test]
    fn selects_rows_and_columns_in_requested_order_without_aliasing() {
        let matrix = SunMatrix::from_rows(vec![vec![11.0, 12.0, 13.0, 14.0], vec![21.0, 22.0, 23.0, 24.0], vec![31.0, 32.0, 33.0, 34.0]]).unwrap();
        let mut selected = matrix.select(&[3, 1], &[4, 2]).unwrap();
        assert_eq!(selected.shape(), (2, 2));
        assert_eq!(selected.to_rows(), vec![vec![34.0, 32.0], vec![14.0, 12.0]]);
        selected.set(1, 1, 99.0).unwrap();
        assert_eq!(matrix.get(3, 4).unwrap(), 34.0);
        assert_eq!(matrix.select(&[2], &[3]).unwrap().to_rows(), vec![vec![23.0]]);
        assert_eq!(matrix.select(&[1, 2, 3], &[4]).unwrap().shape(), (3, 1));
        assert_eq!(matrix.select(&[2], &[1, 2, 3, 4]).unwrap().shape(), (1, 4));
    }

    #[test]
    fn selection_rejects_out_of_bounds_axes() {
        let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        assert_eq!(matrix.select(&[], &[1]).unwrap().shape(), (0, 1));
        assert_eq!(matrix.select(&[1], &[]).unwrap().shape(), (1, 0));
        for (rows, cols) in [(vec![0], vec![1]), (vec![3], vec![1]), (vec![1], vec![0]), (vec![1], vec![3])] {
            assert!(matches!(matrix.select(&rows, &cols), Err(MatrixError::IndexOutOfBounds { .. })));
        }
    }

    #[test]
    fn linear_indexing_reads_in_column_major_order() {
        let matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        assert_eq!(matrix.element_count(), 6);
        let values: Vec<_> = (1..=matrix.element_count()).map(|index| matrix.get_linear(index).unwrap()).collect();
        assert_eq!(values, vec![1.0, 4.0, 2.0, 5.0, 3.0, 6.0]);
    }

    #[test]
    fn linear_indexing_writes_without_changing_shape() {
        let mut matrix = SunMatrix::from_rows(vec![vec![1.0, 2.0, 3.0], vec![4.0, 5.0, 6.0]]).unwrap();
        matrix.set_linear(2, 40.0).unwrap();
        matrix.set_linear(5, 30.0).unwrap();
        assert_eq!(matrix.shape(), (2, 3));
        assert_eq!(matrix.to_rows(), vec![vec![1.0, 2.0, 30.0], vec![40.0, 5.0, 6.0]]);
        for index in [0, 7, usize::MAX] {
            let expected = MatrixError::LinearIndexOutOfBounds { index, elements: 6 };
            assert_eq!(matrix.get_linear(index).unwrap_err(), expected);
            assert_eq!(matrix.set_linear(index, 99.0).unwrap_err(), expected);
        }
        assert_eq!(matrix.to_rows(), vec![vec![1.0, 2.0, 30.0], vec![40.0, 5.0, 6.0]]);
    }

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

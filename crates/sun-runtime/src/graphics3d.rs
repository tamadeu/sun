use serde::Serialize;
use sun_numeric::SunMatrix;
use super::{RuntimeError, Value};

const MAX_POINTS: usize = 10_000;
const MAX_CURVES: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Curve3D {
    pub vertices: Vec<[f64; 3]>,
    pub segments: Vec<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scene3D {
    Curves { curves: Vec<Curve3D>, bounds: [[f64; 2]; 3] },
    Surface { vertices: Vec<[f64; 3]>, triangles: Vec<[u32; 3]>, bounds: [[f64; 2]; 3], rows: usize, cols: usize },
}

impl Scene3D {
    pub fn series_count(&self) -> usize { match self { Self::Curves { curves, .. } => curves.len(), Self::Surface { .. } => 1 } }
}

fn invalid(message: impl Into<String>) -> RuntimeError { RuntimeError::InvalidArgument(message.into()) }

fn vector(value: &Value) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Number(number) => Ok(vec![*number]),
        Value::Matrix(matrix) if !matrix.is_empty() && (matrix.shape().0 == 1 || matrix.shape().1 == 1) && matrix.element_count() <= MAX_POINTS =>
            Ok((1..=matrix.element_count()).map(|index| matrix.get_linear(index)).collect::<Result<_, _>>()?),
        _ => Err(invalid("3D coordinates must be nonempty numeric vectors or scalars with at most 10000 points")),
    }
}

fn bounds(vertices: impl Iterator<Item = [f64; 3]>) -> [[f64; 2]; 3] {
    let mut result = [[f64::INFINITY, f64::NEG_INFINITY]; 3];
    for vertex in vertices {
        for axis in 0..3 { result[axis][0] = result[axis][0].min(vertex[axis]); result[axis][1] = result[axis][1].max(vertex[axis]); }
    }
    result
}

fn curve(points: impl Iterator<Item = [f64; 3]>) -> Result<Curve3D, RuntimeError> {
    let mut vertices = Vec::new();
    let mut segments = Vec::new();
    let mut previous = None;
    for point in points {
        if point.iter().all(|value| value.is_finite()) {
            let index = vertices.len() as u32;
            vertices.push(point);
            if let Some(previous) = previous { segments.push([previous, index]); }
            previous = Some(index);
        } else { previous = None; }
    }
    if vertices.is_empty() { return Err(invalid("each plot3 curve requires a finite coordinate triple")); }
    Ok(Curve3D { vertices, segments })
}

fn plot3(args: &[Value]) -> Result<Scene3D, RuntimeError> {
    if args.len() != 3 { return Err(RuntimeError::Arity("plot3".into(), "three numeric coordinates: plot3(x,y,z)".into())); }
    let matrix_shape = args.iter().find_map(|value| match value { Value::Matrix(matrix) if matrix.shape().0 > 1 && matrix.shape().1 > 1 => Some(matrix.shape()), _ => None });
    let curves = if let Some((rows, cols)) = matrix_shape {
        if cols > MAX_CURVES || rows.checked_mul(cols).is_none_or(|count| count > MAX_POINTS) {
            return Err(invalid("plot3 supports at most 16 curves and 10000 total points"));
        }
        let matrices: Vec<_> = args.iter().map(|value| match value {
            Value::Matrix(matrix) if matrix.shape() == (rows, cols) => Ok(matrix),
            _ => Err(invalid("plot3 matrix coordinates must all have the same shape")),
        }).collect::<Result<_, _>>()?;
        let mut curves = Vec::with_capacity(cols);
        for column in 1..=cols {
            curves.push(curve((1..=rows).map(|row| [matrices[0].get(row,column).unwrap(), matrices[1].get(row,column).unwrap(), matrices[2].get(row,column).unwrap()]))?);
        }
        curves
    } else {
        let x = vector(&args[0])?;
        let y = vector(&args[1])?;
        let z = vector(&args[2])?;
        if x.len() != y.len() || x.len() != z.len() { return Err(invalid("plot3 vectors must have equal lengths")); }
        vec![curve(x.into_iter().zip(y).zip(z).map(|((x,y),z)| [x,y,z]))?]
    };
    let bounds = bounds(curves.iter().flat_map(|curve| curve.vertices.iter().copied()));
    Ok(Scene3D::Curves { curves, bounds })
}

fn has_area(triangle: [u32; 3], vertices: &[[f64; 3]]) -> bool {
    let raw = triangle.map(|index| vertices[index as usize]);
    let ranges: [[f64;2];3] = std::array::from_fn(|axis| [raw.iter().map(|point| point[axis]).fold(f64::INFINITY,f64::min),raw.iter().map(|point| point[axis]).fold(f64::NEG_INFINITY,f64::max)]);
    let points = raw.map(|point| std::array::from_fn::<_,3,_>(|axis| {
        let [minimum,maximum] = ranges[axis];
        if minimum == maximum { return 0.0; }
        let scale = minimum.abs().max(maximum.abs()).max(f64::MIN_POSITIVE);
        (point[axis]/scale-minimum/scale)/(maximum/scale-minimum/scale)
    }));
    let first: [f64;3] = std::array::from_fn(|axis| points[1][axis]-points[0][axis]);
    let second: [f64;3] = std::array::from_fn(|axis| points[2][axis]-points[0][axis]);
    let cross = [first[1]*second[2]-first[2]*second[1], first[2]*second[0]-first[0]*second[2], first[0]*second[1]-first[1]*second[0]];
    cross.iter().any(|value| value.is_finite() && *value != 0.0)
}

fn surf(args: &[Value]) -> Result<Scene3D, RuntimeError> {
    if args.len() != 1 && args.len() != 3 { return Err(RuntimeError::Arity("surf".into(), "surf(Z) or surf(X,Y,Z)".into())); }
    let Value::Matrix(z) = &args[args.len()-1] else { return Err(invalid("surf Z must be a numeric matrix")); };
    let (rows, cols) = z.shape();
    if rows < 2 || cols < 2 || z.element_count() > MAX_POINTS { return Err(invalid("surf requires at least 2x2 and at most 10000 grid points")); }
    let mut x_values = Vec::new();
    let mut y_values = Vec::new();
    let mut grids: Option<(&SunMatrix, &SunMatrix)> = None;
    if args.len() == 3 {
        match (&args[0], &args[1]) {
            (Value::Matrix(x), Value::Matrix(y)) if x.shape() == z.shape() && y.shape() == z.shape() => grids = Some((x,y)),
            (x,y) => {
                x_values = vector(x)?; y_values = vector(y)?;
                if x_values.len() != cols || y_values.len() != rows { return Err(invalid("surf x/y vectors must match Z columns/rows, or X/Y/Z must have equal matrix shapes")); }
            }
        }
    }
    let mut vertices = Vec::with_capacity(z.element_count());
    let mut indices = Vec::with_capacity(z.element_count());
    for row in 1..=rows {
        for col in 1..=cols {
            let point = if let Some((x,y)) = grids { [x.get(row,col)?, y.get(row,col)?, z.get(row,col)?] }
                else if args.len() == 1 { [col as f64, row as f64, z.get(row,col)?] }
                else { [x_values[col-1], y_values[row-1], z.get(row,col)?] };
            if point.iter().all(|value| value.is_finite()) { indices.push(Some(vertices.len() as u32)); vertices.push(point); }
            else { indices.push(None); }
        }
    }
    let mut triangles = Vec::with_capacity((rows-1)*(cols-1)*2);
    for row in 0..rows-1 {
        for col in 0..cols-1 {
            let first = row*cols+col;
            for triangle in [[first,first+1,first+cols+1], [first,first+cols+1,first+cols]] {
                if let [Some(first),Some(second),Some(third)] = triangle.map(|index| indices[index]) {
                    let triangle = [first,second,third];
                    if has_area(triangle, &vertices) { triangles.push(triangle); }
                }
            }
        }
    }
    if triangles.is_empty() { return Err(invalid("surf requires at least one finite, nondegenerate triangle")); }
    let mut used = vec![false;vertices.len()];
    for triangle in &triangles { for index in triangle { used[*index as usize] = true; } }
    let mut remap = vec![0;vertices.len()];
    let mut compact = Vec::new();
    for (index, vertex) in vertices.into_iter().enumerate() {
        if used[index] { remap[index] = compact.len() as u32; compact.push(vertex); }
    }
    for triangle in &mut triangles { *triangle = triangle.map(|index| remap[index as usize]); }
    let vertices = compact;
    let bounds = bounds(vertices.iter().copied());
    Ok(Scene3D::Surface { vertices, triangles, bounds, rows, cols })
}

pub(super) fn prepare(name: &str, args: &[Value]) -> Result<Scene3D, RuntimeError> {
    match name { "plot3" => plot3(args), "surf" => surf(args), _ => unreachable!() }
}
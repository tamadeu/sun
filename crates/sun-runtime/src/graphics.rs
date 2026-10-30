use serde::Serialize;
use super::{ExecutionContext, RuntimeError, Value};

const MAX_POINTS: usize = 10_000;
const MAX_TRACES: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FigureTrace {
    pub x: Vec<Option<f64>>,
    pub y: Vec<Option<f64>>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FigureLayout {
    pub title: String,
    pub x_label: String,
    pub y_label: String,
    pub legend: Vec<String>,
    pub show_legend: bool,
    pub grid: bool,
    pub x_range: Option<[f64; 2]>,
    pub y_range: Option<[f64; 2]>,
    pub axis_revision: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Figure {
    pub id: u32,
    pub traces: Vec<FigureTrace>,
    pub layout: FigureLayout,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FigureEvent {
    Figure { figure: Figure },
    FigureUpdate { #[serde(rename = "figureId")] figure_id: u32, layout: FigureLayout },
    FiguresCleared,
}

#[derive(Default)]
pub(super) struct GraphicsState {
    pub current: Option<Figure>,
    next_id: u32,
}

pub(super) fn handles(name: &str) -> bool {
    matches!(name, "plot" | "title" | "xlabel" | "ylabel" | "legend" | "grid" | "axis" | "clf")
}

fn invalid(message: impl Into<String>) -> RuntimeError { RuntimeError::InvalidArgument(message.into()) }

fn vector(value: &Value) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Number(number) => Ok(vec![*number]),
        Value::Matrix(matrix) if !matrix.is_empty() && (matrix.shape().0 == 1 || matrix.shape().1 == 1) => {
            if matrix.element_count() > MAX_POINTS { return Err(invalid("plot supports at most 10000 total points")); }
            Ok((1..=matrix.element_count()).map(|index| matrix.get_linear(index)).collect::<Result<_, _>>()?)
        }
        _ => Err(invalid("plot coordinates must be nonempty numeric vectors or scalars")),
    }
}

fn finite(number: f64) -> Option<f64> { number.is_finite().then_some(number) }

fn traces(x: &Value, y: &Value) -> Result<Vec<FigureTrace>, RuntimeError> {
    let x = vector(x)?;
    let columns = match y {
        Value::Matrix(matrix) if matrix.shape().0 > 1 && matrix.shape().1 > 1 => {
            let (rows, cols) = matrix.shape();
            if rows != x.len() || cols > MAX_TRACES || matrix.element_count() > MAX_POINTS {
                return Err(invalid("plot y columns must match x length, with at most 16 curves and 10000 total points"));
            }
            (1..=cols).map(|column| (1..=rows).map(|row| matrix.get(row, column)).collect::<Result<Vec<_>, _>>()).collect::<Result<Vec<_>, _>>()?
        }
        _ => vec![vector(y)?],
    };
    let mut traces = Vec::with_capacity(columns.len());
    for column in columns {
        if column.len() != x.len() { return Err(invalid("plot x and y vectors must have equal lengths")); }
        if !x.iter().zip(&column).any(|(x, y)| x.is_finite() && y.is_finite()) {
            return Err(invalid("each plot curve needs at least one finite coordinate pair"));
        }
        traces.push(FigureTrace { x: x.iter().copied().map(finite).collect(), y: column.into_iter().map(finite).collect() });
    }
    Ok(traces)
}

fn text(value: &Value) -> Result<String, RuntimeError> {
    match value {
        Value::String(text) if text.len() <= 512 => Ok(text.clone()),
        _ => Err(invalid("figure labels must be strings of at most 512 UTF-8 bytes")),
    }
}

pub(super) fn call(name: &str, args: &[Value], context: &mut ExecutionContext<'_>) -> Result<Value, RuntimeError> {
    context.check_cancelled()?;
    if name == "clf" {
        if !args.is_empty() { return Err(RuntimeError::Arity(name.into(), "no arguments".into())); }
        context.host.figure_event(&FigureEvent::FiguresCleared)?;
        context.graphics.current = None;
        return Ok(Value::Void);
    }
    if name == "plot" {
        if args.len() != 2 { return Err(RuntimeError::Arity(name.into(), "two numeric arguments: plot(x, y)".into())); }
        let traces = traces(&args[0], &args[1])?;
        let layout = FigureLayout { legend: (1..=traces.len()).map(|index| format!("Serie {index}")).collect(),
            show_legend: traces.len() > 1, ..FigureLayout::default() };
        let id = context.graphics.next_id.checked_add(1).ok_or_else(|| invalid("figure ID limit reached; clear the session"))?;
        let figure = Figure { id, traces, layout };
        context.check_cancelled()?;
        context.host.figure_event(&FigureEvent::Figure { figure: figure.clone() })?;
        context.graphics.current = Some(figure);
        context.graphics.next_id = id;
        return Ok(Value::Void);
    }
    let figure = context.graphics.current.as_ref().ok_or_else(|| invalid("plot must create a figure before changing its labels or axes"))?;
    let mut layout = figure.layout.clone();
    match name {
        "title" | "xlabel" | "ylabel" => {
            if args.len() != 1 { return Err(RuntimeError::Arity(name.into(), "one string".into())); }
            let label = text(&args[0])?;
            match name { "title" => layout.title = label, "xlabel" => layout.x_label = label, _ => layout.y_label = label }
        }
        "grid" => layout.grid = match args {
            [] => !layout.grid,
            [Value::Bool(enabled)] => *enabled,
            [Value::String(state)] if state == "on" || state == "off" => state == "on",
            _ => return Err(invalid("grid expects no arguments, a boolean, or 'on'/'off'")),
        },
        "legend" => match args {
            [Value::String(state)] if state == "on" || state == "off" => layout.show_legend = state == "on",
            labels if labels.len() == figure.traces.len() => {
                layout.legend = labels.iter().map(text).collect::<Result<_, _>>()?;
                layout.show_legend = true;
            }
            _ => return Err(RuntimeError::Arity(name.into(), "one string per curve or 'on'/'off'".into())),
        },
        "axis" => { match args {
            [Value::String(mode)] if mode == "auto" => { layout.x_range = None; layout.y_range = None; }
            [limits] => {
                let limits = vector(limits)?;
                if limits.len() != 4 || !limits.iter().all(|value| value.is_finite()) || limits[0] >= limits[1] || limits[2] >= limits[3] {
                    return Err(invalid("axis expects [xmin, xmax, ymin, ymax] with finite increasing bounds"));
                }
                layout.x_range = Some([limits[0], limits[1]]);
                layout.y_range = Some([limits[2], limits[3]]);
            }
            _ => return Err(invalid("axis expects four limits or 'auto'")),
        }
            layout.axis_revision = layout.axis_revision.checked_add(1).ok_or_else(|| invalid("axis revision limit reached; create a new plot"))?;
        },
        _ => unreachable!(),
    }
    let id = figure.id;
    context.check_cancelled()?;
    context.host.figure_event(&FigureEvent::FigureUpdate { figure_id: id, layout: layout.clone() })?;
    context.graphics.current.as_mut().unwrap().layout = layout;
    Ok(Value::Void)
}
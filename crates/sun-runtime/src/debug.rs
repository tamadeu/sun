use serde::{Deserialize, Serialize};
use std::{collections::{BTreeSet, HashMap}, sync::Arc};
use sun_ast::{Module, Stmt, StmtKind};
use sun_source::Span;
use super::Value;

#[derive(Debug)]
pub struct DebugSource {
    pub id: String, pub name: String, pub text: String, pub executable_lines: Vec<usize>,
    line_starts: Vec<usize>,
}

impl DebugSource {
    pub fn new(id: impl Into<String>, name: impl Into<String>, text: impl Into<String>, module: &Module) -> Self {
        let text = text.into();
        let mut line_starts = vec![0];
        line_starts.extend(text.bytes().enumerate().filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)));
        let mut source = Self { id: id.into(), name: name.into(), text, executable_lines: Vec::new(), line_starts };
        let mut lines = BTreeSet::new();
        source.collect_lines(&module.statements, &mut lines);
        source.executable_lines = lines.into_iter().collect();
        source
    }

    pub fn location(&self, span: Span) -> DebugLocation {
        let offset = span.start.min(self.text.len());
        let line = self.line_starts.partition_point(|start| *start <= offset).saturating_sub(1);
        let column = self.text.get(self.line_starts[line]..offset).map_or(0, |text| text.encode_utf16().count());
        DebugLocation { source_id: self.id.clone(), line: line + 1, column: column + 1 }
    }

    fn collect_lines(&self, statements: &[Stmt], lines: &mut BTreeSet<usize>) {
        for statement in statements {
            if !matches!(statement.kind, StmtKind::Function(_)) { lines.insert(self.location(statement.span).line); }
            match &statement.kind {
                StmtKind::Function(function) => self.collect_lines(&function.body, lines),
                StmtKind::For { body, .. } | StmtKind::ForEach { body, .. } | StmtKind::While { body, .. } => self.collect_lines(body, lines),
                StmtKind::If { branches, else_body } => {
                    for (condition, body) in branches { lines.insert(self.location(condition.span).line); self.collect_lines(body, lines); }
                    self.collect_lines(else_body, lines);
                }
                StmtKind::Switch { cases, otherwise_body, .. } => {
                    for (value, body) in cases { lines.insert(self.location(value.span).line); self.collect_lines(body, lines); }
                    self.collect_lines(otherwise_body, lines);
                }
                StmtKind::Try { body, catch_body, .. } => { self.collect_lines(body, lines); self.collect_lines(catch_body, lines); }
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum DebugNumber { Number(f64), Special(String) }

fn number(value: f64) -> DebugNumber {
    if value.is_nan() { DebugNumber::Special("NaN".into()) }
    else if value == f64::INFINITY { DebugNumber::Special("Inf".into()) }
    else if value == f64::NEG_INFINITY { DebugNumber::Special("-Inf".into()) }
    else { DebugNumber::Number(value) }
}

#[derive(Debug, Clone, Serialize)]
pub struct DebugVariable {
    pub name: String, pub kind: &'static str, pub rows: Option<usize>, pub cols: Option<usize>,
    pub value: String, pub preview: Vec<Vec<DebugNumber>>, pub truncated: bool,
}

fn variables(env: &HashMap<String, Value>) -> Vec<DebugVariable> {
    let mut entries: Vec<_> = env.iter().collect();
    entries.sort_by(|left, right| left.0.cmp(right.0));
    entries.into_iter().take(100).enumerate().map(|(index, (name, value))| {
        let name_truncated = name.len() > 200;
        let display_name = if name_truncated { format!("{}...#{index}", name.chars().take(180).collect::<String>()) } else { name.clone() };
        let mut summary = DebugVariable { name: display_name, kind: "void", rows: None, cols: None,
            value: String::new(), preview: Vec::new(), truncated: false };
        match value {
            Value::Matrix(matrix) => {
                let (rows, cols) = matrix.shape();
                summary.kind = "matrix"; summary.rows = Some(rows); summary.cols = Some(cols);
                summary.value = if matrix.is_empty() { "[]".into() } else { format!("{} elementos", matrix.element_count()) };
                summary.preview = (1..=rows.min(5)).map(|row| (1..=cols.min(5)).map(|col| number(matrix.get(row, col).unwrap())).collect()).collect();
                summary.truncated = !matrix.is_empty() && (rows > 5 || cols > 5);
            }
            Value::Number(value) => { summary.kind = "number"; summary.rows = Some(1); summary.cols = Some(1); summary.value = Value::Number(*value).to_string(); summary.preview = vec![vec![number(*value)]]; }
            Value::Complex(value) => { summary.kind = "complex"; summary.rows = Some(1); summary.cols = Some(1); summary.value = Value::Complex(*value).to_string(); }
            Value::Bool(value) => { summary.kind = "boolean"; summary.rows = Some(1); summary.cols = Some(1); summary.value = value.to_string(); }
            other => {
                summary.kind = match other { Value::String(_) => "string", Value::Exception(_) => "exception", _ => "void" };
                let text = other.to_string(); summary.truncated = text.chars().count() > 200;
                summary.value = text.chars().take(200).collect();
            }
        }
        summary.truncated |= name_truncated;
        summary
    }).collect()
}

#[derive(Debug, Clone)]
pub struct DebugFrame {
    pub name: String, pub source: Arc<DebugSource>, pub location: DebugLocation,
    pub variables: Vec<DebugVariable>, pub variables_truncated: bool,
}

pub struct DebugSnapshot { pub reason: DebugStopReason, pub frames: Vec<DebugFrame>, pub depth: usize }

pub(super) struct DebugState {
    pub enabled: bool, pub source: Option<Arc<DebugSource>>, pub name: String,
    pub span: Span, pub callers: Vec<DebugFrame>,
}

impl Default for DebugState {
    fn default() -> Self { Self { enabled: false, source: None, name: "<script>".into(), span: Span::new(0, 0), callers: Vec::new() } }
}

impl DebugState {
    pub fn frame(&self, env: &HashMap<String, Value>) -> Option<DebugFrame> {
        let source = self.source.as_ref()?.clone();
        Some(DebugFrame { name: self.name.chars().take(200).collect(), location: source.location(self.span), source,
            variables: variables(env), variables_truncated: env.len() > 100 })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugAction { Continue, StepInto, StepOver, StepOut }

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DebugBreakpoint { pub source_id: String, pub line: usize }

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugLocation { pub source_id: String, pub line: usize, pub column: usize }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugStopReason { Entry, Breakpoint, Step, Pause }

pub struct DebugController {
    breakpoints: Vec<DebugBreakpoint>,
    action: DebugAction,
    depth: usize,
    entry: bool,
    pause: bool,
}

impl DebugController {
    pub fn new(breakpoints: Vec<DebugBreakpoint>, stop_on_entry: bool) -> Self {
        Self { breakpoints, action: DebugAction::Continue, depth: 0, entry: stop_on_entry, pause: false }
    }

    pub fn set_breakpoints(&mut self, breakpoints: Vec<DebugBreakpoint>) { self.breakpoints = breakpoints; }

    pub fn request_pause(&mut self) { self.pause = true; }

    pub fn resume(&mut self, action: DebugAction, depth: usize) { self.action = action; self.depth = depth; }

    pub fn checkpoint(&mut self, location: &DebugLocation, depth: usize) -> Option<DebugStopReason> {
        if self.entry { self.entry = false; return Some(DebugStopReason::Entry); }
        if self.pause { self.pause = false; return Some(DebugStopReason::Pause); }
        if self.breakpoints.iter().any(|point| point.source_id == location.source_id && point.line == location.line) {
            return Some(DebugStopReason::Breakpoint);
        }
        match self.action {
            DebugAction::StepInto => Some(DebugStopReason::Step),
            DebugAction::StepOver if depth <= self.depth => Some(DebugStopReason::Step),
            DebugAction::StepOut if depth < self.depth => Some(DebugStopReason::Step),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(source: &str, line: usize) -> DebugLocation { DebugLocation { source_id: source.into(), line, column: 1 } }

    #[test]
    fn stepping_respects_call_depth_and_breakpoints_override_step_over() {
        let mut controller = DebugController::new(vec![DebugBreakpoint { source_id: "function".into(), line: 3 }], true);
        assert_eq!(controller.checkpoint(&location("script", 1), 0), Some(DebugStopReason::Entry));
        controller.resume(DebugAction::StepOver, 0);
        assert_eq!(controller.checkpoint(&location("function", 2), 1), None);
        assert_eq!(controller.checkpoint(&location("function", 3), 1), Some(DebugStopReason::Breakpoint));
        controller.resume(DebugAction::StepOut, 1);
        assert_eq!(controller.checkpoint(&location("function", 4), 1), None);
        assert_eq!(controller.checkpoint(&location("script", 2), 0), Some(DebugStopReason::Step));
        controller.resume(DebugAction::StepInto, 0);
        assert_eq!(controller.checkpoint(&location("function", 1), 1), Some(DebugStopReason::Step));
    }

    #[test]
    fn pause_and_updated_breakpoints_are_source_scoped() {
        let mut controller = DebugController::new(Vec::new(), false);
        controller.request_pause();
        assert_eq!(controller.checkpoint(&location("first", 2), 0), Some(DebugStopReason::Pause));
        controller.resume(DebugAction::Continue, 0);
        controller.set_breakpoints(vec![DebugBreakpoint { source_id: "first".into(), line: 2 }]);
        assert_eq!(controller.checkpoint(&location("second", 2), 0), None);
        assert_eq!(controller.checkpoint(&location("first", 2), 0), Some(DebugStopReason::Breakpoint));
    }

    #[test]
    fn variable_snapshots_bound_names_counts_and_matrix_previews() {
        let mut env = HashMap::new();
        env.insert("a".repeat(250), Value::Matrix(sun_numeric::SunMatrix::filled(20, 20, 1.0).unwrap()));
        let summary = variables(&env);
        assert!(summary[0].name.len() <= 200);
        assert_eq!(summary[0].rows, Some(20));
        assert_eq!(summary[0].preview.len(), 5);
        assert_eq!(summary[0].preview[0].len(), 5);
        assert!(summary[0].truncated);
        for index in 0..150 { env.insert(format!("variable{index}"), Value::Number(1.0)); }
        assert_eq!(variables(&env).len(), 100);
    }

    #[test]
    fn complex_snapshots_are_textual_scalars() {
        let env = HashMap::from([("z".into(), Value::Complex(num_complex::Complex64::new(2.0, -3.0)))]);
        let summary = variables(&env);
        assert_eq!(summary[0].kind, "complex");
        assert_eq!(summary[0].value, "2 - 3i");
        assert_eq!(summary[0].rows, Some(1));
        assert_eq!(summary[0].cols, Some(1));
        assert!(summary[0].preview.is_empty());
    }
}
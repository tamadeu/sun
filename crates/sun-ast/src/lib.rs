//! Abstract syntax tree for Sun.

use sun_source::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Module { pub statements: Vec<Stmt> }

#[derive(Debug, Clone, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
    /// Whether a trailing semicolon suppresses the statement's implicit result.
    pub suppress_output: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDefinition {
    pub name: String,
    pub parameters: Vec<String>,
    pub output: Option<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StmtKind {
    Function(FunctionDefinition),
    Assign { name: String, value: Expr },
    IndexAssign { name: String, row: Expr, col: Expr, value: Expr },
    LinearIndexAssign { name: String, index: Expr, value: Expr },
    While { condition: Expr, body: Vec<Stmt> },
    For { name: String, start: Expr, step: Option<Expr>, stop: Expr, body: Vec<Stmt> },
    ParFor { name: String, start: Expr, step: Option<Expr>, stop: Expr, body: Vec<Stmt> },
    ForEach { name: String, source: Expr, body: Vec<Stmt> },
    If { branches: Vec<(Expr, Vec<Stmt>)>, else_body: Vec<Stmt> },
    Switch { selector: Expr, cases: Vec<(Expr, Vec<Stmt>)>, otherwise_body: Vec<Stmt> },
    Try { body: Vec<Stmt>, catch_name: Option<String>, catch_body: Vec<Stmt> },
    Break,
    Continue,
    Return,
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr { pub kind: ExprKind, pub span: Span }

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number(f64),
    Complex { real: f64, imaginary: f64 },
    Bool(bool),
    String(String),
    Identifier(String),
    IndexEnd,
    IndexAll,
    IndexRange { start: Box<Expr>, step: Option<Box<Expr>>, stop: Box<Expr> },
    FieldAccess { object: Box<Expr>, field: String },
    Matrix(Vec<Vec<Expr>>),
    Unary { op: UnaryOp, operand: Box<Expr> },
    Binary { left: Box<Expr>, op: BinaryOp, right: Box<Expr> },
    Call { callee: Box<Expr>, arguments: Vec<Expr> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp { Positive, Negative, Not }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add, Subtract, Multiply, ElementMultiply, Divide, ElementDivide, Power,
    Equal, NotEqual, Less, LessEqual, Greater, GreaterEqual, And, Or,
}

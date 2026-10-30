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
pub enum StmtKind {
    Assign { name: String, value: Expr },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr { pub kind: ExprKind, pub span: Span }

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Number(f64),
    String(String),
    Identifier(String),
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

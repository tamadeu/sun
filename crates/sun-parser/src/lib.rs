//! Initial parser: assignments, expression statements, calls and matrix literals.

use sun_ast::*;
use sun_lexer::{lex, LexError, Token, TokenKind};
use sun_source::Span;
use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum ParseError {
    #[error(transparent)]
    Lex(#[from] LexError),
    #[error("unexpected token at byte {offset}: expected {expected}")]
    UnexpectedToken { offset: usize, expected: &'static str },
}

pub fn parse(source: &str) -> Result<Module, ParseError> {
    Parser { tokens: lex(source)?, current: 0 }.module()
}

struct Parser { tokens: Vec<Token>, current: usize }

impl Parser {
    fn module(mut self) -> Result<Module, ParseError> {
        let mut statements = Vec::new();
        self.skip_separators();
        while !self.is_at_end() {
            statements.push(self.statement()?);
            self.skip_separators();
        }
        Ok(Module { statements })
    }

    fn statement(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span.start;
        if matches!(&self.peek().kind, TokenKind::Ident(_)) && matches!(&self.peek_n(1).kind, TokenKind::LeftParen) {
            let saved = self.current;
            let name = match self.advance().kind.clone() { TokenKind::Ident(name) => name, _ => unreachable!() };
            self.advance();
            let candidate = (|| -> Result<Option<Stmt>, ParseError> {
                let row = self.expression(0)?;
                if !matches!(self.peek().kind, TokenKind::Comma) { return Ok(None); }
                self.advance();
                let col = self.expression(0)?;
                self.expect(|kind| matches!(kind, TokenKind::RightParen), ")")?;
                if !matches!(self.peek().kind, TokenKind::Equal) { return Ok(None); }
                self.advance();
                let value = self.expression(0)?;
                let end = value.span.end;
                let suppress_output = matches!(self.peek().kind, TokenKind::Semicolon);
                if suppress_output { self.advance(); }
                Ok(Some(Stmt { kind: StmtKind::IndexAssign { name, row, col, value }, span: Span::new(start, end), suppress_output }))
            })();
            match candidate {
                Ok(Some(stmt)) => return Ok(stmt),
                Ok(None) => self.current = saved,
                Err(error) => { self.current = saved; if !matches!(error, ParseError::UnexpectedToken { .. }) { return Err(error); } }
            }
        }
        if let (TokenKind::Ident(name), TokenKind::Equal) = (&self.peek().kind, &self.peek_n(1).kind) {
            let name = name.clone();
            self.advance();
            self.advance();
            let value = self.expression(0)?;
            let end = value.span.end;
            let suppress_output = matches!(self.peek().kind, TokenKind::Semicolon);
            if suppress_output { self.advance(); }
            return Ok(Stmt {
                kind: StmtKind::Assign { name, value },
                span: Span::new(start, end),
                suppress_output,
            });
        }
        let expr = self.expression(0)?;
        let suppress_output = matches!(self.peek().kind, TokenKind::Semicolon);
        if suppress_output { self.advance(); }
        Ok(Stmt { span: expr.span, kind: StmtKind::Expr(expr), suppress_output })
    }

    fn expression(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let token = self.advance().clone();
        let mut lhs = match token.kind {
            TokenKind::Number(value) => Expr { kind: ExprKind::Number(value), span: token.span },
            TokenKind::String(value) => Expr { kind: ExprKind::String(value), span: token.span },
            TokenKind::Ident(name) => Expr { kind: ExprKind::Identifier(name), span: token.span },
            TokenKind::Plus | TokenKind::Minus | TokenKind::Bang => {
                let op = match token.kind {
                    TokenKind::Plus => UnaryOp::Positive,
                    TokenKind::Minus => UnaryOp::Negative,
                    _ => UnaryOp::Not,
                };
                let operand = self.expression(13)?;
                Expr { span: Span::new(token.span.start, operand.span.end), kind: ExprKind::Unary { op, operand: Box::new(operand) } }
            }
            TokenKind::LeftParen => {
                let expr = self.expression(0)?;
                self.expect(|kind| matches!(kind, TokenKind::RightParen), ")")?;
                expr
            }
            TokenKind::LeftBracket => self.matrix(token.span.start)?,
            _ => return Err(ParseError::UnexpectedToken { offset: token.span.start, expected: "expression" }),
        };

        loop {
            if matches!(&self.peek().kind, TokenKind::LeftParen) {
                if 15 < min_bp { break; }
                self.advance();
                let mut arguments = Vec::new();
                if !matches!(self.peek().kind, TokenKind::RightParen) {
                    loop {
                        arguments.push(self.expression(0)?);
                        if !matches!(self.peek().kind, TokenKind::Comma) { break; }
                        self.advance();
                    }
                }
                let close = self.expect(|kind| matches!(kind, TokenKind::RightParen), ")")?;
                lhs = Expr { span: Span::new(lhs.span.start, close.span.end), kind: ExprKind::Call { callee: Box::new(lhs), arguments } };
                continue;
            }
            let Some((left_bp, right_bp, op)) = infix_binding_power(&self.peek().kind) else { break; };
            if left_bp < min_bp { break; }
            self.advance();
            let rhs = self.expression(right_bp)?;
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr { span, kind: ExprKind::Binary { left: Box::new(lhs), op, right: Box::new(rhs) } };
        }
        Ok(lhs)
    }

    fn matrix(&mut self, start: usize) -> Result<Expr, ParseError> {
        let mut rows: Vec<Vec<Expr>> = Vec::new();
        if matches!(self.peek().kind, TokenKind::RightBracket) {
            let close = self.advance().clone();
            return Ok(Expr { kind: ExprKind::Matrix(rows), span: Span::new(start, close.span.end) });
        }
        rows.push(vec![self.expression(0)?]);
        loop {
            match self.peek().kind {
                TokenKind::Comma => { self.advance(); rows.last_mut().unwrap().push(self.expression(0)?); }
                TokenKind::Semicolon | TokenKind::Newline => {
                    self.advance();
                    if matches!(self.peek().kind, TokenKind::RightBracket) { break; }
                    rows.push(vec![self.expression(0)?]);
                }
                TokenKind::RightBracket => break,
                _ => return Err(ParseError::UnexpectedToken { offset: self.peek().span.start, expected: "comma, semicolon or closing bracket in matrix" }),
            }
        }
        let close = self.expect(|kind| matches!(kind, TokenKind::RightBracket), "]")?;
        Ok(Expr { kind: ExprKind::Matrix(rows), span: Span::new(start, close.span.end) })
    }

    fn skip_separators(&mut self) {
        while matches!(self.peek().kind, TokenKind::Newline | TokenKind::Semicolon) { self.advance(); }
    }
    fn expect(&mut self, predicate: impl FnOnce(&TokenKind) -> bool, expected: &'static str) -> Result<Token, ParseError> {
        if predicate(&self.peek().kind) { Ok(self.advance().clone()) }
        else { Err(ParseError::UnexpectedToken { offset: self.peek().span.start, expected }) }
    }
    fn peek(&self) -> &Token { &self.tokens[self.current] }
    fn peek_n(&self, n: usize) -> &Token { self.tokens.get(self.current + n).unwrap_or_else(|| self.tokens.last().unwrap()) }
    fn is_at_end(&self) -> bool { matches!(self.peek().kind, TokenKind::Eof) }
    fn advance(&mut self) -> &Token {
        let index = self.current;
        if !self.is_at_end() { self.current += 1; }
        &self.tokens[index]
    }
}

fn infix_binding_power(kind: &TokenKind) -> Option<(u8, u8, BinaryOp)> {
    Some(match kind {
        TokenKind::OrOr => (1, 2, BinaryOp::Or),
        TokenKind::AndAnd => (3, 4, BinaryOp::And),
        TokenKind::EqualEqual => (5, 6, BinaryOp::Equal),
        TokenKind::NotEqual => (5, 6, BinaryOp::NotEqual),
        TokenKind::Less => (5, 6, BinaryOp::Less),
        TokenKind::LessEqual => (5, 6, BinaryOp::LessEqual),
        TokenKind::Greater => (5, 6, BinaryOp::Greater),
        TokenKind::GreaterEqual => (5, 6, BinaryOp::GreaterEqual),
        TokenKind::Plus => (7, 8, BinaryOp::Add),
        TokenKind::Minus => (7, 8, BinaryOp::Subtract),
        TokenKind::Star => (9, 10, BinaryOp::Multiply),
        TokenKind::DotStar => (9, 10, BinaryOp::ElementMultiply),
        TokenKind::Slash => (9, 10, BinaryOp::Divide),
        TokenKind::DotSlash => (9, 10, BinaryOp::ElementDivide),
        TokenKind::Caret => (12, 11, BinaryOp::Power),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_precedence_and_assignment() {
        let module = parse("x = 2 + 3 * 4;").unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::Assign { name, value } = &module.statements[0].kind else { panic!("expected assignment") };
        assert_eq!(name, "x");
        assert!(module.statements[0].suppress_output);
        let ExprKind::Binary { op: BinaryOp::Add, right, .. } = &value.kind else { panic!("expected addition at root") };
        assert!(matches!(&right.kind, ExprKind::Binary { op: BinaryOp::Multiply, .. }));
    }

    #[test]
    fn parses_indexed_matrix_assignment() {
        let module = parse("A(2, 3) = 10;").unwrap();
        let StmtKind::IndexAssign { name, row, col, value } = &module.statements[0].kind else { panic!("expected indexed assignment") };
        assert_eq!(name, "A");
        assert!(matches!(row.kind, ExprKind::Number(2.0)));
        assert!(matches!(col.kind, ExprKind::Number(3.0)));
        assert!(matches!(value.kind, ExprKind::Number(10.0)));
    }

    #[test]
    fn keeps_function_calls_as_expression_statements() {
        let module = parse("disp(A(1, 2));").unwrap();
        assert!(matches!(module.statements[0].kind, StmtKind::Expr(_)));
    }

    #[test]
    fn parses_matrix_literal() {
        let module = parse("A = [1, 2; 3, 4];").unwrap();
        let StmtKind::Assign { value, .. } = &module.statements[0].kind else { panic!("expected assignment") };
        assert!(matches!(&value.kind, ExprKind::Matrix(rows) if rows.len() == 2 && rows[0].len() == 2));
    }
}

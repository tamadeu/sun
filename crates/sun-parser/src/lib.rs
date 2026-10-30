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
    #[error("'{keyword}' at byte {offset} is only valid inside a loop")]
    LoopControlOutsideLoop { offset: usize, keyword: &'static str },
}

pub fn parse(source: &str) -> Result<Module, ParseError> {
    Parser { tokens: lex(source)?, current: 0, loop_depth: 0, argument_depth: 0, block_depth: 0 }.module()
}

struct Parser { tokens: Vec<Token>, current: usize, loop_depth: usize, argument_depth: usize, block_depth: usize }

impl Parser {
    fn module(mut self) -> Result<Module, ParseError> {
        let statements = self.block(&[])?;
        let mut names = std::collections::HashSet::new();
        for statement in &statements {
            if let StmtKind::Function(function) = &statement.kind {
                if !names.insert(&function.name) {
                    return Err(ParseError::UnexpectedToken { offset: statement.span.start, expected: "unique function name within module" });
                }
            }
        }
        Ok(Module { statements })
    }

    fn block(&mut self, terminators: &[TokenKind]) -> Result<Vec<Stmt>, ParseError> {
        self.block_depth += 1;
        let result = self.block_contents(terminators);
        self.block_depth -= 1;
        result
    }

    fn block_contents(&mut self, terminators: &[TokenKind]) -> Result<Vec<Stmt>, ParseError> {
        let mut statements = Vec::new();
        self.skip_separators();
        while !self.is_at_end() && !terminators.contains(&self.peek().kind) {
            statements.push(self.statement()?);
            self.skip_separators();
        }
        Ok(statements)
    }

    fn identifier(&mut self, expected: &'static str) -> Result<String, ParseError> {
        let token = self.expect(|kind| matches!(kind, TokenKind::Ident(_)), expected)?;
        let TokenKind::Ident(name) = token.kind else { unreachable!() };
        Ok(name)
    }

    fn condition_header(&mut self, expected: &'static str) -> Result<Expr, ParseError> {
        let condition = self.expression(0)?;
        self.expect(|kind| matches!(kind, TokenKind::Newline | TokenKind::Semicolon), expected)?;
        Ok(condition)
    }

    fn close_block(&mut self, start: usize, kind: StmtKind, expected: &'static str) -> Result<Stmt, ParseError> {
        let close = self.expect(|kind| matches!(kind, TokenKind::End), expected)?;
        let suppress_output = matches!(self.peek().kind, TokenKind::Semicolon);
        if suppress_output { self.advance(); }
        Ok(Stmt { kind, span: Span::new(start, close.span.end), suppress_output })
    }

    fn statement(&mut self) -> Result<Stmt, ParseError> {
        let start = self.peek().span.start;
        if matches!(self.peek().kind, TokenKind::Function) {
            if self.block_depth != 1 {
                return Err(ParseError::UnexpectedToken { offset: start, expected: "function declaration at module level" });
            }
            self.advance();
            let first = self.identifier("function name or output variable")?;
            let (name, output) = if matches!(self.peek().kind, TokenKind::Equal) {
                self.advance();
                (self.identifier("function name after =")?, Some(first))
            } else { (first, None) };
            self.expect(|kind| matches!(kind, TokenKind::LeftParen), "( after function name")?;
            let mut parameters = Vec::new();
            if !matches!(self.peek().kind, TokenKind::RightParen) {
                loop {
                    let offset = self.peek().span.start;
                    let parameter = self.identifier("function parameter")?;
                    if parameters.contains(&parameter) {
                        return Err(ParseError::UnexpectedToken { offset, expected: "unique function parameter" });
                    }
                    parameters.push(parameter);
                    if !matches!(self.peek().kind, TokenKind::Comma) { break; }
                    self.advance();
                }
            }
            self.expect(|kind| matches!(kind, TokenKind::RightParen), ") after function parameters")?;
            self.expect(|kind| matches!(kind, TokenKind::Newline | TokenKind::Semicolon), "newline or semicolon after function header")?;
            let body = self.block(&[TokenKind::End])?;
            return self.close_block(start, StmtKind::Function(FunctionDefinition { name, parameters, output, body }), "end to close function");
        }
        if matches!(self.peek().kind, TokenKind::For | TokenKind::ParFor) {
            let parallel = matches!(self.peek().kind, TokenKind::ParFor);
            self.advance();
            let token = self.expect(|kind| matches!(kind, TokenKind::Ident(_)), "loop variable after for")?;
            let TokenKind::Ident(name) = token.kind else { unreachable!() };
            self.expect(|kind| matches!(kind, TokenKind::Equal), "= after for variable")?;
            let source = self.expression(0)?;
            let range = match &source.kind {
                ExprKind::IndexRange { start, step, stop } => Some((*start.clone(), step.as_deref().cloned(), *stop.clone())),
                _ => None,
            };
            if parallel && range.is_none() {
                return Err(ParseError::UnexpectedToken { offset: self.peek().span.start, expected: "start:stop or start:step:stop range in parfor" });
            }
            self.expect(|kind| matches!(kind, TokenKind::Newline | TokenKind::Semicolon), "newline or semicolon after for source")?;
            self.loop_depth += 1;
            let body = self.block(&[TokenKind::End]);
            self.loop_depth -= 1;
            let body = body?;
            let kind = match range {
                Some((start, step, stop)) if parallel => StmtKind::ParFor { name, start, step, stop, body },
                Some((start, step, stop)) => StmtKind::For { name, start, step, stop, body },
                None => StmtKind::ForEach { name, source, body },
            };
            return self.close_block(start, kind, if parallel { "end to close parfor block" } else { "end to close for block" });
        }
        if matches!(self.peek().kind, TokenKind::While) {
            self.advance();
            let condition = self.condition_header("newline or semicolon after while condition")?;
            self.loop_depth += 1;
            let body = self.block(&[TokenKind::End]);
            self.loop_depth -= 1;
            return self.close_block(start, StmtKind::While { condition, body: body? }, "end to close while block");
        }
        if matches!(self.peek().kind, TokenKind::If) {
            self.advance();
            let condition = self.condition_header("newline or semicolon after if condition")?;
            let terminators = [TokenKind::Elseif, TokenKind::Else, TokenKind::End];
            let mut branches = vec![(condition, self.block(&terminators)?)];
            while matches!(self.peek().kind, TokenKind::Elseif) {
                self.advance();
                let condition = self.condition_header("newline or semicolon after elseif condition")?;
                branches.push((condition, self.block(&terminators)?));
            }
            let else_body = if matches!(self.peek().kind, TokenKind::Else) {
                self.advance();
                self.expect(|kind| matches!(kind, TokenKind::Newline | TokenKind::Semicolon), "newline or semicolon after else")?;
                self.block(&[TokenKind::End])?
            } else { Vec::new() };
            return self.close_block(start, StmtKind::If { branches, else_body }, "end to close if block");
        }
        if matches!(self.peek().kind, TokenKind::Switch) {
            self.advance();
            let selector = self.condition_header("newline or semicolon after switch expression")?;
            self.skip_separators();
            let terminators = [TokenKind::Case, TokenKind::Otherwise, TokenKind::End];
            let mut cases = Vec::new();
            while matches!(self.peek().kind, TokenKind::Case) {
                self.advance();
                let value = self.condition_header("newline or semicolon after case expression")?;
                cases.push((value, self.block(&terminators)?));
            }
            let otherwise_body = if matches!(self.peek().kind, TokenKind::Otherwise) {
                self.advance();
                self.expect(|kind| matches!(kind, TokenKind::Newline | TokenKind::Semicolon), "newline or semicolon after otherwise")?;
                self.block(&[TokenKind::End])?
            } else { Vec::new() };
            return self.close_block(start, StmtKind::Switch { selector, cases, otherwise_body }, "end to close switch block");
        }
        if matches!(self.peek().kind, TokenKind::Try) {
            self.advance();
            self.expect(|kind| matches!(kind, TokenKind::Newline | TokenKind::Semicolon), "newline or semicolon after try")?;
            let body = self.block(&[TokenKind::Catch, TokenKind::End])?;
            self.expect(|kind| matches!(kind, TokenKind::Catch), "catch in try block")?;
            let catch_name = if matches!(self.peek().kind, TokenKind::Ident(_)) {
                let TokenKind::Ident(name) = self.advance().kind.clone() else { unreachable!() };
                Some(name)
            } else { None };
            self.expect(|kind| matches!(kind, TokenKind::Newline | TokenKind::Semicolon), "newline or semicolon after catch or its exception variable")?;
            let catch_body = self.block(&[TokenKind::End])?;
            return self.close_block(start, StmtKind::Try { body, catch_name, catch_body }, "end to close try/catch block");
        }
        if matches!(self.peek().kind, TokenKind::Break | TokenKind::Continue | TokenKind::Return) {
            let token = self.advance().clone();
            let (keyword, kind) = match token.kind {
                TokenKind::Break => ("break", StmtKind::Break),
                TokenKind::Continue => ("continue", StmtKind::Continue),
                TokenKind::Return => ("return", StmtKind::Return),
                _ => unreachable!(),
            };
            if self.loop_depth == 0 && keyword != "return" { return Err(ParseError::LoopControlOutsideLoop { offset: start, keyword }); }
            if !matches!(self.peek().kind, TokenKind::Newline | TokenKind::Semicolon | TokenKind::Eof) {
                return Err(ParseError::UnexpectedToken { offset: self.peek().span.start, expected: "newline or semicolon after control statement" });
            }
            let suppress_output = matches!(self.peek().kind, TokenKind::Semicolon);
            if suppress_output { self.advance(); }
            return Ok(Stmt { kind, span: token.span, suppress_output });
        }
        if matches!(&self.peek().kind, TokenKind::Ident(_)) && matches!(&self.peek_n(1).kind, TokenKind::LeftParen) {
            let saved = self.current;
            let name = match self.advance().kind.clone() { TokenKind::Ident(name) => name, _ => unreachable!() };
            self.advance();
            let candidate = (|| -> Result<Option<Stmt>, ParseError> {
                let row = self.argument_expression()?;
                let col = if matches!(self.peek().kind, TokenKind::Comma) {
                    self.advance();
                    Some(self.argument_expression()?)
                } else { None };
                self.expect(|kind| matches!(kind, TokenKind::RightParen), ")")?;
                if !matches!(self.peek().kind, TokenKind::Equal) { return Ok(None); }
                self.advance();
                let value = self.expression(0)?;
                let end = value.span.end;
                let suppress_output = matches!(self.peek().kind, TokenKind::Semicolon);
                if suppress_output { self.advance(); }
                let kind = match col {
                    Some(col) => StmtKind::IndexAssign { name, row, col, value },
                    None => StmtKind::LinearIndexAssign { name, index: row, value },
                };
                Ok(Some(Stmt { kind, span: Span::new(start, end), suppress_output }))
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

    fn argument_expression(&mut self) -> Result<Expr, ParseError> {
        self.argument_depth += 1;
        let result = (|| -> Result<Expr, ParseError> {
            if matches!(self.peek().kind, TokenKind::Colon) {
                let token = self.advance().clone();
                return Ok(Expr { kind: ExprKind::IndexAll, span: token.span });
            }
            self.expression(0)
        })();
        self.argument_depth -= 1;
        result
    }

    fn expression(&mut self, min_bp: u8) -> Result<Expr, ParseError> {
        let token = self.advance().clone();
        let mut lhs = match token.kind {
            TokenKind::Number(value) => Expr { kind: ExprKind::Number(value), span: token.span },
            TokenKind::Bool(value) => Expr { kind: ExprKind::Bool(value), span: token.span },
            TokenKind::String(value) => Expr { kind: ExprKind::String(value), span: token.span },
            TokenKind::Ident(name) => Expr { kind: ExprKind::Identifier(name), span: token.span },
            TokenKind::End if self.argument_depth > 0 => Expr { kind: ExprKind::IndexEnd, span: token.span },
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
            if matches!(self.peek().kind, TokenKind::Dot) {
                if 15 < min_bp { break; }
                self.advance();
                let token = self.expect(|kind| matches!(kind, TokenKind::Ident(_)), "field name after .")?;
                let TokenKind::Ident(field) = token.kind else { unreachable!() };
                lhs = Expr { span: Span::new(lhs.span.start, token.span.end), kind: ExprKind::FieldAccess { object: Box::new(lhs), field } };
                continue;
            }
            if matches!(&self.peek().kind, TokenKind::LeftParen) {
                if 15 < min_bp { break; }
                self.advance();
                let mut arguments = Vec::new();
                if !matches!(self.peek().kind, TokenKind::RightParen) {
                    loop {
                        arguments.push(self.argument_expression()?);
                        if !matches!(self.peek().kind, TokenKind::Comma) { break; }
                        self.advance();
                    }
                }
                let close = self.expect(|kind| matches!(kind, TokenKind::RightParen), ")")?;
                lhs = Expr { span: Span::new(lhs.span.start, close.span.end), kind: ExprKind::Call { callee: Box::new(lhs), arguments } };
                continue;
            }
            if matches!(self.peek().kind, TokenKind::Colon) {
                if 6 < min_bp { break; }
                if matches!(lhs.kind, ExprKind::IndexRange { .. }) {
                    return Err(ParseError::UnexpectedToken { offset: self.peek().span.start, expected: "at most two colons in a range" });
                }
                self.advance();
                let next = self.expression(7)?;
                let (step, stop) = if matches!(self.peek().kind, TokenKind::Colon) {
                    self.advance();
                    (Some(Box::new(next)), self.expression(7)?)
                } else { (None, next) };
                lhs = Expr { span: Span::new(lhs.span.start, stop.span.end),
                    kind: ExprKind::IndexRange { start: Box::new(lhs), step, stop: Box::new(stop) } };
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
                    while matches!(self.peek().kind, TokenKind::Newline) { self.advance(); }
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
    fn parses_parfor_range_as_one_statement() {
        let source = "parfor index = 1:4\n output(index) = index * index;\nend";
        let module = parse(source).unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::ParFor { name, step, body, .. } = &module.statements[0].kind else { panic!("expected parfor") };
        assert_eq!(name, "index");
        assert!(step.is_none());
        assert_eq!(body.len(), 1);
        assert_eq!(module.statements[0].span, Span::new(0, source.len()));
        assert!(parse("parfor index = 3:-1:1; output(index, 1) = index; end").is_ok());
    }

    #[test]
    fn rejects_malformed_parfor_headers_and_unclosed_blocks() {
        for source in ["parfor = 1:3; end", "parfor index 1:3; end", "parfor index = [1, 2]; end",
            "parfor index = 1; end", "parfor index = 1:; end", "parfor index = 1:3\n",
            "parfor index = 1:3 disp(index); end", "parfor(index = 1:3); end"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_bidimensional_slices() {
        let module = parse("disp(A(2:end, :)); disp(A(:, end));").unwrap();
        assert_eq!(module.statements.len(), 2);
        let StmtKind::Expr(expr) = &module.statements[0].kind else { panic!("expected expression") };
        let ExprKind::Call { arguments, .. } = &expr.kind else { panic!("expected disp call") };
        let ExprKind::Call { arguments, .. } = &arguments[0].kind else { panic!("expected indexing") };
        assert!(matches!(arguments[0].kind, ExprKind::IndexRange { step: None, .. }));
        assert!(matches!(arguments[1].kind, ExprKind::IndexAll));
    }

    #[test]
    fn parses_slice_steps_arithmetic_and_index_end() {
        let module = parse("A(end:-1:1, 1:2:end);").unwrap();
        let StmtKind::Expr(expr) = &module.statements[0].kind else { panic!("expected expression") };
        let ExprKind::Call { arguments, .. } = &expr.kind else { panic!("expected indexing") };
        let ExprKind::IndexRange { start, step, .. } = &arguments[0].kind else { panic!("expected range") };
        assert!(matches!(start.kind, ExprKind::IndexEnd));
        assert!(matches!(step.as_ref().unwrap().kind, ExprKind::Unary { op: UnaryOp::Negative, .. }));
        assert!(parse("A(first + 1:step:last - 1, B(end));").is_ok());
        let module = parse("A(2:end, :);").unwrap();
        let StmtKind::Expr(expr) = &module.statements[0].kind else { panic!("expected expression") };
        let ExprKind::Call { arguments, .. } = &expr.kind else { panic!("expected indexing") };
        assert_eq!(arguments[0].span, Span::new(2, 7));
    }

    #[test]
    fn rejects_malformed_and_standalone_slice_syntax() {
        for source in ["A(1::end, :);", "A(1:2:, :);", "A(1:2:3:4, :);", "A(:end, 1);", "A(1:, :);",
            "value = :;", "A(1:2, :); value = end;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_linear_index_assignment() {
        let source = "A(end - 1) = 7;";
        let module = parse(source).unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::LinearIndexAssign { name, index, value } = &module.statements[0].kind else { panic!("expected linear index assignment") };
        assert_eq!(name, "A");
        assert!(matches!(index.kind, ExprKind::Binary { op: BinaryOp::Subtract, .. }));
        assert!(matches!(value.kind, ExprKind::Number(7.0)));
        assert!(module.statements[0].suppress_output);
        assert_eq!(module.statements[0].span, Span::new(0, source.len() - 1));
    }

    #[test]
    fn keeps_single_argument_calls_distinct_from_index_assignments() {
        let module = parse("sqrt(4); disp(A(end)); A(B(end)) = A(end); A(1, 2) = 9;").unwrap();
        assert_eq!(module.statements.len(), 4);
        assert!(matches!(module.statements[0].kind, StmtKind::Expr(_)));
        assert!(matches!(module.statements[1].kind, StmtKind::Expr(_)));
        assert!(matches!(module.statements[2].kind, StmtKind::LinearIndexAssign { .. }));
        assert!(matches!(module.statements[3].kind, StmtKind::IndexAssign { .. }));
    }

    #[test]
    fn rejects_malformed_linear_assignments_without_leaking_index_context() {
        for source in ["A() = 7;", "A(1,) = 7;", "A(1, 2, 3) = 7;", "A(1) =;", "A(end) = end;", "A(end); value = end;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_end_in_matrix_reads_and_assignments() {
        let module = parse("disp(A(end, 1)); A(end - 1, end) = 7;").unwrap();
        assert_eq!(module.statements.len(), 2);
        let StmtKind::IndexAssign { row, col, .. } = &module.statements[1].kind else { panic!("expected indexed assignment") };
        let ExprKind::Binary { left, op: BinaryOp::Subtract, .. } = &row.kind else { panic!("expected subtraction") };
        assert!(matches!(left.kind, ExprKind::IndexEnd));
        assert!(matches!(col.kind, ExprKind::IndexEnd));
    }

    #[test]
    fn index_end_does_not_replace_block_end() {
        let module = parse("while 0; if 1; disp(A(end, end)); end; end; switch 1; case 1; A(end, end) = 7; end").unwrap();
        assert_eq!(module.statements.len(), 2);
        assert!(matches!(module.statements[0].kind, StmtKind::While { .. }));
        assert!(matches!(module.statements[1].kind, StmtKind::Switch { .. }));
    }

    #[test]
    fn parses_nested_index_end_and_preserves_its_span() {
        let module = parse("value = A(B(end, 1), sqrt(end));").unwrap();
        let StmtKind::Assign { value, .. } = &module.statements[0].kind else { panic!("expected assignment") };
        let ExprKind::Call { arguments, .. } = &value.kind else { panic!("expected call") };
        let ExprKind::Call { arguments: inner, .. } = &arguments[0].kind else { panic!("expected nested call") };
        assert!(matches!(inner[0].kind, ExprKind::IndexEnd));
        assert_eq!(inner[0].span, Span::new(12, 15));
    }

    #[test]
    fn rejects_end_as_a_standalone_value_and_restores_argument_context() {
        for source in ["value = end;", "value = (end);", "[end];", "A(end, 1) = end;",
            "disp(A(end, 1)); value = end;", "foo(end); value = end;", "for index = 1:end; end"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_named_catch_and_exception_fields() {
        let module = parse("try; disp(undefined); catch failure; disp(failure.message); end").unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::Try { catch_name, catch_body, .. } = &module.statements[0].kind else { panic!("expected try") };
        assert_eq!(catch_name.as_deref(), Some("failure"));
        let StmtKind::Expr(expr) = &catch_body[0].kind else { panic!("expected call") };
        let ExprKind::Call { arguments, .. } = &expr.kind else { panic!("expected call") };
        assert!(matches!(&arguments[0].kind, ExprKind::FieldAccess { field, .. } if field == "message"));
    }

    #[test]
    fn field_access_binds_like_other_postfix_expressions() {
        let source = "result = failure.message + other.identifier;";
        let module = parse(source).unwrap();
        let StmtKind::Assign { value, .. } = &module.statements[0].kind else { panic!("expected assignment") };
        let ExprKind::Binary { left, right, op: BinaryOp::Add } = &value.kind else { panic!("expected addition") };
        assert!(matches!(&left.kind, ExprKind::FieldAccess { field, .. } if field == "message"));
        assert!(matches!(&right.kind, ExprKind::FieldAccess { field, .. } if field == "identifier"));
        assert_eq!(left.span, Span::new(9, 24));
        assert!(parse("disp((failure).message);").is_ok());
    }

    #[test]
    fn rejects_invalid_named_catch_and_field_syntax() {
        for source in ["try; catch failure extra; end", "try; catch failure.message; end", "try; catch(failure); end",
            "try; catch return; end", "failure.", "failure.1", "failure.message = \"changed\";"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_try_catch_as_one_statement() {
        let source = "try\n value = undefined;\ncatch\n value = 0;\nend";
        let module = parse(source).unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::Try { body, catch_name, catch_body } = &module.statements[0].kind else { panic!("expected try/catch") };
        assert_eq!(body.len(), 1);
        assert!(catch_name.is_none());
        assert_eq!(catch_body.len(), 1);
        assert_eq!(module.statements[0].span, Span::new(0, source.len()));
    }

    #[test]
    fn parses_empty_and_nested_try_catch_blocks() {
        for source in ["try; catch; end", "try; try; catch; end; catch; try; catch; end; end",
            "try; if 1; return; end; catch; switch 1; case 1; return; end; end",
            "while 1; try; break; catch; continue; end; end"] {
            assert!(parse(source).is_ok(), "unexpectedly rejected {source:?}");
        }
    }

    #[test]
    fn rejects_invalid_try_catch_blocks() {
        for source in ["try disp(1); catch; end", "try; end", "try\n", "try; catch\n", "try; catch; catch; end",
            "try; catch(1); end", "try; catch disp(1); end", "catch;", "if 1; catch; end",
            "try; if 1; catch; end", "try; catch; else; end", "try = 1;", "catch = 1;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn try_cannot_catch_syntax_errors_or_invalid_loop_control() {
        for source in ["try; value =; catch; end", "try; catch; value =; end",
            "try; break; catch; end", "try; catch; continue; end"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn rejects_return_with_a_value() {
        for source in ["return 42;", "return(42);", "return();", "return = 1;", "value = return;", "if return; end", "return + 1;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_return_with_semicolon_newline_or_eof() {
        for (source, suppressed) in [("return", false), ("return\n", false), ("return;", true), ("return % comment\n", false)] {
            let module = parse(source).unwrap();
            assert_eq!(module.statements.len(), 1);
            assert!(matches!(module.statements[0].kind, StmtKind::Return));
            assert_eq!(module.statements[0].suppress_output, suppressed);
            assert_eq!(module.statements[0].span, Span::new(0, 6));
        }
    }

    #[test]
    fn parses_return_inside_each_control_flow_construct() {
        for source in ["if 1; return; elseif 1; return; else; return; end", "while 1; return; end",
            "for index = 1:3; return; end", "for column = [1; 2]; return; end",
            "switch 1; case 1; return; otherwise; return; end"] {
            assert!(parse(source).is_ok(), "unexpectedly rejected {source:?}");
        }
    }

    #[test]
    fn return_does_not_skip_syntax_validation_of_following_code() {
        for source in ["return; if 1;", "return; break;", "return; case 1;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_switch_cases_as_one_statement() {
        let source = "switch mode\ncase 1\n disp(10);\ncase 2\n disp(20);\notherwise\n disp(0);\nend";
        let module = parse(source).unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::Switch { selector, cases, otherwise_body } = &module.statements[0].kind else { panic!("expected switch") };
        assert!(matches!(&selector.kind, ExprKind::Identifier(name) if name == "mode"));
        assert_eq!(cases.len(), 2);
        assert!(cases.iter().all(|(_, body)| body.len() == 1));
        assert_eq!(otherwise_body.len(), 1);
        assert_eq!(module.statements[0].span, Span::new(0, source.len()));
    }

    #[test]
    fn parses_empty_inline_and_nested_switch_blocks() {
        for source in ["switch 1; end", "switch 1; case 1; case 2; end", "switch 1; otherwise; end",
            "switch 1; case 1; if 1; switch \"mode\"; case \"mode\"; end; end; otherwise; end",
            "for value = 1:2; switch value; case 1; continue; otherwise; break; end; end"] {
            assert!(parse(source).is_ok(), "unexpectedly rejected {source:?}");
        }
    }

    #[test]
    fn rejects_invalid_switch_blocks_and_stray_labels() {
        for source in ["switch; end", "switch 1 disp(1); end", "switch 1; disp(1); end",
            "switch 1; case; end", "switch 1; case 1 disp(1); end", "switch 1; otherwise disp(1); end",
            "switch 1; otherwise; otherwise; end", "switch 1; otherwise; case 1; end",
            "switch 1; case 1\n", "switch 1; case 1; switch 2; end", "switch 1; else; end",
            "case 1;", "otherwise;", "if 1; case 1; end", "while 0; otherwise; end",
            "switch = 1;", "case = 1;", "otherwise = 1;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn rejects_loop_control_in_switch_outside_a_loop() {
        for keyword in ["break", "continue"] {
            let source = format!("switch 1; case 0; {keyword}; end");
            assert!(matches!(parse(&source), Err(ParseError::LoopControlOutsideLoop { .. })));
        }
    }

    #[test]
    fn parses_for_matrix_source_as_one_statement() {
        let source = "for column = [1, 2; 3, 4]\n disp(column);\nend";
        let module = parse(source).unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::ForEach { name, source: iterable, body } = &module.statements[0].kind else { panic!("expected matrix for") };
        assert_eq!(name, "column");
        assert!(matches!(iterable.kind, ExprKind::Matrix(_)));
        assert_eq!(body.len(), 1);
        assert_eq!(module.statements[0].span, Span::new(0, source.len()));
    }

    #[test]
    fn parses_for_sources_with_expressions_and_loop_control() {
        for source in ["for column = A; continue; end", "for column = transpose(A); break; end",
            "for column = A + B; if 1; break; end; end", "for column = 7; end", "for column = []; end"] {
            let module = parse(source).unwrap();
            assert!(matches!(module.statements[0].kind, StmtKind::ForEach { .. }));
        }
    }

    #[test]
    fn rejects_invalid_for_sources() {
        for source in ["for column =; end", "for column = A disp(column); end", "for column = A, B; end",
            "for column = A\n", "for column = A; else; end", "for column = A; end; break;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_for_range_as_one_statement() {
        let source = "for index = 1:3\n disp(index);\nend";
        let module = parse(source).unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::For { name, start, step, stop, body } = &module.statements[0].kind else { panic!("expected for") };
        assert_eq!(name, "index");
        assert!(matches!(start.kind, ExprKind::Number(1.0)));
        assert!(step.is_none());
        assert!(matches!(stop.kind, ExprKind::Number(3.0)));
        assert_eq!(body.len(), 1);
        assert_eq!(module.statements[0].span, Span::new(0, source.len()));
    }

    #[test]
    fn parses_for_with_step_expressions_and_nested_control_flow() {
        let module = parse("for index = (1 + 4):-0.5:limit - 1; if index; continue; end; while 0; break; end; for inner = 1:2; break; end; end;").unwrap();
        let StmtKind::For { step, body, .. } = &module.statements[0].kind else { panic!("expected for") };
        assert!(matches!(step.as_ref().unwrap().kind, ExprKind::Unary { op: UnaryOp::Negative, .. }));
        assert_eq!(body.len(), 3);
    }

    #[test]
    fn rejects_invalid_for_headers_and_unclosed_blocks() {
        for source in ["for = 1:3; end", "for index 1:3; end", "for index = :3; end",
            "for index = 1:; end", "for index = 1::3; end", "for index = 1:2:; end", "for index = 1:2:3:4; end",
            "for index = 1:3 disp(index); end", "for index = 1:3\n", "for index = 1:3; if 1; end",
            "for index = 1:3; else; end", "for index = 1:3; end; continue;", "for = 1;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_if_elseif_else_as_one_statement() {
        let source = "if 0\n disp(1);\nelseif 1\n disp(2);\nelseif 2\n disp(4);\nelse\n disp(3);\nend";
        let module = parse(source).unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::If { branches, else_body } = &module.statements[0].kind else { panic!("expected if") };
        assert_eq!(branches.len(), 3);
        assert!(branches.iter().all(|(_, body)| body.len() == 1));
        assert_eq!(else_body.len(), 1);
        assert_eq!(module.statements[0].span, Span::new(0, source.len()));
    }

    #[test]
    fn parses_nested_conditionals_and_loop_control() {
        let module = parse("while 1; if 0; elseif 1; if 1; continue; end; else; break; end; end; if 1; end;").unwrap();
        assert_eq!(module.statements.len(), 2);
        assert!(matches!(module.statements[0].kind, StmtKind::While { .. }));
        assert!(matches!(module.statements[1].kind, StmtKind::If { .. }));
    }

    #[test]
    fn rejects_invalid_conditional_blocks() {
        for source in ["if 1\n", "if 1\nelse\n", "if 1; else; else; end", "if 1; else; elseif 1; end",
            "elseif 1; end", "else; end", "if\nend", "if 1 disp(1); end", "if 0; elseif; end",
            "if 0; else disp(1); end", "while 1; else; end", "if 1; while 0; end"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn rejects_loop_control_outside_loops_even_in_unexecuted_branches() {
        for keyword in ["break", "continue"] {
            for source in [format!("{keyword};"), format!("if 0; {keyword}; end"), format!("while 0; end; {keyword};")] {
                assert!(matches!(parse(&source), Err(ParseError::LoopControlOutsideLoop { .. })), "unexpected result for {source:?}");
            }
        }
    }

    #[test]
    fn rejects_arguments_to_loop_control_statements() {
        for source in ["while 1; break(1); end", "while 1; continue 2; end", "while 1; break = 1; end"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

    #[test]
    fn parses_while_block() {
        let source = "while counter < 3\n    counter = counter + 1;\nend";
        let module = parse(source).unwrap();
        assert_eq!(module.statements.len(), 1);
        let StmtKind::While { condition, body } = &module.statements[0].kind else { panic!("expected while") };
        assert!(matches!(condition.kind, ExprKind::Binary { op: BinaryOp::Less, .. }));
        assert_eq!(body.len(), 1);
        assert!(body[0].suppress_output);
        assert_eq!(module.statements[0].span, Span::new(0, source.len()));
    }

    #[test]
    fn parses_nested_and_inline_while_blocks() {
        let module = parse("while 1; while 0; end; end; disp(3);").unwrap();
        assert_eq!(module.statements.len(), 2);
        let StmtKind::While { body, .. } = &module.statements[0].kind else { panic!("expected while") };
        assert!(matches!(body[0].kind, StmtKind::While { .. }));
    }

    #[test]
    fn rejects_unclosed_while_blocks() {
        for source in ["while 1\n", "while 1\nwhile 0\nend\n"] {
            assert!(matches!(parse(source), Err(ParseError::UnexpectedToken { expected: "end to close while block", .. })));
        }
    }

    #[test]
    fn rejects_invalid_while_headers_and_stray_end() {
        for source in ["while\nend", "while 1 disp(1); end", "end", "while = 1;", "end = 1;"] {
            assert!(parse(source).is_err(), "unexpectedly accepted {source:?}");
        }
    }

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
        assert!(matches!(&row.kind, ExprKind::Number(n) if *n == 2.0));
        assert!(matches!(&col.kind, ExprKind::Number(n) if *n == 3.0));
        assert!(matches!(&value.kind, ExprKind::Number(n) if *n == 10.0));
    }

    #[test]
    fn keeps_function_calls_as_expression_statements() {
        let module = parse("disp(A(1, 2));").unwrap();
        assert!(matches!(&module.statements[0].kind, StmtKind::Expr(_)));
    }

    #[test]
    fn parses_matrix_literal() {
        let module = parse("A = [1, 2; 3, 4];").unwrap();
        let StmtKind::Assign { value, .. } = &module.statements[0].kind else { panic!("expected assignment") };
        assert!(matches!(&value.kind, ExprKind::Matrix(rows) if rows.len() == 2 && rows[0].len() == 2));
    }

    #[test]
    fn parses_matrix_rows_with_semicolon_and_newline() {
        let module = parse("A = [120, 80, 50;\n    150, 60, 70;\n    100, 110, 90];").unwrap();
        let StmtKind::Assign { value, .. } = &module.statements[0].kind else { panic!("expected assignment") };
        let ExprKind::Matrix(rows) = &value.kind else { panic!("expected matrix") };
        assert_eq!(rows.len(), 3);
        for (row, expected) in rows.iter().zip([[120.0, 80.0, 50.0], [150.0, 60.0, 70.0], [100.0, 110.0, 90.0]]) {
            assert_eq!(row.len(), expected.len());
            for (element, expected_value) in row.iter().zip(expected) {
                assert!(matches!(&element.kind, ExprKind::Number(value) if *value == expected_value));
            }
        }
        assert!(module.statements[0].suppress_output);
    }
}

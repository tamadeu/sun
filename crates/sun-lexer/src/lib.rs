//! Lexer for the initial Sun 0.1 syntax.

use sun_source::Span;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Number(f64), Ident(String), String(String),
    Plus, Minus, Star, DotStar, Slash, DotSlash, Caret,
    Equal, EqualEqual, NotEqual, Less, LessEqual, Greater, GreaterEqual,
    Bang, AndAnd, OrOr,
    LeftParen, RightParen, LeftBracket, RightBracket,
    Comma, Semicolon, Colon, Dot, Newline, Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token { pub kind: TokenKind, pub span: Span }

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum LexError {
    #[error("unexpected character {character} at byte {offset}")]
    UnexpectedCharacter { character: char, offset: usize },
    #[error("unterminated string starting at byte {offset}")]
    UnterminatedString { offset: usize },
    #[error("invalid number starting at byte {offset}")]
    InvalidNumber { offset: usize },
}

pub fn lex(source: &str) -> Result<Vec<Token>, LexError> {
    let mut lexer = Lexer { source, offset: 0, tokens: Vec::new() };
    lexer.scan()?;
    lexer.tokens.push(Token { kind: TokenKind::Eof, span: Span::new(source.len(), source.len()) });
    Ok(lexer.tokens)
}

struct Lexer<'a> { source: &'a str, offset: usize, tokens: Vec<Token> }

impl Lexer<'_> {
    fn scan(&mut self) -> Result<(), LexError> {
        while self.offset < self.source.len() {
            let start = self.offset;
            let ch = self.peek().expect("offset is in bounds");
            match ch {
                ' ' | '\t' | '\r' => { self.bump(); }
                '\n' => { self.bump(); self.push(TokenKind::Newline, start); }
                '%' => while self.peek().is_some_and(|c| c != '\n') { self.bump(); },
                '0'..='9' => self.number(start)?,
                'a'..='z' | 'A'..='Z' | '_' => {
                    self.bump();
                    while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') { self.bump(); }
                    self.push(TokenKind::Ident(self.source[start..self.offset].to_string()), start);
                }
                '"' | '\'' => self.string(start, ch)?,
                '+' => { self.bump(); self.push(TokenKind::Plus, start); }
                '-' => { self.bump(); self.push(TokenKind::Minus, start); }
                '*' => { self.bump(); self.push(TokenKind::Star, start); }
                '.' if self.peek_next() == Some('*') => { self.bump(); self.bump(); self.push(TokenKind::DotStar, start); }
                '.' if self.peek_next() == Some('/') => { self.bump(); self.bump(); self.push(TokenKind::DotSlash, start); }
                '/' => { self.bump(); self.push(TokenKind::Slash, start); }
                '^' => { self.bump(); self.push(TokenKind::Caret, start); }
                '=' if self.peek_next() == Some('=') => { self.bump(); self.bump(); self.push(TokenKind::EqualEqual, start); }
                '=' => { self.bump(); self.push(TokenKind::Equal, start); }
                '~' if self.peek_next() == Some('=') => { self.bump(); self.bump(); self.push(TokenKind::NotEqual, start); }
                '<' if self.peek_next() == Some('=') => { self.bump(); self.bump(); self.push(TokenKind::LessEqual, start); }
                '<' => { self.bump(); self.push(TokenKind::Less, start); }
                '>' if self.peek_next() == Some('=') => { self.bump(); self.bump(); self.push(TokenKind::GreaterEqual, start); }
                '>' => { self.bump(); self.push(TokenKind::Greater, start); }
                '!' => { self.bump(); self.push(TokenKind::Bang, start); }
                '&' if self.peek_next() == Some('&') => { self.bump(); self.bump(); self.push(TokenKind::AndAnd, start); }
                '|' if self.peek_next() == Some('|') => { self.bump(); self.bump(); self.push(TokenKind::OrOr, start); }
                '(' => { self.bump(); self.push(TokenKind::LeftParen, start); }
                ')' => { self.bump(); self.push(TokenKind::RightParen, start); }
                '[' => { self.bump(); self.push(TokenKind::LeftBracket, start); }
                ']' => { self.bump(); self.push(TokenKind::RightBracket, start); }
                ',' => { self.bump(); self.push(TokenKind::Comma, start); }
                ';' => { self.bump(); self.push(TokenKind::Semicolon, start); }
                ':' => { self.bump(); self.push(TokenKind::Colon, start); }
                '.' => { self.bump(); self.push(TokenKind::Dot, start); }
                other => return Err(LexError::UnexpectedCharacter { character: other, offset: start }),
            }
        }
        Ok(())
    }

    fn number(&mut self, start: usize) -> Result<(), LexError> {
        while self.peek().is_some_and(|c| c.is_ascii_digit()) { self.bump(); }
        if self.peek() == Some('.') && self.peek_next().is_some_and(|c| c.is_ascii_digit()) {
            self.bump();
            while self.peek().is_some_and(|c| c.is_ascii_digit()) { self.bump(); }
        }
        if self.peek().is_some_and(|c| c == 'e' || c == 'E') {
            let exponent = self.offset;
            self.bump();
            if self.peek().is_some_and(|c| c == '+' || c == '-') { self.bump(); }
            let digits = self.offset;
            while self.peek().is_some_and(|c| c.is_ascii_digit()) { self.bump(); }
            if digits == self.offset { return Err(LexError::InvalidNumber { offset: exponent }); }
        }
        let value = self.source[start..self.offset].parse().map_err(|_| LexError::InvalidNumber { offset: start })?;
        self.push(TokenKind::Number(value), start);
        Ok(())
    }

    fn string(&mut self, start: usize, quote: char) -> Result<(), LexError> {
        self.bump();
        let content_start = self.offset;
        while let Some(c) = self.peek() {
            if c == quote {
                let value = self.source[content_start..self.offset].to_string();
                self.bump();
                self.push(TokenKind::String(value), start);
                return Ok(());
            }
            if c == '\n' { break; }
            self.bump();
        }
        Err(LexError::UnterminatedString { offset: start })
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token { kind, span: Span::new(start, self.offset) });
    }
    fn peek(&self) -> Option<char> { self.source[self.offset..].chars().next() }
    fn peek_next(&self) -> Option<char> {
        let mut chars = self.source[self.offset..].chars();
        chars.next()?;
        chars.next()
    }
    fn bump(&mut self) { if let Some(c) = self.peek() { self.offset += c.len_utf8(); } }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_numbers_identifiers_and_matrix_operators() {
        let tokens = lex("A = 2.5 .* B;").unwrap();
        assert!(matches!(tokens[0].kind, TokenKind::Ident(ref name) if name == "A"));
        assert!(matches!(&tokens[2].kind, TokenKind::Number(n) if *n == 2.5));
        assert_eq!(tokens[3].kind, TokenKind::DotStar);
    }

    #[test]
    fn ignores_matlab_style_comments() {
        let tokens = lex("x = 1; % comment\ny = 2;").unwrap();
        assert!(tokens.iter().any(|token| token.kind == TokenKind::Newline));
        assert_eq!(tokens.iter().filter(|token| matches!(&token.kind, TokenKind::Ident(_))).count(), 2);
    }
}

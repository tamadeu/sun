//! Source locations shared by the Sun frontend.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self { Self { start, end } }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> Self {
        Self { name: name.into(), text: text.into() }
    }

    pub fn line_column(&self, offset: usize) -> (usize, usize) {
        let bounded = offset.min(self.text.len());
        let prefix = &self.text[..self.text.floor_char_boundary(bounded)];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        (line, column)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_line_and_column() {
        let source = SourceFile::new("test.sun", "x = 1;\ny = 2;");
        assert_eq!(source.line_column(7), (2, 1));
    }
}

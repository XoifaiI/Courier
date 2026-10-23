//! Splits schema text into tokens.

use crate::error::{SchemaError, Span};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Token<'s> {
    Name(&'s str),
    Number(f64),
    Text(&'s str),
    Symbol(&'static str),
    End,
}

impl Token<'_> {
    pub fn describe(&self) -> String {
        match self {
            Token::Name(name) => format!("\"{name}\""),
            Token::Number(number) => number.to_string(),
            Token::Text(text) => format!("\"{text}\""),
            Token::Symbol(symbol) => format!("\"{symbol}\""),
            Token::End => "end of file".to_string(),
        }
    }
}

const SYMBOLS: [&str; 12] = ["..", "->", "{", "}", "(", ")", "[", "]", ":", ",", "?", "="];

pub fn tokenize(source: &str) -> Result<Vec<(Token<'_>, Span)>, (Span, SchemaError)> {
    tokenize_file(source, 0)
}

pub fn tokenize_file(source: &str, file: u32) -> Result<Vec<(Token<'_>, Span)>, (Span, SchemaError)> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::with_capacity(source.len() / 4);
    let mut index = 0;
    let mut line = 1;
    let mut line_start = 0;

    while index < bytes.len() {
        let span = Span { file, line, column: (index - line_start + 1) as u32 };
        let byte = bytes[index];
        let rest = &source[index..];

        if byte == b'\n' {
            line += 1;
            index += 1;
            line_start = index;
        } else if byte.is_ascii_whitespace() {
            index += 1;
        } else if rest.starts_with("--") {
            index += rest.find('\n').unwrap_or(rest.len());
        } else if byte == b'"' {
            let length = rest[1..].find(['"', '\n']).filter(|end| rest.as_bytes()[1 + end] == b'"');
            let Some(length) = length else { return Err((span, SchemaError::UnterminatedText)) };
            tokens.push((Token::Text(&rest[1..1 + length]), span));
            index += length + 2;
        } else if let Some(symbol) = SYMBOLS.iter().find(|symbol| rest.starts_with(**symbol)) {
            tokens.push((Token::Symbol(symbol), span));
            index += symbol.len();
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            let length = rest.bytes().take_while(|byte| byte.is_ascii_alphanumeric() || *byte == b'_').count();
            tokens.push((Token::Name(&rest[..length]), span));
            index += length;
        } else if byte.is_ascii_digit() || byte == b'-' {
            let length = number_length(rest.as_bytes());
            let text = &rest[..length];
            let number = text.parse().map_err(|_| (span, SchemaError::NotANumber(text.to_string())))?;
            tokens.push((Token::Number(number), span));
            index += length;
        } else {
            let character = rest.chars().next().unwrap_or('?');
            return Err((span, SchemaError::UnexpectedCharacter(character)));
        }
    }
    tokens.push((Token::End, Span { file, line, column: (index - line_start + 1) as u32 }));
    Ok(tokens)
}

fn number_length(bytes: &[u8]) -> usize {
    let digits = |from: usize| from + bytes[from..].iter().take_while(|byte| byte.is_ascii_digit()).count();
    let whole = digits(1);
    if bytes.get(whole) == Some(&b'.') && bytes.get(whole + 1).is_some_and(u8::is_ascii_digit) {
        digits(whole + 1)
    } else {
        whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<Token<'_>> {
        tokenize(source).expect("valid source").into_iter().map(|(token, _)| token).collect()
    }

    #[test]
    fn ranges_split_from_numbers() {
        assert_eq!(kinds("1..5"), [Token::Number(1.0), Token::Symbol(".."), Token::Number(5.0), Token::End]);
        assert_eq!(kinds("-0.25..2"), [Token::Number(-0.25), Token::Symbol(".."), Token::Number(2.0), Token::End]);
    }

    #[test]
    fn comments_run_to_the_end_of_the_line() {
        assert_eq!(kinds("a -- b c\nd"), [Token::Name("a"), Token::Name("d"), Token::End]);
    }

    #[test]
    fn spans_count_lines_and_columns_from_one() {
        let tokens = tokenize("a\n  b").expect("valid source");
        assert_eq!(tokens[1].1, Span { file: 0, line: 2, column: 3 });
    }

    #[test]
    fn text_runs_between_quotes() {
        assert_eq!(kinds("x = \"a.b\""), [Token::Name("x"), Token::Symbol("="), Token::Text("a.b"), Token::End]);
        assert!(matches!(tokenize("\"a\nb\""), Err((_, SchemaError::UnterminatedText))));
    }

    #[test]
    fn a_stray_character_is_refused() {
        assert!(matches!(tokenize("a $"), Err((_, SchemaError::UnexpectedCharacter('$')))));
    }
}

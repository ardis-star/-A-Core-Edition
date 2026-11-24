//! Lexer for -A Core.
//!
//! Turns source text into tokens. Both `//` and `/* ... */` comments
//! are supported. Single- and double-quoted strings are equivalent.

use crate::diagnostics::{lex_error, CompileResult, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Ident(String),
    Int(i64),
    Double(f64),
    Str(String),
    Block,
    Reuse,
    Set,
    Setc,
    When,
    Else,
    Anyelse,
    Loop,
    While,
    Then,
    And,
    Or,
    Each,
    In,
    As,
    Struct,
    Enum,
    Trait,
    Impl,
    For,
    Pub,
    Import,
    Module,
    Fail,
    SelfKw,
    Insert,
    Remove,
    Return,
    Aout,
    On,
    Off,
    /// The `A` execution marker (only valid inside `aout()`).
    A,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Question,
    At,
    Arrow,
    Eof,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
    /// Source text of the token (for error messages).
    pub text: String,
    /// True when at least one newline separates this token from the previous
    /// one. The parser uses it to decide where a statement ends.
    pub nl_before: bool,
}

fn keyword(word: &str) -> Option<TokenKind> {
    Some(match word {
        "block" => TokenKind::Block,
        "reuse" => TokenKind::Reuse,
        "set" => TokenKind::Set,
        "setc" => TokenKind::Setc,
        "when" => TokenKind::When,
        "else" => TokenKind::Else,
        "anyelse" => TokenKind::Anyelse,
        "loop" => TokenKind::Loop,
        "while" => TokenKind::While,
        "then" => TokenKind::Then,
        "and" => TokenKind::And,
        "or" => TokenKind::Or,
        "each" => TokenKind::Each,
        "in" => TokenKind::In,
        "as" => TokenKind::As,
        "struct" => TokenKind::Struct,
        "enum" => TokenKind::Enum,
        "trait" => TokenKind::Trait,
        "impl" => TokenKind::Impl,
        "for" => TokenKind::For,
        "pub" => TokenKind::Pub,
        "import" => TokenKind::Import,
        "module" => TokenKind::Module,
        "fail" => TokenKind::Fail,
        "self" => TokenKind::SelfKw,
        "insert" => TokenKind::Insert,
        "remove" => TokenKind::Remove,
        "return" => TokenKind::Return,
        "aout" => TokenKind::Aout,
        "on" => TokenKind::On,
        "off" => TokenKind::Off,
        "A" => TokenKind::A,
        _ => return None,
    })
}

pub fn lex(src: &str) -> CompileResult<Vec<Token>> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;
    let mut col = 1usize;
    // Set when a newline is crossed; consumed by the next token pushed.
    let mut nl_before = false;

    let peek = |i: usize, chars: &Vec<char>| -> Option<char> { chars.get(i).copied() };

    while i < chars.len() {
        let c = chars[i];
        let span = Span::new(line, col);

        // Whitespace.
        if c.is_whitespace() {
            if c == '\n' {
                line += 1;
                col = 1;
                nl_before = true;
            } else {
                col += 1;
            }
            i += 1;
            continue;
        }

        // Line comment.
        if c == '/' && peek(i + 1, &chars) == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
                col += 1;
            }
            continue;
        }

        // Block comment.
        if c == '/' && peek(i + 1, &chars) == Some('*') {
            let start = span;
            i += 2;
            col += 2;
            let mut closed = false;
            while i < chars.len() {
                if chars[i] == '*' && peek(i + 1, &chars) == Some('/') {
                    i += 2;
                    col += 2;
                    closed = true;
                    break;
                }
                if chars[i] == '\n' {
                    line += 1;
                    col = 1;
                    nl_before = true;
                } else {
                    col += 1;
                }
                i += 1;
            }
            if !closed {
                return Err(lex_error("unterminated block comment", start));
            }
            continue;
        }

        // String literal (single or double quotes, interchangeable).
        if c == '"' || c == '\'' {
            let quote = c;
            let start = span;
            i += 1;
            col += 1;
            let mut value = String::new();
            let mut closed = false;
            while i < chars.len() {
                let ch = chars[i];
                if ch == quote {
                    i += 1;
                    col += 1;
                    closed = true;
                    break;
                }
                if ch == '\n' {
                    return Err(lex_error("unterminated string literal", start));
                }
                if ch == '\\' {
                    i += 1;
                    col += 1;
                    if i >= chars.len() {
                        break;
                    }
                    let esc = chars[i];
                    match esc {
                        'n' => value.push('\n'),
                        't' => value.push('\t'),
                        'r' => value.push('\r'),
                        '0' => value.push('\0'),
                        '\\' => value.push('\\'),
                        '\'' => value.push('\''),
                        '"' => value.push('"'),
                        other => {
                            return Err(lex_error(
                                format!("unknown escape sequence '\\{}'", other),
                                Span::new(line, col),
                            ))
                        }
                    }
                    i += 1;
                    col += 1;
                    continue;
                }
                value.push(ch);
                i += 1;
                col += 1;
            }
            if !closed {
                return Err(lex_error("unterminated string literal", start));
            }
            tokens.push(Token {
                kind: TokenKind::Str(value.clone()),
                span: start,
                text: value,
                nl_before,
            });
            nl_before = false;
            continue;
        }

        // Number literal: int, or double when a '.' is followed by a digit.
        if c.is_ascii_digit() {
            let start_col = col;
            let mut text = String::new();
            while i < chars.len() && chars[i].is_ascii_digit() {
                text.push(chars[i]);
                i += 1;
                col += 1;
            }
            let mut is_double = false;
            if i < chars.len()
                && chars[i] == '.'
                && peek(i + 1, &chars)
                    .map(|n| n.is_ascii_digit())
                    .unwrap_or(false)
            {
                is_double = true;
                text.push('.');
                i += 1;
                col += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    text.push(chars[i]);
                    i += 1;
                    col += 1;
                }
            }
            let _ = start_col;
            if is_double {
                match text.parse::<f64>() {
                    Ok(v) => {
                        tokens.push(Token {
                            kind: TokenKind::Double(v),
                            span,
                            text,
                            nl_before,
                        });
                        nl_before = false;
                    }
                    Err(_) => {
                        return Err(lex_error(
                            format!("invalid number literal '{}'", text),
                            span,
                        ))
                    }
                }
            } else {
                match text.parse::<i64>() {
                    Ok(v) => {
                        tokens.push(Token {
                            kind: TokenKind::Int(v),
                            span,
                            text,
                            nl_before,
                        });
                        nl_before = false;
                    }
                    Err(_) => {
                        return Err(lex_error(
                            format!("integer literal '{}' is out of range", text),
                            span,
                        ))
                    }
                }
            }
            continue;
        }

        // Identifier or keyword.
        if c.is_ascii_alphabetic() || c == '_' {
            let mut text = String::new();
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                text.push(chars[i]);
                i += 1;
                col += 1;
            }
            let kind = keyword(&text).unwrap_or_else(|| TokenKind::Ident(text.clone()));
            tokens.push(Token {
                kind,
                span,
                text,
                nl_before,
            });
            nl_before = false;
            continue;
        }

        // Symbols.
        let (kind, len): (TokenKind, usize) = match c {
            '(' => (TokenKind::LParen, 1),
            ')' => (TokenKind::RParen, 1),
            '{' => (TokenKind::LBrace, 1),
            '}' => (TokenKind::RBrace, 1),
            '[' => (TokenKind::LBracket, 1),
            ']' => (TokenKind::RBracket, 1),
            ',' => (TokenKind::Comma, 1),
            ':' => (TokenKind::Colon, 1),
            '.' => (TokenKind::Dot, 1),
            '+' => (TokenKind::Plus, 1),
            '-' => {
                if peek(i + 1, &chars) == Some('>') {
                    (TokenKind::Arrow, 2)
                } else {
                    (TokenKind::Minus, 1)
                }
            }
            '*' => (TokenKind::Star, 1),
            '/' => (TokenKind::Slash, 1),
            '%' => (TokenKind::Percent, 1),
            '&' => (TokenKind::Amp, 1),
            '?' => (TokenKind::Question, 1),
            '@' => (TokenKind::At, 1),
            '=' => {
                if peek(i + 1, &chars) == Some('=') {
                    (TokenKind::EqEq, 2)
                } else {
                    (TokenKind::Eq, 1)
                }
            }
            '!' => {
                if peek(i + 1, &chars) == Some('=') {
                    (TokenKind::NotEq, 2)
                } else {
                    return Err(lex_error(
                        "unexpected character '!'; use '!=' for not-equal",
                        span,
                    ));
                }
            }
            '<' => {
                if peek(i + 1, &chars) == Some('=') {
                    (TokenKind::Le, 2)
                } else {
                    (TokenKind::Lt, 1)
                }
            }
            '>' => {
                if peek(i + 1, &chars) == Some('=') {
                    (TokenKind::Ge, 2)
                } else {
                    (TokenKind::Gt, 1)
                }
            }
            other => {
                // ';' is the one stray character people write by reflex, so it
                // deserves a suggestion rather than a bare rejection.
                let err = lex_error(format!("unexpected character '{}'", other), span);
                return Err(if other == ';' {
                    err.with_hint(
                        "-A statements end at the line break: delete the ';' and start the next statement on its own line",
                    )
                } else {
                    err
                });
            }
        };
        let text: String = chars[i..i + len].iter().collect();
        tokens.push(Token {
            kind,
            span,
            text,
            nl_before,
        });
        nl_before = false;
        i += len;
        col += len;
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::new(line, col),
        text: String::new(),
        nl_before,
    });
    Ok(tokens)
}

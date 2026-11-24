//! Compiler diagnostics: structured errors with source spans.
//!
//! Every failure in the -A pipeline is reported as a [`Diagnostic`]
//! (never a panic) so the CLI can print a useful message and exit
//! with a non-zero status.

use std::fmt;

/// A position in the source file (1-based line and column).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(line: usize, col: usize) -> Self {
        Span { line, col }
    }

    pub fn dummy() -> Self {
        Span { line: 1, col: 1 }
    }
}

impl Default for Span {
    fn default() -> Self {
        Span::dummy()
    }
}

/// A single compiler diagnostic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Stable machine-readable code, e.g. `E_PARSE`, `E_QUEUE`.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
    /// Where the problem was found.
    pub span: Span,
    /// Source file name, filled in by the CLI when available.
    pub file: String,
    /// Optional suggestion shown by the CLI (`Help:` line).
    pub hint: Option<String>,
}

impl Diagnostic {
    pub fn new(code: impl Into<String>, message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            code: code.into(),
            message: message.into(),
            span,
            file: String::new(),
            hint: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }

    /// Attach an actionable suggestion. Hints are printed by the CLI but never
    /// change the machine-readable code or the message.
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// The category of the failure, in words, for people rather than scripts.
    pub fn category(&self) -> &'static str {
        match self.code.as_str() {
            "E_LEX" => "lexing",
            "E_PARSE" => "syntax",
            "E_SEMANTIC" => "name and meaning check",
            "E_TYPE" => "type check",
            "E_QUEUE" => "block scheduling",
            "E_IO" => "input/output",
            "E_TOOLCHAIN" => "assembler/linker",
            "E_USAGE" => "command line",
            _ => "compiler",
        }
    }

    /// Multi-line, human-readable report printed by the CLI:
    ///
    /// ```text
    /// -A Core Edition: error
    ///   File:    hello.ma
    ///   Line:    12:5
    ///   Code:    E_TYPE (type check)
    ///   Reason:  a condition must be bool (on/off), found int
    ///   Help:    compare explicitly, e.g. `when x != 0`
    /// ```
    ///
    /// Every field the CLI promises is present: file, line and column,
    /// category, explanation and (when known) a suggested correction.
    pub fn report(&self) -> String {
        let mut out = String::new();
        out.push_str("-A Core Edition: error\n");
        // A source location only exists once there is a source file; usage
        // errors have neither, and printing a fake location would be noise.
        if !self.file.is_empty() {
            out.push_str(&format!("  File:    {}\n", self.file));
            out.push_str(&format!(
                "  Line:    {}:{}\n",
                self.span.line, self.span.col
            ));
        }
        out.push_str(&format!("  Code:    {} ({})\n", self.code, self.category()));
        out.push_str(&format!("  Reason:  {}\n", self.message));
        if let Some(hint) = &self.hint {
            out.push_str(&format!("  Help:    {}\n", hint));
        }
        out
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.file.is_empty() {
            write!(
                f,
                "error[{}]: {}\n  --> line {}:{}",
                self.code, self.message, self.span.line, self.span.col
            )
        } else {
            write!(
                f,
                "error[{}]: {}\n  --> {}:{}:{}",
                self.code, self.message, self.file, self.span.line, self.span.col
            )
        }
    }
}

impl std::error::Error for Diagnostic {}

/// Convenience alias used throughout the compiler.
pub type CompileResult<T> = Result<T, Diagnostic>;

pub fn lex_error(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new("E_LEX", message, span)
}

pub fn parse_error(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new("E_PARSE", message, span)
}

pub fn semantic_error(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new("E_SEMANTIC", message, span)
}

pub fn type_error(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new("E_TYPE", message, span)
}

pub fn queue_error(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::new("E_QUEUE", message, span)
}

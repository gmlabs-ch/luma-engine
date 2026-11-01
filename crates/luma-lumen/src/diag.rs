//! Diagnostics: what the parser and the checker report, and how they are
//! printed.

use std::fmt::Write;

use crate::ast::{SourceFile, Span};

/// How serious a diagnostic is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Something to look at; the story still runs.
    Warning,
    /// The story cannot run.
    Error,
}

/// A problem found in a story.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Error or warning.
    pub severity: Severity,
    /// What is wrong, in one sentence without a final period.
    pub message: String,
    /// Where.
    pub span: Span,
    /// A suggestion, shown next to the caret.
    pub help: Option<String>,
}

impl Diagnostic {
    /// An error at `span`.
    pub fn error(span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            message: message.into(),
            span,
            help: None,
        }
    }

    /// A warning at `span`.
    pub fn warning(span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            severity: Severity::Warning,
            message: message.into(),
            span,
            help: None,
        }
    }

    /// Adds a suggestion.
    pub fn with_help(mut self, help: impl Into<String>) -> Diagnostic {
        self.help = Some(help.into());
        self
    }

    /// Renders the diagnostic the way the command line shows it:
    ///
    /// ```text
    /// error: unknown knot `flamme`
    ///   --> chapter1/lab.lumen:42:31
    ///    |
    /// 42 |   - Test it in the flame -> flamme
    ///    |                               ^^^^^^ did you mean `flame`?
    /// ```
    pub fn render(&self, files: &[SourceFile]) -> String {
        let mut out = String::new();
        let label = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let _ = writeln!(out, "{label}: {}", self.message);
        let Some(file) = files.get(self.span.file) else {
            return out;
        };
        let _ = writeln!(
            out,
            "  --> {}:{}:{}",
            file.path, self.span.line, self.span.col
        );
        let line = file.line(self.span.line);
        let number = self.span.line.to_string();
        let gutter = " ".repeat(number.len());
        let _ = writeln!(out, "{gutter} |");
        let _ = writeln!(out, "{number} | {line}");
        let pad = " ".repeat(self.span.col.saturating_sub(1) as usize);
        let carets = "^".repeat(self.span.len.max(1) as usize);
        let _ = write!(out, "{gutter} | {pad}{carets}");
        if let Some(help) = &self.help {
            let _ = write!(out, " {help}");
        }
        out.push('\n');
        out
    }
}

/// Whether any diagnostic is an error.
pub fn has_errors(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|d| d.severity == Severity::Error)
}

/// The closest of `candidates` to `name`, when it is close enough to be a
/// typo (an edit distance of two, or a third of the length up to three).
pub fn did_you_mean<'a>(name: &str, candidates: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (name.chars().count() / 3).clamp(2, 3);
    candidates
        .map(|c| (edit_distance(name, c), c))
        .filter(|(d, _)| *d <= limit)
        .min_by_key(|(d, _)| *d)
        .map(|(_, c)| c)
}

/// The Levenshtein distance between two strings.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_like_the_spec() {
        let files = vec![SourceFile {
            path: "chapter1/lab.lumen".to_string(),
            text: "x\n".repeat(41) + "  - Test it in the flame -> flamme\n",
        }];
        let d = Diagnostic::error(
            Span {
                file: 0,
                line: 42,
                col: 31,
                len: 6,
            },
            "unknown knot `flamme`",
        )
        .with_help("did you mean `flame`?");
        assert_eq!(
            d.render(&files),
            "error: unknown knot `flamme`\n  --> chapter1/lab.lumen:42:31\n   |\n42 |   - Test it in the flame -> flamme\n   |                               ^^^^^^ did you mean `flame`?\n"
        );
    }

    #[test]
    fn suggests_close_names_only() {
        let knots = ["flame", "lab_night", "conseil"];
        assert_eq!(did_you_mean("flamme", knots.into_iter()), Some("flame"));
        assert_eq!(did_you_mean("conseill", knots.into_iter()), Some("conseil"));
        assert_eq!(did_you_mean("kitchen", knots.into_iter()), None);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }
}

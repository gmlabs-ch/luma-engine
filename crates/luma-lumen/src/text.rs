//! Inline tags of a line: `${expr}`, `{pause 0.6}`, `{shake}...{/shake}`,
//! `{seq: a | b | c}`.

use crate::ast::{AltKind, Segment, Span, Text};
use crate::diag::Diagnostic;
use crate::expr;

/// Parses a line's text. `base` is the span of the text's first
/// character; the diagnostics get the exact columns.
pub fn parse_text(src: &str, base: Span, diags: &mut Vec<Diagnostic>) -> Text {
    let chars: Vec<char> = src.chars().collect();
    let mut segments = Vec::new();
    let mut plain = String::new();
    let mut i = 0;
    let at = |offset: usize, len: usize| Span {
        col: base.col + offset as u32,
        len: len.max(1) as u32,
        ..base
    };
    while i < chars.len() {
        let c = chars[i];
        let insertion = c == '$' && chars.get(i + 1) == Some(&'{');
        if insertion || c == '{' {
            let open = if insertion { i + 1 } else { i };
            let Some(close) = (open + 1..chars.len()).find(|&j| chars[j] == '}') else {
                diags.push(Diagnostic::error(
                    at(i, chars.len() - i),
                    if insertion {
                        "unterminated `${`"
                    } else {
                        "unterminated `{`"
                    },
                ));
                plain.extend(&chars[i..]);
                break;
            };
            if !plain.is_empty() {
                segments.push(Segment::Text(std::mem::take(&mut plain)));
            }
            let inner: String = chars[open + 1..close].iter().collect();
            if insertion {
                match expr::parse(&inner, at(open + 1, inner.chars().count())) {
                    Ok(e) => segments.push(Segment::Value(e)),
                    Err(err) => diags.push(Diagnostic::error(
                        at(open + 1 + err.offset as usize, 1),
                        format!("bad expression in `${{...}}`: {}", err.message),
                    )),
                }
            } else {
                segments.push(tag(&inner, at(open + 1, inner.chars().count()), diags));
            }
            i = close + 1;
        } else {
            plain.push(c);
            i += 1;
        }
    }
    if !plain.is_empty() {
        segments.push(Segment::Text(plain));
    }
    Text(segments)
}

fn tag(inner: &str, span: Span, diags: &mut Vec<Diagnostic>) -> Segment {
    let trimmed = inner.trim();
    if let Some(name) = trimmed.strip_prefix('/') {
        return Segment::Tag {
            name: name.trim().to_string(),
            args: String::new(),
            close: true,
        };
    }
    for (prefix, kind) in [
        ("seq:", AltKind::Seq),
        ("cycle:", AltKind::Cycle),
        ("shuffle:", AltKind::Shuffle),
    ] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let mut items = Vec::new();
            let mut offset = inner.find(prefix).unwrap_or(0) + prefix.len();
            for item in rest.split('|') {
                let lead = item.len() - item.trim_start().len();
                let item_span = Span {
                    col: span.col + inner[..offset + lead].chars().count() as u32,
                    ..span
                };
                items.push(parse_text(item.trim(), item_span, diags));
                offset += item.len() + 1;
            }
            if items.len() < 2 {
                diags.push(Diagnostic::warning(
                    span,
                    format!("`{{{prefix} ...}}` has a single alternative"),
                ));
            }
            return Segment::Alt {
                kind,
                items,
                key: span,
            };
        }
    }
    let (name, args) = match trimmed.split_once(char::is_whitespace) {
        Some((n, a)) => (n.to_string(), a.trim().to_string()),
        None => (trimmed.to_string(), String::new()),
    };
    if name.is_empty() {
        diags.push(Diagnostic::error(span, "empty `{}` tag"));
    }
    Segment::Tag {
        name,
        args,
        close: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expr::Expr;

    #[test]
    fn splits_text_values_tags_and_alternations() {
        let mut diags = Vec::new();
        let text = parse_text(
            "Hi ${n} there.{pause 0.6} {shake}Red{/shake} {seq: A | B|C}",
            Span::default(),
            &mut diags,
        );
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(text.0.len(), 10, "{text:?}");
        assert_eq!(text.0[0], Segment::Text("Hi ".to_string()));
        assert!(matches!(&text.0[1], Segment::Value(Expr::Var(n, _)) if n == "n"));
        assert_eq!(
            text.0[3],
            Segment::Tag {
                name: "pause".to_string(),
                args: "0.6".to_string(),
                close: false
            }
        );
        assert!(matches!(&text.0[7], Segment::Tag { name, close: true, .. } if name == "shake"));
        let Segment::Alt { kind, items, .. } = &text.0[9] else {
            panic!("{:?}", text.0[9]);
        };
        assert_eq!(*kind, AltKind::Seq);
        assert_eq!(
            items.iter().map(Text::plain).collect::<Vec<_>>(),
            ["A", "B", "C"]
        );
        assert_eq!(text.plain(), "Hi  there. Red A");
    }

    #[test]
    fn reports_unterminated_tags() {
        let mut diags = Vec::new();
        let text = parse_text(
            "Oops {pause",
            Span {
                col: 5,
                ..Span::default()
            },
            &mut diags,
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].span.col, 10);
        assert_eq!(text.plain(), "Oops {pause");
        let mut diags = Vec::new();
        parse_text("${1 +}", Span::default(), &mut diags);
        assert!(diags[0].message.starts_with("bad expression"));
    }
}

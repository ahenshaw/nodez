//! A tiny YAML emitter for [`nodez::Value`].
//!
//! The point of the demo is that the node graph produces a *domain-specific*
//! config file. `nodez::Value` is already an ordered document tree, so the
//! generator builds one of those and this module spells it out.

use nodez::Value;

/// Render a value as a YAML document.
pub fn to_yaml(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0, false);
    out
}

fn write_value(out: &mut String, value: &Value, indent: usize, inline_start: bool) {
    match value {
        Value::Map(entries) if !entries.is_empty() => {
            for (i, (key, child)) in entries.iter().enumerate() {
                if i > 0 || !inline_start {
                    push_indent(out, indent);
                }
                out.push_str(&quote_key(key));
                out.push(':');
                write_child(out, child, indent);
            }
        }
        Value::List(items) if !items.is_empty() => {
            for (i, item) in items.iter().enumerate() {
                if i > 0 || !inline_start {
                    push_indent(out, indent);
                }
                out.push_str("- ");
                match item {
                    Value::Map(inner) if !inner.is_empty() => {
                        write_value(out, &Value::Map(inner.clone()), indent + 1, true);
                    }
                    Value::List(_) => {
                        out.push('\n');
                        write_value(out, item, indent + 1, false);
                    }
                    scalar => {
                        out.push_str(&scalar_text(scalar));
                        out.push('\n');
                    }
                }
            }
        }
        Value::Map(_) => out.push_str("{}\n"),
        Value::List(_) => out.push_str("[]\n"),
        scalar => {
            if !inline_start {
                push_indent(out, indent);
            }
            out.push_str(&scalar_text(scalar));
            out.push('\n');
        }
    }
}

/// Write the value of a `key:` pair, choosing inline or block form.
fn write_child(out: &mut String, child: &Value, indent: usize) {
    match child {
        Value::Map(entries) if !entries.is_empty() => {
            out.push('\n');
            write_value(out, child, indent + 1, false);
        }
        Value::List(items) if !items.is_empty() => {
            out.push('\n');
            write_value(out, child, indent + 1, false);
        }
        Value::Map(_) => out.push_str(" {}\n"),
        Value::List(_) => out.push_str(" []\n"),
        scalar => {
            out.push(' ');
            out.push_str(&scalar_text(scalar));
            out.push('\n');
        }
    }
}

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::Null => "~".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Value::Text(s) | Value::Choice(s) => quote_scalar(s),
        other => quote_scalar(&other.to_literal()),
    }
}

fn quote_key(key: &str) -> String {
    quote_scalar(key)
}

/// Quote anything YAML would otherwise reinterpret.
///
/// A bare colon is left alone so `image: nginx:latest` comes out the way a
/// hand-written compose file would. A colon *followed by a space* does start a
/// mapping, and a digits-and-colons string like `8080:80` is read as a
/// sexagesimal number by YAML 1.1 parsers, so both of those are quoted.
fn quote_scalar(s: &str) -> String {
    const SPECIAL: &[char] = &[
        '#', '{', '}', '[', ']', ',', '&', '*', '!', '|', '>', '\'', '"', '%', '@', '`', '\\',
    ];
    let looks_reserved = matches!(
        s.to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~"
    );
    let numeric = s.parse::<f64>().is_ok();
    let sexagesimal =
        s.contains(':') && s.chars().all(|c| c.is_ascii_digit() || c == ':');
    let needs_quotes = s.is_empty()
        || looks_reserved
        || numeric
        || sexagesimal
        || s.starts_with(' ')
        || s.ends_with(' ')
        || s.starts_with('-')
        || s.contains(": ")
        || s.ends_with(':')
        || s.contains(SPECIAL)
        || s.contains('\n');

    if !needs_quotes {
        return s.to_owned();
    }
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, Value)]) -> Value {
        Value::Map(
            entries
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
        )
    }

    #[test]
    fn nested_maps_and_lists() {
        let doc = map(&[
            ("version", Value::Text("3.9".to_owned())),
            (
                "services",
                map(&[(
                    "web",
                    map(&[
                        ("image", Value::Text("nginx:latest".to_owned())),
                        (
                            "ports",
                            Value::List(vec![Value::Text("8080:80".to_owned())]),
                        ),
                    ]),
                )]),
            ),
        ]);

        assert_eq!(
            to_yaml(&doc),
            "version: \"3.9\"\nservices:\n  web:\n    image: nginx:latest\n    ports:\n      - \"8080:80\"\n"
        );
    }

    #[test]
    fn maps_inside_lists_start_on_the_dash() {
        let doc = map(&[(
            "volumes",
            Value::List(vec![map(&[
                ("type", Value::Text("bind".to_owned())),
                ("read_only", Value::Bool(true)),
            ])]),
        )]);
        assert_eq!(
            to_yaml(&doc),
            "volumes:\n  - type: bind\n    read_only: true\n"
        );
    }

    #[test]
    fn ambiguous_scalars_are_quoted() {
        // Reserved words, numbers and sexagesimals would change meaning.
        assert_eq!(scalar_text(&Value::Text("yes".to_owned())), "\"yes\"");
        assert_eq!(scalar_text(&Value::Text("3.9".to_owned())), "\"3.9\"");
        assert_eq!(scalar_text(&Value::Text("8080:80".to_owned())), "\"8080:80\"");
        assert_eq!(
            scalar_text(&Value::Text("key: value".to_owned())),
            "\"key: value\""
        );
        assert_eq!(
            scalar_text(&Value::Text("${DB_PASSWORD}".to_owned())),
            "\"${DB_PASSWORD}\""
        );

        // These read fine bare, and look wrong quoted.
        assert_eq!(scalar_text(&Value::Text("plain".to_owned())), "plain");
        assert_eq!(
            scalar_text(&Value::Text("nginx:latest".to_owned())),
            "nginx:latest"
        );
        assert_eq!(
            scalar_text(&Value::Text("/var/lib/data".to_owned())),
            "/var/lib/data"
        );
    }
}

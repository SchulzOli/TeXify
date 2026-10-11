//! YAML -> JSON values without YAML 1.1 surprises.
//!
//! Only plain decimals become integers: `10:00` stays a time string (not 600)
//! and `01234` stays a postcode (not octal 668). Quoted and block scalars are
//! always strings. Anchors/aliases are supported.

use std::collections::HashMap;

use regex::Regex;
use serde_json::{Map, Number, Value};
use std::sync::LazyLock;
use yaml_rust2::parser::{Event, EventReceiver, Parser};
use yaml_rust2::scanner::TScalarStyle;

use crate::{bail, error::Result};

static INT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-+]?(0|[1-9][0-9_]*)$").unwrap());
static FLOAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[-+]?([0-9][0-9_]*\.[0-9]*|\.[0-9]+)([eE][-+]?[0-9]+)?$").unwrap());

enum Frame {
    Seq(Vec<Value>, usize),
    Map(Map<String, Value>, Option<String>, usize),
}

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    docs: Vec<Value>,
    anchors: HashMap<usize, Value>,
    error: Option<String>,
}

fn resolve_plain(s: &str) -> Value {
    match s {
        "" | "~" | "null" | "Null" | "NULL" => return Value::Null,
        "true" | "True" | "TRUE" => return Value::Bool(true),
        "false" | "False" | "FALSE" => return Value::Bool(false),
        _ => {}
    }
    if INT.is_match(s) {
        if let Ok(i) = s.replace('_', "").parse::<i64>() {
            return Value::Number(i.into());
        }
    }
    if FLOAT.is_match(s) {
        if let Some(n) = s.replace('_', "").parse::<f64>().ok().and_then(Number::from_f64) {
            return Value::Number(n);
        }
    }
    Value::String(s.to_string())
}

impl Builder {
    fn push_value(&mut self, value: Value, anchor: usize) {
        if anchor > 0 {
            self.anchors.insert(anchor, value.clone());
        }
        match self.stack.last_mut() {
            None => self.docs.push(value),
            Some(Frame::Seq(items, _)) => items.push(value),
            Some(Frame::Map(map, key, _)) => match key.take() {
                None => {
                    let k = match value {
                        Value::String(s) => s,
                        Value::Null => "null".into(),
                        other => other.to_string(),
                    };
                    *key = Some(k);
                }
                Some(k) => {
                    map.insert(k, value);
                }
            },
        }
    }
}

impl EventReceiver for Builder {
    fn on_event(&mut self, ev: Event) {
        match ev {
            Event::Scalar(text, style, anchor, _tag) => {
                let value = if style == TScalarStyle::Plain { resolve_plain(&text) } else { Value::String(text) };
                self.push_value(value, anchor);
            }
            Event::SequenceStart(anchor, _) => self.stack.push(Frame::Seq(Vec::new(), anchor)),
            Event::MappingStart(anchor, _) => self.stack.push(Frame::Map(Map::new(), None, anchor)),
            Event::SequenceEnd | Event::MappingEnd => {
                let (value, anchor) = match self.stack.pop() {
                    Some(Frame::Seq(items, a)) => (Value::Array(items), a),
                    Some(Frame::Map(map, _, a)) => (Value::Object(map), a),
                    None => return,
                };
                self.push_value(value, anchor);
            }
            Event::Alias(id) => match self.anchors.get(&id).cloned() {
                Some(v) => self.push_value(v, 0),
                None => self.error = Some(format!("unknown alias #{id}")),
            },
            _ => {}
        }
    }
}

/// Parse a YAML document into a JSON value (empty document -> `null`).
pub fn load(text: &str) -> Result<Value> {
    let mut builder = Builder::default();
    let mut parser = Parser::new_from_str(text);
    if let Err(e) = parser.load(&mut builder, false) {
        bail!("{e}");
    }
    if let Some(e) = builder.error {
        bail!("{e}");
    }
    Ok(builder.docs.into_iter().next().unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keeps_times_and_postcodes_as_text() {
        let v = load("time: 10:00\nzip: 01234\nn: 42\nf: 1.5\nq: '7'\nb: true\nl: [a, 2]").unwrap();
        assert_eq!(v, json!({"time": "10:00", "zip": "01234", "n": 42, "f": 1.5, "q": "7", "b": true, "l": ["a", 2]}));
    }

    #[test]
    fn block_scalars_and_aliases() {
        let v = load("a: &x |\n  line 1\n  line 2\nb: *x").unwrap();
        assert_eq!(v["a"], json!("line 1\nline 2\n"));
        assert_eq!(v["b"], v["a"]);
    }
}

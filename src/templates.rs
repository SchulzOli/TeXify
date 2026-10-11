//! Template discovery, metadata and the Jinja environment.
//!
//! A template is a folder:
//!
//! ```text
//! <name>/
//!   template.tex    Jinja template with LaTeX-friendly delimiters (see guide.md)
//!   template.yaml   description, fields, defaults, heading map, i18n strings
//!   example.md      a complete example document (used by `texify new`)
//!   ...             any other file (logo, .sty, images) is copied into the build
//! ```
//!
//! Search order (later wins): built-in (embedded in the binary) ->
//! `~/.texify/templates` -> project `templates/`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;

use chrono::Duration;
use include_dir::{Dir, include_dir};
use minijinja::syntax::SyntaxConfig;
use minijinja::value::{Serde, Value, ValueKind};
use minijinja::{Environment, ErrorKind, UndefinedBehavior};
use regex::Regex;
use rust_decimal::Decimal;
use serde_json::Value as Json;

use crate::config::{Config, deep_merge, texify_home};
use crate::error::Result;
use crate::latex::{Lang, color_hex, escape, format_date, format_money, format_number, to_date, to_decimal};
use crate::markdown::{AssetFn, Renderer};
use crate::{bail, err, yaml};

static BUILTIN: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/assets/templates");

pub const TEMPLATE_FILE: &str = "template.tex";
pub const META_FILE: &str = "template.yaml";
pub const EXAMPLE_FILE: &str = "example.md";
const META_KEYS: [&str; 6] = ["description", "fields", "defaults", "headings", "strings", "title_from_heading"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Builtin,
    User,
    Project,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Builtin => "builtin",
            Origin::User => "user",
            Origin::Project => "project",
        }
    }
}

#[derive(Debug, Clone)]
enum Source {
    Embedded(&'static Dir<'static>),
    Disk(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct Template {
    pub name: String,
    pub origin: Origin,
    source: Source,
    pub meta: Json,
}

impl Template {
    /// Where the template lives (folder path, or `builtin:<name>`).
    pub fn location(&self) -> String {
        match &self.source {
            Source::Embedded(_) => format!("builtin:{}", self.name),
            Source::Disk(p) => p.display().to_string(),
        }
    }

    /// All files of the template folder: (file name, bytes).
    pub fn files(&self) -> Vec<(String, Vec<u8>)> {
        match &self.source {
            Source::Embedded(dir) => dir.files().map(|f| (file_name(f.path()), f.contents().to_vec())).collect(),
            Source::Disk(dir) => std::fs::read_dir(dir)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter(|e| e.path().is_file())
                        .filter_map(|e| {
                            Some((e.file_name().to_string_lossy().into_owned(), std::fs::read(e.path()).ok()?))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        }
    }

    pub fn read(&self, file: &str) -> Option<String> {
        match &self.source {
            Source::Embedded(dir) => {
                dir.files().find(|f| file_name(f.path()) == file).and_then(|f| f.contents_utf8()).map(String::from)
            }
            Source::Disk(dir) => std::fs::read_to_string(dir.join(file)).ok(),
        }
    }

    pub fn description(&self) -> String {
        self.meta["description"].as_str().unwrap_or_default().trim().to_string()
    }

    pub fn fields(&self) -> Vec<Field> {
        let Some(map) = self.meta["fields"].as_object() else { return Vec::new() };
        map.iter()
            .map(|(name, spec)| match spec {
                Json::String(d) => Field { name: name.clone(), description: d.clone(), required: false },
                Json::Object(o) => Field {
                    name: name.clone(),
                    description: o.get("description").and_then(Json::as_str).unwrap_or_default().to_string(),
                    required: o.get("required").and_then(Json::as_bool).unwrap_or(false),
                },
                _ => Field { name: name.clone(), description: String::new(), required: false },
            })
            .collect()
    }

    pub fn defaults(&self) -> Json {
        self.meta.get("defaults").cloned().filter(Json::is_object).unwrap_or_else(|| Json::Object(Default::default()))
    }

    pub fn headings(&self) -> Option<&Json> {
        self.meta.get("headings").filter(|h| h.is_object())
    }

    pub fn strings(&self, lang: Lang) -> Json {
        let s = &self.meta["strings"];
        deep_merge(&s["en"], &s[lang.code()])
    }

    pub fn title_from_heading(&self) -> bool {
        self.meta["title_from_heading"].as_bool().unwrap_or(false)
    }

    pub fn example(&self) -> String {
        self.read(EXAMPLE_FILE).unwrap_or_else(|| format!("---\ntemplate: {}\n---\n", self.name))
    }

    pub fn source(&self) -> String {
        self.read(TEMPLATE_FILE).unwrap_or_default()
    }

    pub fn meta_source(&self) -> String {
        self.read(META_FILE).unwrap_or_default()
    }

    /// Files copied into every build (everything except the template files).
    pub fn asset_files(&self) -> Vec<(String, Vec<u8>)> {
        self.files()
            .into_iter()
            .filter(|(n, _)| ![TEMPLATE_FILE, META_FILE, EXAMPLE_FILE].contains(&n.as_str()))
            .collect()
    }

    pub fn missing_fields(&self, ctx: &Json) -> Vec<Field> {
        self.fields()
            .into_iter()
            .filter(|f| f.required)
            .filter(|f| {
                let v = f.name.split('.').fold(ctx, |cur, part| &cur[part]);
                match v {
                    Json::Null => true,
                    Json::String(s) => s.is_empty(),
                    Json::Array(a) => a.is_empty(),
                    Json::Object(o) => o.is_empty(),
                    _ => false,
                }
            })
            .collect()
    }

    /// Write all files of this template into `dest` (for `texify templates eject`).
    pub fn write_to(&self, dest: &Path) -> Result<()> {
        std::fs::create_dir_all(dest)?;
        for (name, bytes) in self.files() {
            std::fs::write(dest.join(name), bytes)?;
        }
        Ok(())
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Shared files (`texify.sty`, `texify-letter.sty`) copied into every build.
pub fn shared_files() -> Vec<(String, Vec<u8>)> {
    BUILTIN
        .get_dir("_shared")
        .map(|d| d.files().map(|f| (file_name(f.path()), f.contents().to_vec())).collect())
        .unwrap_or_default()
}

pub fn shared_file(name: &str) -> Option<String> {
    BUILTIN.get_dir("_shared")?.files().find(|f| file_name(f.path()) == name)?.contents_utf8().map(String::from)
}

/// Parse and validate a template.yaml.
pub fn parse_meta(text: &str, origin: &str) -> Result<Json> {
    let meta = yaml::load(text).map_err(|e| err!("{origin}: invalid YAML: {e}"))?;
    let meta = if meta.is_null() { Json::Object(Default::default()) } else { meta };
    let Some(obj) = meta.as_object() else { bail!("{origin}: must be a YAML mapping") };
    let unknown: Vec<&str> = obj.keys().map(String::as_str).filter(|k| !META_KEYS.contains(k)).collect();
    if !unknown.is_empty() {
        bail!("{origin}: unknown keys: {}", unknown.join(", "));
    }
    Ok(meta)
}

fn disk_template(dir: &Path, origin: Origin) -> Result<Template> {
    let meta_path = dir.join(META_FILE);
    let meta = match std::fs::read_to_string(&meta_path) {
        Ok(text) => parse_meta(&text, &meta_path.display().to_string())?,
        Err(_) => Json::Object(Default::default()),
    };
    Ok(Template { name: file_name(dir), origin, source: Source::Disk(dir.to_path_buf()), meta })
}

/// All templates, later sources overriding earlier ones.
pub fn discover(cfg: &Config) -> Result<BTreeMap<String, Template>> {
    let mut found = BTreeMap::new();
    for dir in BUILTIN.dirs() {
        let name = file_name(dir.path());
        if name.starts_with('_') || dir.get_file(dir.path().join(TEMPLATE_FILE)).is_none() {
            continue;
        }
        let meta = match dir.get_file(dir.path().join(META_FILE)).and_then(|f| f.contents_utf8()) {
            Some(text) => parse_meta(text, &format!("builtin:{name}"))?,
            None => Json::Object(Default::default()),
        };
        found.insert(name.clone(), Template { name, origin: Origin::Builtin, source: Source::Embedded(dir), meta });
    }
    let dirs = cfg.template_dirs();
    let (user_dir, project_dirs) = dirs.split_last().expect("user dir is always present");
    let mut roots = vec![(user_dir.clone(), Origin::User)];
    roots.extend(project_dirs.iter().map(|d| (d.clone(), Origin::Project)));
    for (root, origin) in roots {
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for d in entries {
            let name = file_name(&d);
            if d.is_dir() && !name.starts_with(['_', '.']) && d.join(TEMPLATE_FILE).is_file() {
                found.insert(name, disk_template(&d, origin)?);
            }
        }
    }
    Ok(found)
}

pub fn get_template(name: &str, cfg: &Config) -> Result<Template> {
    let mut all = discover(cfg)?;
    match all.remove(name) {
        Some(t) => Ok(t),
        None => bail!(
            "Template '{name}' not found. Available: {}\nDesign a new one with AI: texify ai template {name} \"<description>\"",
            all.keys().cloned().collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Folder for `~/.texify/templates`.
pub fn user_template_dir() -> PathBuf {
    texify_home().join("templates")
}

// --------------------------------------------------------------------------
// Template helpers available inside templates
// --------------------------------------------------------------------------

fn value_str(v: &Value) -> String {
    match v.kind() {
        ValueKind::Undefined | ValueKind::None => String::new(),
        ValueKind::String => v.as_str().unwrap_or_default().to_string(),
        _ => v.to_string(),
    }
}

fn decimal(v: &Value, what: &str) -> Result<Decimal> {
    to_decimal(&value_str(v), what)
}

fn dec_value(d: Decimal) -> Value {
    Value::from(d.to_string())
}

/// Line totals, net, VAT and gross with exact decimal arithmetic.
pub fn invoice_totals(items: &Value, vat: Option<&Value>, small_business: bool) -> Result<Value> {
    let Ok(iter) = items.try_iter() else { bail!("An invoice needs at least one line item under 'items'") };
    let mut rows = Vec::new();
    let mut net = Decimal::ZERO;
    for (i, it) in iter.enumerate() {
        let i = i + 1;
        let price = it.get_attr("price").unwrap_or(Value::UNDEFINED);
        if it.kind() != ValueKind::Map || price.is_undefined() {
            bail!("Item {i}: needs at least 'desc' and 'price'");
        }
        let qty_v = it.get_attr("qty").unwrap_or(Value::UNDEFINED);
        let qty = if qty_v.is_undefined() { Decimal::ONE } else { decimal(&qty_v, &format!("quantity in item {i}"))? };
        let price = decimal(&price, &format!("price in item {i}"))?;
        let total = (qty * price).round_dp(2);
        net += total;
        let mut row: BTreeMap<String, Value> = BTreeMap::new();
        if let Ok(keys) = it.try_iter() {
            for k in keys {
                let key = value_str(&k);
                row.insert(key.clone(), it.get_attr(&key).unwrap_or(Value::UNDEFINED));
            }
        }
        row.insert("qty".into(), dec_value(qty));
        row.insert("price".into(), dec_value(price));
        row.insert("total".into(), dec_value(total));
        rows.push(Value::from(row));
    }
    if rows.is_empty() {
        bail!("An invoice needs at least one line item under 'items'");
    }
    let rate = if small_business {
        Decimal::ZERO
    } else {
        match vat {
            Some(v) if !v.is_undefined() && !v.is_none() => decimal(v, "VAT rate")?,
            _ => Decimal::from(19),
        }
    };
    let vat_amount = (net * rate / Decimal::from(100)).round_dp(2);
    let mut out: BTreeMap<String, Value> = BTreeMap::new();
    out.insert("items".into(), Value::from(rows));
    out.insert("net".into(), dec_value(net));
    out.insert("vat_rate".into(), dec_value(rate));
    out.insert("vat".into(), dec_value(vat_amount));
    out.insert("gross".into(), dec_value(net + vat_amount));
    Ok(Value::from(out))
}

static COMMAND_BEFORE_VALUE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(\\[A-Za-z@]+)(\\(?:VAR|BLOCK)\{)").unwrap());

/// Insert `{}` between a LaTeX command and a following `\VAR{`/`\BLOCK{`.
/// Without it `\selectfont\VAR{name}` would render as `\selectfontAlex`.
pub fn separate_commands(source: &str) -> String {
    COMMAND_BEFORE_VALUE.replace_all(source, "$1{}$2").into_owned()
}

/// Jinja semantics for `trim_blocks` with line statements: the newline that
/// directly follows a `%:` line is removed, i.e. one empty line after it
/// disappears. minijinja does not do this, so the source is adjusted up front
/// (blank lines matter in LaTeX, e.g. inside tables).
fn trim_after_line_statements(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut after_statement = false;
    for line in source.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        if after_statement && content.is_empty() {
            after_statement = false;
            continue;
        }
        after_statement = content.trim_start().starts_with("%:");
        out.push_str(line);
    }
    out
}

/// Source transformations applied to every template file before parsing.
pub fn preprocess(source: &str) -> String {
    separate_commands(&trim_after_line_statements(source))
}

fn syntax() -> SyntaxConfig {
    SyntaxConfig::builder()
        .block_delimiters("\\BLOCK{", "}")
        .variable_delimiters("\\VAR{", "}")
        .comment_delimiters("\\#{", "}")
        .line_statement_prefix("%:")
        .line_comment_prefix("%#")
        .trim_blocks(true)
        .lstrip_blocks(true)
        .keep_trailing_newline(true)
        .build()
        .expect("valid TeXify template syntax")
}

fn mj_err(e: impl std::fmt::Display) -> minijinja::Error {
    minijinja::Error::new(ErrorKind::InvalidOperation, e.to_string())
}

/// Build the Jinja environment for one render.
pub fn environment(
    template: &Template,
    lang: Lang,
    currency: String,
    renderer: Renderer,
    asset: AssetFn,
) -> Environment<'static> {
    let mut env = Environment::new();
    env.set_syntax(syntax());
    env.set_undefined_behavior(UndefinedBehavior::Chainable);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);

    let files: BTreeMap<String, String> =
        template.files().into_iter().filter_map(|(n, b)| Some((n, String::from_utf8(b).ok()?))).collect();
    env.set_loader(move |name| {
        let src = files.get(name).cloned().or_else(|| shared_file(name));
        Ok(src.map(|s| preprocess(&s)))
    });

    env.set_formatter(|out, _state, value| {
        let text = if value.is_safe() {
            value.to_string()
        } else {
            match value.kind() {
                ValueKind::Undefined | ValueKind::None => String::new(),
                ValueKind::Bool => {
                    if value.is_true() {
                        "true".into()
                    } else {
                        "false".into()
                    }
                }
                ValueKind::Number => value.to_string(),
                ValueKind::String => escape(value.as_str().unwrap_or_default()),
                ValueKind::Seq => {
                    let items: Vec<String> =
                        value.try_iter().map(|it| it.map(|v| value_str(&v)).collect()).unwrap_or_default();
                    escape(&items.join(", "))
                }
                _ => escape(&value.to_string()),
            }
        };
        out.write_str(&text).map_err(|_| minijinja::Error::new(ErrorKind::WriteFailure, "write failed"))
    });

    // ---- filters ----
    let r = renderer.clone();
    env.add_filter("md", move |v: Value| -> std::result::Result<Value, minijinja::Error> {
        r.render_inline(&value_str(&v)).map(Value::from_safe_string).map_err(mj_err)
    });
    let r = renderer.clone();
    env.add_filter("mdblock", move |v: Value| -> std::result::Result<Value, minijinja::Error> {
        r.render(&value_str(&v)).map(Value::from_safe_string).map_err(mj_err)
    });
    env.add_filter("tex", |v: Value| Value::from_safe_string(value_str(&v)));
    env.add_filter("lines", |v: Value, sep: Option<String>| {
        let sep = sep.unwrap_or_else(|| "\\\\".into());
        let items: Vec<String> = match v.kind() {
            ValueKind::Seq => v.try_iter().map(|it| it.map(|x| value_str(&x)).collect()).unwrap_or_default(),
            _ => value_str(&v).lines().map(String::from).collect(),
        };
        let parts: Vec<String> = items.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).map(escape).collect();
        Value::from_safe_string(parts.join(&sep))
    });
    env.add_filter("texjoin", |v: Value, sep: Option<String>| {
        let sep = escape(&sep.unwrap_or_else(|| ", ".into()));
        let items: Vec<String> =
            v.try_iter().map(|it| it.map(|x| escape(&value_str(&x))).collect()).unwrap_or_default();
        Value::from_safe_string(items.join(&sep))
    });
    env.add_filter("date", move |v: Value, style: Option<String>| {
        Value::from_safe_string(escape(&format_date(&value_str(&v), lang, style.as_deref() == Some("short"))))
    });
    env.add_filter("num", move |v: Value, decimals: Option<u32>| -> std::result::Result<Value, minijinja::Error> {
        let d = decimal(&v, "number").map_err(mj_err)?;
        Ok(Value::from_safe_string(escape(&format_number(d, lang, decimals))))
    });
    env.add_filter("money", move |v: Value, cur: Option<String>| -> std::result::Result<Value, minijinja::Error> {
        let d = decimal(&v, "amount").map_err(mj_err)?;
        Ok(Value::from_safe_string(escape(&format_money(d, lang, cur.as_deref().unwrap_or(&currency)))))
    });
    env.add_filter("color", |v: Value| -> std::result::Result<Value, minijinja::Error> {
        color_hex(&value_str(&v)).map(Value::from).map_err(mj_err)
    });
    let a = asset.clone();
    env.add_filter("asset", move |v: Value| -> std::result::Result<Value, minijinja::Error> {
        a(&value_str(&v)).map(Value::from_safe_string).map_err(mj_err)
    });

    // ---- functions ----
    env.add_function(
        "invoice_totals",
        |items: Value,
         vat: Option<Value>,
         small_business: Option<bool>|
         -> std::result::Result<Value, minijinja::Error> {
            invoice_totals(&items, vat.as_ref(), small_business.unwrap_or(false)).map_err(mj_err)
        },
    );
    env.add_function(
        "sum_amounts",
        |items: Value, key: Option<String>| -> std::result::Result<Value, minijinja::Error> {
            let key = key.unwrap_or_else(|| "amount".into());
            let mut sum = Decimal::ZERO;
            if let Ok(iter) = items.try_iter() {
                for it in iter {
                    let v = it.get_attr(&key).unwrap_or(Value::UNDEFINED);
                    if !v.is_undefined() {
                        sum += decimal(&v, "amount").map_err(mj_err)?;
                    }
                }
            }
            Ok(dec_value(sum))
        },
    );
    env.add_function("add_days", |date: Value, days: i64| -> std::result::Result<Value, minijinja::Error> {
        let s = value_str(&date);
        let d = to_date(&s).ok_or_else(|| mj_err(format!("Not a valid date: {s:?}")))?;
        Ok(Value::from((d + Duration::days(days)).format("%Y-%m-%d").to_string()))
    });
    env.add_global("today", Value::from(chrono::Local::now().format("%Y-%m-%d").to_string()));
    env
}

/// Convert the JSON context; `body` and the section titles/bodies are already
/// LaTeX and are marked safe so the formatter does not escape them again.
fn context_value(ctx: &Json) -> Value {
    let safe = |v: &Json| Value::from_safe_string(v.as_str().unwrap_or_default().to_string());
    let mut map: BTreeMap<String, Value> = BTreeMap::new();
    for (k, v) in ctx.as_object().into_iter().flatten() {
        let value = match k.as_str() {
            "body" => safe(v),
            "sections" => Value::from(
                v.as_array()
                    .into_iter()
                    .flatten()
                    .map(|s| {
                        let mut m: BTreeMap<String, Value> = BTreeMap::new();
                        m.insert("title".into(), safe(&s["title"]));
                        m.insert("body".into(), safe(&s["body"]));
                        m.insert("raw_title".into(), Value::from(s["raw_title"].as_str().unwrap_or_default()));
                        m.insert("slug".into(), Value::from(s["slug"].as_str().unwrap_or_default()));
                        Value::from(m)
                    })
                    .collect::<Vec<_>>(),
            ),
            _ => Value::from(Serde(v)),
        };
        map.insert(k.clone(), value);
    }
    Value::from(map)
}

/// Render `template.tex` with the given context.
pub fn render(env: &Environment<'static>, template: &Template, ctx: &Json) -> Result<String> {
    let tpl = env.get_template(TEMPLATE_FILE).map_err(|e| err!("Template '{}': {}", template.name, describe(&e)))?;
    tpl.render(context_value(ctx)).map_err(|e| err!("Template '{}': {}", template.name, describe(&e)))
}

fn describe(e: &minijinja::Error) -> String {
    let mut msg = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        msg.push_str(&format!(": {s}"));
        source = s.source();
    }
    msg
}

/// Parse a number from a frontmatter value (used by callers outside templates).
pub fn json_decimal(v: &Json) -> Option<Decimal> {
    match v {
        Json::Number(n) => Decimal::from_str(&n.to_string()).ok(),
        Json::String(s) => to_decimal(s, "number").ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_embedded() {
        let cfg = Config::load(Path::new(".")).unwrap();
        let all = discover(&cfg).unwrap();
        for name in [
            "article",
            "letter",
            "invoice",
            "cv",
            "cv-cards",
            "proposal",
            "confirmation",
            "minutes",
            "invitation",
            "certificate",
            "coverpage",
            "contract",
        ] {
            assert!(all.contains_key(name), "{name} missing");
        }
        assert!(shared_file("texify.sty").unwrap().contains("ProvidesPackage{texify}"));
    }

    #[test]
    fn separates_commands() {
        assert_eq!(separate_commands(r"\noindent\VAR{x}\\\VAR{y}"), r"\noindent{}\VAR{x}\\\VAR{y}");
    }

    #[test]
    fn invoice_math_is_exact() {
        let items = Value::from(Serde(
            serde_json::json!([{"desc": "a", "qty": 3, "price": "0,10"}, {"desc": "b", "price": 0.2}]),
        ));
        let t = invoice_totals(&items, Some(&Value::from(19)), false).unwrap();
        assert_eq!(t.get_attr("net").unwrap().to_string(), "0.50");
        assert_eq!(t.get_attr("vat").unwrap().to_string(), "0.10");
        assert_eq!(t.get_attr("gross").unwrap().to_string(), "0.60");
    }
}

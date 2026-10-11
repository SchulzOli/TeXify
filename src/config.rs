//! Configuration: `texify.toml` (project) + `~/.texify/config.toml` (user).
//!
//! Precedence (low -> high): built-in < user config < project config.
//! `[defaults]` values are merged into every document's frontmatter, so sender
//! address, bank details, language or accent colour are written down only once.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::{bail, error::Result};

pub const CONFIG_NAME: &str = "texify.toml";

fn builtin() -> Value {
    json!({
        // lang/font/currency fall back to en/sans/EUR at render time, so that
        // template defaults (e.g. a serif contract) are not overridden here
        "defaults": {},
        "build": {"engine": "auto", "out_dir": "", "docker_image": "texify:latest"},
        "templates": {"paths": ["templates"]},
        // AI is provider-agnostic: any command that reads a prompt on stdin and
        // prints the answer on stdout. Empty = AI only through the MCP server.
        "ai": {"command": "", "attempts": 3, "timeout": 900},
    })
}

/// `$TEXIFY_HOME` or `~/.texify`.
pub fn texify_home() -> PathBuf {
    if let Ok(h) = std::env::var("TEXIFY_HOME") {
        return PathBuf::from(h);
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).unwrap_or_default();
    PathBuf::from(home).join(".texify")
}

/// Recursively merge `over` into a copy of `base` (objects merge, everything else replaces).
pub fn deep_merge(base: &Value, over: &Value) -> Value {
    match (base, over) {
        (Value::Object(a), Value::Object(b)) => {
            let mut out = a.clone();
            for (k, v) in b {
                let merged = match out.get(k) {
                    Some(existing @ Value::Object(_)) if v.is_object() => deep_merge(existing, v),
                    _ => v.clone(),
                };
                out.insert(k.clone(), merged);
            }
            Value::Object(out)
        }
        (_, over) => over.clone(),
    }
}

pub fn find_project_config(start: &Path) -> Option<PathBuf> {
    let start = std::path::absolute(start).ok()?;
    let folder = if start.is_dir() { start } else { start.parent()?.to_path_buf() };
    folder.ancestors().map(|d| d.join(CONFIG_NAME)).find(|c| c.is_file())
}

fn load_toml(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path)?;
    match toml::from_str::<toml::Value>(&text) {
        Ok(v) => Ok(serde_json::to_value(v).unwrap_or(Value::Null)),
        Err(e) => bail!("{}: invalid TOML: {e}", path.display()),
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub data: Value,
    pub project_file: Option<PathBuf>,
    pub user_file: Option<PathBuf>,
}

impl Config {
    /// Load config for a document/folder (searches texify.toml upwards).
    pub fn load(start: &Path) -> Result<Config> {
        let mut data = builtin();
        let user = texify_home().join("config.toml");
        let user_file = if user.is_file() {
            data = deep_merge(&data, &load_toml(&user)?);
            Some(user)
        } else {
            None
        };
        let project_file = find_project_config(start);
        if let Some(p) = &project_file {
            data = deep_merge(&data, &load_toml(p)?);
        }
        Ok(Config { data, project_file, user_file })
    }

    pub fn load_cwd() -> Result<Config> {
        Config::load(&std::env::current_dir()?)
    }

    /// Project root: folder of texify.toml, else the current directory.
    pub fn root(&self) -> PathBuf {
        match &self.project_file {
            Some(p) => p.parent().map(Path::to_path_buf).unwrap_or_default(),
            None => std::env::current_dir().unwrap_or_default(),
        }
    }

    pub fn defaults(&self) -> &Value {
        &self.data["defaults"]
    }

    pub fn str(&self, section: &str, key: &str) -> String {
        self.data[section][key].as_str().unwrap_or_default().to_string()
    }

    pub fn int(&self, section: &str, key: &str, default: i64) -> i64 {
        self.data[section][key].as_i64().unwrap_or(default)
    }

    /// Search order for templates: project paths, then the user dir (built-ins come first).
    pub fn template_dirs(&self) -> Vec<PathBuf> {
        let root = self.root();
        let mut dirs: Vec<PathBuf> = self.data["templates"]["paths"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(|p| root.join(p)).collect())
            .unwrap_or_default();
        dirs.push(texify_home().join("templates"));
        dirs
    }

    /// Copy with an extra template folder (used to test-build new templates).
    pub fn with_template_dir(&self, dir: &Path) -> Config {
        let mut new = self.clone();
        let paths = new.data["templates"]["paths"].as_array_mut();
        if let Some(paths) = paths {
            paths.push(Value::String(dir.to_string_lossy().into_owned()));
        }
        new
    }
}

pub const INIT_TEMPLATE: &str = r#"# TeXify project configuration
# Everything under [defaults] pre-fills EVERY document in this folder (and its
# subfolders). A document's frontmatter overrides these values.

[defaults]
lang = "en"            # en | de
accent = "1F4E79"      # accent colour (hex)
# font = "sans"        # sans | serif – otherwise the template decides (contract: serif)
# currency = "EUR"

[defaults.sender]      # sender for letters, invoices, confirmations
name = "Alex Example"
street = "1 Sample Street"
zip = "12345"
city = "Sampletown"
phone = "+49 170 1234567"
email = "alex@example.com"
# web = "example.com"
# tax_id = "123/456/78901"        # tax number (invoices)
# vat_id = "DE123456789"          # VAT ID (invoices)

# [defaults.sender.bank]
# name = "Example Bank"
# iban = "DE00 0000 0000 0000 0000 00"
# bic = "XXXXDEXXXXX"

[build]
engine = "auto"        # auto | tectonic | latexmk | pdflatex | xelatex | lualatex | docker
out_dir = ""           # empty = PDF next to the source file

[templates]
paths = ["templates"]  # your own templates (override built-ins with the same name)

[ai]
# Optional, for `texify ai ...` on the command line. Any LLM tool that reads the
# prompt on stdin and prints the answer, e.g. "llm -m gpt-4o", "ollama run
# llama3.1", "claude -p". Leave empty to use AI only through the MCP server.
command = ""
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_nested() {
        let a = json!({"x": {"a": 1, "b": 2}, "y": 1});
        let b = json!({"x": {"b": 3}, "z": 4});
        assert_eq!(deep_merge(&a, &b), json!({"x": {"a": 1, "b": 3}, "y": 1, "z": 4}));
    }
}

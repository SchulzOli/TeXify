//! Pipeline: document (Markdown/Text/YAML + frontmatter) -> LaTeX -> PDF.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use regex::Regex;
use serde_json::{Map, Value};

use crate::config::{Config, deep_merge};
use crate::engine;
use crate::error::Result;
use crate::latex::{Lang, color_hex};
use crate::markdown::{AssetFn, Renderer};
use crate::templates::{self, Template};
use crate::{bail, err, yaml};

pub const SOURCE_SUFFIXES: [&str; 5] = ["md", "markdown", "txt", "yaml", "yml"];
pub const DEFAULT_TEMPLATE: &str = "article";

static FRONTMATTER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\A\x{feff}?---[ \t]*\r?\n(.*?)\r?\n(?:---|\.\.\.)[ \t]*(?:\r?\n|\z)").unwrap());

/// A source document: frontmatter + Markdown body.
#[derive(Debug, Clone)]
pub struct Document {
    pub meta: Value,
    pub body: String,
    pub path: Option<PathBuf>,
}

impl Document {
    pub fn base_dir(&self) -> PathBuf {
        match &self.path {
            Some(p) => p.parent().map(Path::to_path_buf).unwrap_or_default(),
            None => std::env::current_dir().unwrap_or_default(),
        }
    }

    pub fn stem(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".into())
    }
}

fn empty_map() -> Value {
    Value::Object(Map::new())
}

pub fn parse_document(text: &str, path: Option<&Path>) -> Result<Document> {
    let Some(m) = FRONTMATTER.captures(text) else {
        return Ok(Document { meta: empty_map(), body: text.to_string(), path: path.map(Path::to_path_buf) });
    };
    let where_ = path.map(|p| format!("{}: ", p.display())).unwrap_or_default();
    let meta = yaml::load(&m[1]).map_err(|e| err!("{where_}frontmatter is not valid YAML: {e}"))?;
    let meta = match meta {
        Value::Null => empty_map(),
        v @ Value::Object(_) => v,
        _ => bail!("{where_}frontmatter must be a YAML mapping"),
    };
    let end = m.get(0).unwrap().end();
    Ok(Document { meta, body: text[end..].to_string(), path: path.map(Path::to_path_buf) })
}

pub fn load_document(path: &Path) -> Result<Document> {
    if !path.is_file() {
        bail!("File not found: {}", path.display());
    }
    let text = std::fs::read_to_string(path)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let ext = ext_of(path);
    if ext == "yaml" || ext == "yml" {
        let mut meta = yaml::load(text).map_err(|e| err!("{}: invalid YAML: {e}", path.display()))?;
        let body = meta.as_object_mut().and_then(|m| m.remove("body")).and_then(|b| b.as_str().map(String::from));
        if !meta.is_object() {
            meta = empty_map();
        }
        return Ok(Document { meta, body: body.unwrap_or_default(), path: Some(path.to_path_buf()) });
    }
    parse_document(text, Some(path))
}

pub fn ext_of(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

// --------------------------------------------------------------------------
// Assets (photos, logos, signatures, Markdown images)
// --------------------------------------------------------------------------

struct Assets {
    search: Vec<PathBuf>,
    build_dir: PathBuf,
    copied: Mutex<HashMap<PathBuf, String>>,
}

impl Assets {
    fn resolve(&self, src: &str) -> Result<String> {
        if Regex::new(r"(?i)^[a-z][a-z0-9+.-]*://").unwrap().is_match(src) {
            bail!("Only local files can be embedded, not URLs: {src}");
        }
        let found = self.search.iter().map(|b| b.join(src)).find(|p| p.is_file());
        let Some(p) = found else {
            let dirs: Vec<String> = self.search.iter().map(|d| d.display().to_string()).collect();
            bail!("File not found: {src} (searched: {})", dirs.join(", "));
        };
        let p = std::path::absolute(&p)?;
        let mut copied = self.copied.lock().unwrap();
        if let Some(name) = copied.get(&p) {
            return Ok(name.clone());
        }
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let safe: String = Regex::new(r"[^a-z0-9-]+").unwrap().replace_all(&stem, "-").trim_matches('-').to_string();
        let safe = if safe.is_empty() { "file".into() } else { safe };
        let name = format!("assets/a{}-{safe}.{}", copied.len() + 1, ext_of(&p));
        std::fs::create_dir_all(self.build_dir.join("assets"))?;
        std::fs::copy(&p, self.build_dir.join(&name))?;
        copied.insert(p, name.clone());
        Ok(name)
    }
}

// --------------------------------------------------------------------------
// Rendering
// --------------------------------------------------------------------------

fn build_context(doc: &Document, template: &Template, cfg: &Config, renderer: &Renderer, lang: Lang) -> Result<Value> {
    let mut ctx = deep_merge(&deep_merge(&template.defaults(), cfg.defaults()), &doc.meta);
    let mut body = doc.body.clone();
    if template.title_from_heading() && ctx["title"].as_str().is_none_or(str::is_empty) {
        let (title, rest) = renderer.extract_title(&body);
        if let Some(t) = title {
            ctx["title"] = Value::String(t);
            body = rest;
        }
    }
    let sections = renderer.sections(&body)?;
    ctx["body"] = Value::String(renderer.render(&body)?);
    ctx["sections"] = serde_json::to_value(sections).unwrap_or_default();
    ctx["markdown"] = Value::String(body);
    ctx["txt"] = template.strings(lang);
    ctx["template"] = Value::String(template.name.clone());
    Ok(ctx)
}

/// Render the document into `build_dir` (tex + shared style + assets).
pub fn render_tex(
    doc: &Document,
    cfg: &Config,
    build_dir: &Path,
    template_name: Option<&str>,
) -> Result<(PathBuf, Template)> {
    let name = template_name
        .map(String::from)
        .or_else(|| doc.meta["template"].as_str().map(String::from))
        .or_else(|| cfg.defaults()["template"].as_str().map(String::from))
        .unwrap_or_else(|| DEFAULT_TEMPLATE.into());
    let template = templates::get_template(&name, cfg)?;

    let merged = deep_merge(&deep_merge(&template.defaults(), cfg.defaults()), &doc.meta);
    let lang = Lang::parse(merged["lang"].as_str())?;
    let font = merged["font"].as_str().unwrap_or("sans").to_lowercase();
    if font != "sans" && font != "serif" {
        bail!("font must be 'sans' or 'serif', not {font:?}");
    }
    let accent = match &merged["accent"] {
        Value::Null => "1F4E79".to_string(),
        Value::String(s) => color_hex(s)?,
        other => color_hex(&other.to_string())?,
    };
    let mut doc = doc.clone();
    doc.meta["lang"] = Value::String(lang.code().into());
    doc.meta["font"] = Value::String(font);
    doc.meta["accent"] = Value::String(accent);

    std::fs::create_dir_all(build_dir)?;
    let assets = Arc::new(Assets {
        search: vec![doc.base_dir(), cfg.root()],
        build_dir: build_dir.to_path_buf(),
        copied: Mutex::new(HashMap::new()),
    });
    let asset: AssetFn = Arc::new(move |src: &str| assets.resolve(src));
    let renderer = Renderer::new(lang, template.headings(), Some(asset.clone()))?;
    let ctx = build_context(&doc, &template, cfg, &renderer, lang)?;

    let missing = template.missing_fields(&ctx);
    if !missing.is_empty() {
        let lines: Vec<String> = missing.iter().map(|f| format!("  • {}: {}", f.name, f.description)).collect();
        bail!(
            "Template '{}' still needs these fields (frontmatter or texify.toml [defaults]):\n{}",
            template.name,
            lines.join("\n")
        );
    }

    for (name, bytes) in templates::shared_files().into_iter().chain(template.asset_files()) {
        std::fs::write(build_dir.join(name), bytes)?;
    }
    let currency = ctx["currency"].as_str().unwrap_or("EUR").to_string();
    let env = templates::environment(&template, lang, currency, renderer, asset);
    let tex = templates::render(&env, &template, &ctx)?;
    let tex_path = build_dir.join(format!("{}.tex", safe_stem(&doc.stem())));
    std::fs::write(&tex_path, tex)?;
    Ok((tex_path, template))
}

fn safe_stem(stem: &str) -> String {
    let s: String = Regex::new(r"[^A-Za-z0-9._-]+").unwrap().replace_all(stem, "_").trim_matches('_').to_string();
    if s.is_empty() { "document".into() } else { s }
}

// --------------------------------------------------------------------------
// Build
// --------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct BuildResult {
    pub source: PathBuf,
    pub pdf: PathBuf,
    pub template: Option<String>,
    pub engine: Option<String>,
    pub tex_dir: Option<PathBuf>,
}

pub fn default_output(source: &Path, cfg: &Config) -> PathBuf {
    let out_dir = cfg.str("build", "out_dir");
    let folder = if out_dir.is_empty() {
        source.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        cfg.root().join(out_dir)
    };
    folder.join(format!("{}.pdf", source.file_stem().unwrap_or_default().to_string_lossy()))
}

#[derive(Default)]
pub struct BuildOptions<'a> {
    pub output: Option<PathBuf>,
    pub cfg: Option<Config>,
    pub engine: Option<&'a str>,
    pub template: Option<&'a str>,
    /// also export the complete, standalone-compilable LaTeX project here
    pub tex_dir: Option<PathBuf>,
}

/// Build one source file into a PDF.
pub fn build(source: &Path, opts: BuildOptions) -> Result<BuildResult> {
    let source = std::path::absolute(source)?;
    let cfg = match opts.cfg {
        Some(c) => c,
        None => Config::load(&source)?,
    };
    let output = std::path::absolute(opts.output.unwrap_or_else(|| default_output(&source, &cfg)))?;
    let ext = ext_of(&source);
    if ext == "pdf" {
        return Ok(BuildResult { pdf: source.clone(), source, template: None, engine: None, tex_dir: None });
    }
    if ext == "tex" {
        let eng = engine::resolve(&cfg, opts.engine)?;
        let pdf = engine::compile(&source, &eng, &cfg)?;
        engine::clean_aux(&source);
        if pdf != output {
            if let Some(p) = output.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::rename(&pdf, &output).or_else(|_| std::fs::copy(&pdf, &output).map(|_| ()))?;
        }
        return Ok(BuildResult { source, pdf: output, template: None, engine: Some(eng), tex_dir: None });
    }
    if !SOURCE_SUFFIXES.contains(&ext.as_str()) {
        bail!(
            "Unsupported file type: {} (allowed: .md .txt .yaml .tex .pdf)",
            source.file_name().unwrap_or_default().to_string_lossy()
        );
    }
    let doc = load_document(&source)?;
    let eng = engine::resolve(&cfg, opts.engine)?;
    let tmp = tempfile::Builder::new().prefix("texify-").tempdir()?;
    let (tex_path, tpl) = render_tex(&doc, &cfg, tmp.path(), opts.template)?;
    if let Some(dir) = &opts.tex_dir {
        copy_dir(tmp.path(), dir)?;
    }
    let pdf = engine::compile(&tex_path, &eng, &cfg)?;
    if let Some(p) = output.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::copy(&pdf, &output)?;
    Ok(BuildResult { source, pdf: output, template: Some(tpl.name), engine: Some(eng), tex_dir: opts.tex_dir })
}

/// Render only (no compile): write a standalone LaTeX project to `target_dir`.
pub fn export_tex(source: &Path, target_dir: &Path, template: Option<&str>) -> Result<PathBuf> {
    let source = std::path::absolute(source)?;
    let cfg = Config::load(&source)?;
    let doc = load_document(&source)?;
    let tmp = tempfile::Builder::new().prefix("texify-").tempdir()?;
    let (tex_path, _) = render_tex(&doc, &cfg, tmp.path(), template)?;
    copy_dir(tmp.path(), target_dir)?;
    Ok(target_dir.join(tex_path.file_name().unwrap()))
}

/// Render a document and return the LaTeX source.
pub fn render_to_string(source: &Path, cfg: Option<Config>, template: Option<&str>) -> Result<String> {
    let source = std::path::absolute(source)?;
    let cfg = match cfg {
        Some(c) => c,
        None => Config::load(&source)?,
    };
    let doc = load_document(&source)?;
    let tmp = tempfile::Builder::new().prefix("texify-").tempdir()?;
    let (tex_path, _) = render_tex(&doc, &cfg, tmp.path(), template)?;
    Ok(std::fs::read_to_string(tex_path)?)
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

//! `texify` command line interface.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, SystemTime};

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::json;

use crate::build::{BuildOptions, SOURCE_SUFFIXES, build, export_tex};
use crate::config::{CONFIG_NAME, Config, INIT_TEMPLATE, texify_home};
use crate::error::{Error, Result};
use crate::templates::{discover, get_template, user_template_dir};
use crate::{VERSION, ai, bail, engine, importers, mcp, pdftools};

#[derive(Parser)]
#[command(
    name = "texify",
    version = VERSION,
    about = "Markdown/Text -> LaTeX -> PDF. Professional documents from plain text files.",
    after_help = "Quick start:  texify new letter letter.md  &&  texify build letter.md"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build document(s) into PDF; several + -o x.pdf = one bundle
    Build(BuildCmd),
    /// Generate LaTeX only (project folder for further design work)
    Tex {
        source: PathBuf,
        /// Target folder (default: <name>-tex/)
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(short, long)]
        template: Option<String>,
    },
    /// Start a document from a template's example
    New {
        template: String,
        /// File name (default: <template>.md)
        file: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Templates: list | show <name> | eject <name> | guide
    Templates {
        #[arg(value_enum, default_value = "list")]
        action: TemplateAction,
        name: Option<String>,
        /// eject: target folder (default: ./templates)
        #[arg(long)]
        to: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
    /// Create texify.toml with sender/design defaults
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Merge PDFs into one
    Merge {
        /// PDF files, folders or glob patterns
        #[arg(required = true)]
        inputs: Vec<String>,
        #[arg(short, long, default_value = "merged.pdf")]
        output: PathBuf,
        #[arg(long, value_enum, default_value = "none")]
        sort: SortMode,
        /// One bookmark per file
        #[arg(long)]
        bookmarks: bool,
        /// Overwrite the target
        #[arg(short, long)]
        force: bool,
    },
    /// Select/reorder pages, e.g. 1-3,5,8-
    Pages {
        source: PathBuf,
        ranges: String,
        #[arg(short, long)]
        output: PathBuf,
    },
    /// Page count/format/metadata of PDFs
    Info {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Convert PDF/DOCX/TXT to Markdown
    Import {
        source: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Pre-set the template in the frontmatter
        #[arg(short, long)]
        template: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Check the environment (engines, AI command, template folders)
    Doctor,
    /// AI assistance via any LLM command: new templates, structure text
    Ai {
        #[command(subcommand)]
        action: AiCmd,
    },
    /// MCP server for any AI client (stdio or HTTP) | config snippets
    Mcp(McpCmd),
}

#[derive(Args)]
struct BuildCmd {
    /// .md/.txt/.yaml/.tex, folders, globs – with -o x.pdf also .pdf
    #[arg(required = true)]
    sources: Vec<String>,
    /// Target PDF (several sources: merged) or folder
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Force a template (instead of the frontmatter)
    #[arg(short, long)]
    template: Option<String>,
    /// Force a LaTeX engine
    #[arg(short, long, value_parser = engine::ENGINES)]
    engine: Option<String>,
    /// Also export the complete LaTeX project to this folder
    #[arg(long)]
    tex_dir: Option<PathBuf>,
    /// One bookmark per part (bundles)
    #[arg(long)]
    bookmarks: bool,
    /// Open the PDF afterwards
    #[arg(long)]
    open: bool,
    /// Rebuild on changes
    #[arg(short, long)]
    watch: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum TemplateAction {
    List,
    Show,
    Eject,
    Guide,
}

#[derive(Clone, Copy, ValueEnum)]
enum SortMode {
    None,
    Name,
    Date,
}

#[derive(Subcommand)]
enum AiCmd {
    /// Design a new template and verify it with a test build
    Template {
        /// Name, e.g. 'donation-receipt'
        name: String,
        /// What the document is for and how it should look
        description: String,
        /// Reference template (default: chosen automatically)
        #[arg(long)]
        base: Option<String>,
        /// Store in ~/.texify/templates instead of ./templates
        #[arg(long)]
        user: bool,
        #[arg(short, long, value_parser = engine::ENGINES)]
        engine: Option<String>,
        #[arg(long)]
        force: bool,
    },
    /// Turn unstructured text into a document for a template
    Fill {
        template: String,
        /// Text, Markdown, PDF or DOCX file
        input: PathBuf,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Args)]
struct McpCmd {
    /// serve (default) or config: print the client configuration
    #[arg(value_enum, default_value = "serve")]
    action: McpAction,
    /// Working folder (default: current directory)
    #[arg(long)]
    workspace: Option<PathBuf>,
    /// Streamable HTTP instead of stdio (one server, many clients)
    #[arg(long)]
    http: bool,
    /// HTTP bind address (Docker: 0.0.0.0)
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    #[arg(long, default_value_t = mcp::DEFAULT_PORT)]
    port: u16,
    /// Additional Host header accepted over HTTP (e.g. a LAN name)
    #[arg(long = "allow-host", value_name = "HOST[:PORT]")]
    allow_host: Vec<String>,
    /// config: run the server in the Docker image
    #[arg(long)]
    docker: bool,
    /// config: URL of a running HTTP server
    #[arg(long)]
    url: Option<String>,
    /// config: json = generic mcpServers, vscode = .vscode/mcp.json, toml/yaml for other clients
    #[arg(long, value_enum, default_value = "json")]
    format: ConfigFormat,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum McpAction {
    Serve,
    Config,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
pub enum ConfigFormat {
    Json,
    Vscode,
    Toml,
    Yaml,
}

fn pages(pdf: &Path) -> usize {
    pdftools::info(pdf).map(|i| i.pages).unwrap_or(0)
}

fn open_file(path: &Path) {
    let p = path.display().to_string();
    let _ = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", "start", "", &p]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&p).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&p).spawn()
    };
}

fn ensure_new(path: &Path, force: bool, flag: &str) -> Result<()> {
    if path.exists() && !force {
        bail!("{} already exists ({flag} to overwrite)", path.display());
    }
    Ok(())
}

// --------------------------------------------------------------------------
// build
// --------------------------------------------------------------------------

fn cmd_build(a: BuildCmd) -> Result<()> {
    let mut exts: Vec<&str> = SOURCE_SUFFIXES.to_vec();
    exts.push("tex");
    if a.output.is_some() {
        exts.push("pdf");
    }
    let sources = pdftools::collect_inputs(&a.sources, &exts)?;
    if sources.is_empty() {
        bail!("No input files found");
    }
    let target = a.output.clone();
    let bundle = target.as_ref().is_some_and(|t| t.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))
        && sources.len() > 1;

    let run_once = || -> Result<Option<PathBuf>> {
        if bundle {
            let target = target.clone().unwrap();
            let tmp = tempfile::Builder::new().prefix("texify-bundle-").tempdir()?;
            let mut parts = Vec::new();
            for (i, src) in sources.iter().enumerate() {
                if src.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
                    parts.push(src.clone());
                    println!("  + {}", src.display());
                    continue;
                }
                let stem = src.file_stem().unwrap_or_default().to_string_lossy();
                let r = build(
                    src,
                    BuildOptions {
                        output: Some(tmp.path().join(format!("{i:03}-{stem}.pdf"))),
                        engine: a.engine.as_deref(),
                        template: a.template.as_deref(),
                        ..Default::default()
                    },
                )?;
                println!("  ✓ {}  [{}]", src.display(), r.template.as_deref().unwrap_or("tex"));
                parts.push(r.pdf);
            }
            let titles: Vec<String> =
                sources.iter().map(|s| s.file_stem().unwrap_or_default().to_string_lossy().into_owned()).collect();
            let report = pdftools::merge(&parts, &target, a.bookmarks, Some(&titles))?;
            let total: usize = report.iter().map(|(_, n)| n).sum();
            println!("Done: {} parts, {total} pages -> {}", report.len(), std::path::absolute(&target)?.display());
            return Ok(Some(target));
        }
        let mut last = None;
        for src in &sources {
            if src.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")) {
                continue;
            }
            let stem = src.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let dest = match &target {
                Some(t) if sources.len() == 1 => Some(t.clone()),
                Some(t) => Some(t.join(format!("{stem}.pdf"))),
                None => None,
            };
            let tex_dir = a.tex_dir.as_ref().map(|d| if sources.len() > 1 { d.join(&stem) } else { d.clone() });
            let r = build(
                src,
                BuildOptions {
                    output: dest,
                    engine: a.engine.as_deref(),
                    template: a.template.as_deref(),
                    tex_dir,
                    ..Default::default()
                },
            )?;
            let tpl = r.template.as_deref().map(|t| format!(", template {t}")).unwrap_or_default();
            println!(
                "✓ {} -> {}  ({} p.{tpl}, {})",
                src.display(),
                r.pdf.display(),
                pages(&r.pdf),
                r.engine.as_deref().unwrap_or("-")
            );
            if let Some(d) = &r.tex_dir {
                println!("  LaTeX project: {}", std::path::absolute(d)?.display());
            }
            last = Some(r.pdf);
        }
        Ok(last)
    };

    let result = run_once()?;
    if a.open {
        if let Some(p) = &result {
            open_file(p);
        }
    }
    if a.watch {
        watch(&sources, &run_once)?;
    }
    Ok(())
}

fn snapshot(sources: &[PathBuf], cfg: &Config) -> HashMap<PathBuf, SystemTime> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { walk(&p, out) } else { out.push(p) }
        }
    }
    let mut files: Vec<PathBuf> = sources.to_vec();
    files.extend(cfg.project_file.clone());
    for d in cfg.template_dirs() {
        walk(&d, &mut files);
    }
    files.into_iter().filter_map(|f| Some((f.clone(), f.metadata().ok()?.modified().ok()?))).collect()
}

fn watch(sources: &[PathBuf], rebuild: &dyn Fn() -> Result<Option<PathBuf>>) -> Result<()> {
    let cfg = Config::load(&sources[0])?;
    println!("Watching for changes … (Ctrl+C to stop)");
    let mut state = snapshot(sources, &cfg);
    loop {
        std::thread::sleep(Duration::from_secs(1));
        let now = snapshot(sources, &cfg);
        if now != state {
            state = now;
            println!("[{}] change detected", chrono::Local::now().format("%H:%M:%S"));
            if let Err(e) = rebuild() {
                report_error(&e);
            }
        }
    }
}

// --------------------------------------------------------------------------
// templates / new / init
// --------------------------------------------------------------------------

fn shorten(s: &str, width: usize) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= width {
        return s;
    }
    let mut out = String::new();
    for word in s.split(' ') {
        if out.chars().count() + word.chars().count() + 3 > width {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    format!("{out} …")
}

fn cmd_templates(action: TemplateAction, name: Option<String>, to: Option<PathBuf>, force: bool) -> Result<()> {
    let cfg = Config::load_cwd()?;
    match action {
        TemplateAction::Guide => println!("{}", ai::GUIDE),
        TemplateAction::List => {
            let all = discover(&cfg)?;
            let width = all.keys().map(String::len).max().unwrap_or(0);
            for (n, t) in &all {
                let origin =
                    if t.origin.as_str() == "builtin" { String::new() } else { format!("  [{}]", t.origin.as_str()) };
                println!("  {n:<width$}  {}{origin}", shorten(&t.description(), 78));
            }
            println!("\nDetails: texify templates show <name>   New one with AI: texify ai template <name> \"…\"");
        }
        TemplateAction::Show | TemplateAction::Eject => {
            let Some(name) = name else { bail!("texify templates show|eject <name>") };
            let t = get_template(&name, &cfg)?;
            if matches!(action, TemplateAction::Show) {
                println!("{}  ({}: {})\n\n{}\n\nFields:", t.name, t.origin.as_str(), t.location(), t.description());
                for f in t.fields() {
                    println!("  {:<18} {}{}", f.name, f.description, if f.required { " (required)" } else { "" });
                }
                if let Some(h) = t.headings().and_then(|h| h.as_object()) {
                    println!("\nHeadings:");
                    for (lvl, spec) in h {
                        println!("  {:<4} -> {spec}", "#".repeat(lvl.parse().unwrap_or(1)));
                    }
                }
                println!("\nStart a document: texify new {}", t.name);
            } else {
                let dest = to.unwrap_or_else(|| cfg.root().join("templates")).join(&t.name);
                ensure_new(&dest, force, "--force")?;
                t.write_to(&dest)?;
                println!("✓ Template '{}' copied to {}", t.name, std::path::absolute(&dest)?.display());
                println!("  This copy takes precedence over the built-in one – adapt it freely.");
                println!("  Shared design package for reference: texify tex <doc> exports texify.sty with every build");
            }
        }
    }
    Ok(())
}

fn cmd_new(template: &str, file: Option<PathBuf>, force: bool) -> Result<()> {
    let t = get_template(template, &Config::load_cwd()?)?;
    let dest = file.unwrap_or_else(|| PathBuf::from(format!("{}.md", t.name)));
    ensure_new(&dest, force, "--force")?;
    std::fs::write(&dest, t.example())?;
    println!("✓ created {} (template '{}'). Edit it, then: texify build {}", dest.display(), t.name, dest.display());
    Ok(())
}

fn cmd_init(force: bool) -> Result<()> {
    let dest = PathBuf::from(CONFIG_NAME);
    ensure_new(&dest, force, "--force")?;
    std::fs::write(&dest, INIT_TEMPLATE)?;
    println!(
        "✓ created {} – enter your sender details once, they apply to every document.",
        std::path::absolute(&dest)?.display()
    );
    Ok(())
}

// --------------------------------------------------------------------------
// doctor / ai / mcp
// --------------------------------------------------------------------------

fn cmd_doctor() -> Result<()> {
    let cfg = Config::load_cwd()?;
    println!("TeXify {VERSION}");
    println!(
        "Project config:  {}",
        cfg.project_file.as_ref().map_or("– (texify init)".into(), |p| p.display().to_string())
    );
    println!("User folder:     {}", texify_home().display());
    println!("\nLaTeX engines:");
    for (name, ok) in engine::available(&cfg) {
        println!("  {} {name}", if ok { "✓" } else { "·" });
    }
    match engine::resolve(&cfg, None) {
        Ok(e) => println!("  -> using: {e}"),
        Err(_) => println!("  -> no engine available (see README: Installation)"),
    }
    match ai::ai_command(&cfg) {
        Ok(cmd) => println!("\nAI command (CLI): {}", cmd.join(" ")),
        Err(_) => println!("\nAI command (CLI): – not configured; AI is available through the MCP server (texify mcp)"),
    }
    println!("\nTemplate folders:");
    for d in cfg.template_dirs() {
        println!("  {} {}", if d.is_dir() { "✓" } else { "·" }, d.display());
    }
    println!(
        "  ✓ built-in ({} templates embedded)",
        discover(&Config { project_file: None, user_file: None, ..cfg.clone() })?
            .values()
            .filter(|t| t.origin.as_str() == "builtin")
            .count()
    );
    Ok(())
}

fn cmd_ai(action: AiCmd) -> Result<()> {
    let cfg = Config::load_cwd()?;
    let progress = |m: &str| println!("{m}");
    match action {
        AiCmd::Template { name, description, base, user, engine, force } => {
            let root = if user { user_template_dir() } else { cfg.root().join("templates") };
            let preview = PathBuf::from(format!("{name}-example.pdf"));
            let path = ai::generate_template(
                &name,
                &description,
                &cfg,
                &root,
                base.as_deref(),
                engine.as_deref(),
                force,
                Some(&preview),
                &progress,
            )?;
            println!("✓ template '{name}' created: {}", path.display());
            println!("  preview: {}", std::path::absolute(&preview)?.display());
            println!("  start a document: texify new {name}");
        }
        AiCmd::Fill { template, input, output, force } => {
            let ext = input.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            let content = if ext == "pdf" || ext == "docx" {
                importers::to_markdown(&input, None)?
            } else {
                std::fs::read_to_string(&input)?
            };
            let text = ai::fill(&template, &content, &cfg, &progress)?;
            let stem = input.file_stem().unwrap_or_default().to_string_lossy();
            let dest = output.unwrap_or_else(|| input.with_file_name(format!("{stem}-{template}.md")));
            ensure_new(&dest, force, "--force")?;
            std::fs::write(&dest, &text)?;
            let todos = text.matches("TODO").count();
            println!(
                "✓ {}{}",
                dest.display(),
                if todos > 0 { format!("  ({todos}× TODO – please complete)") } else { String::new() }
            );
            println!("  build: texify build {}", dest.display());
        }
    }
    Ok(())
}

/// Configuration snippet for MCP clients (stdio, HTTP or Docker).
pub fn mcp_client_config(
    mode: &str,
    format: ConfigFormat,
    workspace: &Path,
    url: &str,
    image: &str,
    exe: &str,
) -> String {
    let ws = workspace.display().to_string();
    let entry = match mode {
        "http" => json!({"type": "http", "url": url}),
        "docker" => {
            json!({"type": "stdio", "command": "docker", "args": ["run", "-i", "--rm", "-v", format!("{ws}:/work"), image, "mcp"]})
        }
        _ => json!({"type": "stdio", "command": exe, "args": ["mcp", "--workspace", ws]}),
    };
    if format == ConfigFormat::Vscode {
        return serde_json::to_string_pretty(&json!({"servers": {"texify": entry}})).unwrap_or_default();
    }
    let mut plain = entry.as_object().cloned().unwrap_or_default();
    if mode != "http" {
        plain.remove("type");
    }
    match format {
        ConfigFormat::Toml | ConfigFormat::Yaml => {
            let (head, indent) = if format == ConfigFormat::Toml {
                ("[mcp_servers.texify]", "")
            } else {
                ("mcp_servers:\n  texify:", "    ")
            };
            let sep = if format == ConfigFormat::Toml { " = " } else { ": " };
            let mut lines = vec![head.to_string()];
            for (k, v) in &plain {
                lines.push(format!("{indent}{k}{sep}{v}"));
            }
            lines.join("\n")
        }
        _ => serde_json::to_string_pretty(&json!({"mcpServers": {"texify": plain}})).unwrap_or_default(),
    }
}

fn cmd_mcp(a: McpCmd) -> Result<()> {
    let workspace = std::path::absolute(a.workspace.clone().unwrap_or_else(|| PathBuf::from(".")))?;
    if a.action == McpAction::Config {
        let mode = if a.http {
            "http"
        } else if a.docker {
            "docker"
        } else {
            "stdio"
        };
        let url = a.url.clone().unwrap_or_else(|| format!("http://localhost:{}/mcp", a.port));
        let cfg = Config::load(&workspace)?;
        let image = {
            let i = cfg.str("build", "docker_image");
            if i.is_empty() { "texify:latest".to_string() } else { i }
        };
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "texify".into());
        println!("{}", mcp_client_config(mode, a.format, &workspace, &url, &image, &exe));
        return Ok(());
    }
    if !workspace.is_dir() {
        bail!("Workspace folder does not exist: {}", workspace.display());
    }
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let result = rt.block_on(async {
        if a.http {
            mcp::serve_http(workspace, &a.host, a.port, a.allow_host).await
        } else {
            mcp::serve_stdio(workspace).await
        }
    });
    result.map_err(|e| Error::Msg(e.to_string()))
}

// --------------------------------------------------------------------------
// entry point
// --------------------------------------------------------------------------

fn report_error(e: &Error) {
    match e {
        Error::Compile(c) => eprintln!("{}", c.details()),
        Error::Msg(m) => eprintln!("Error: {m}"),
    }
}

fn dispatch(cmd: Cmd) -> Result<()> {
    match cmd {
        Cmd::Build(a) => cmd_build(a),
        Cmd::Tex { source, output, template } => {
            let stem = source.file_stem().unwrap_or_default().to_string_lossy();
            let target = output.unwrap_or_else(|| source.with_file_name(format!("{stem}-tex")));
            let tex = export_tex(&source, &target, template.as_deref())?;
            println!("✓ LaTeX project: {}", std::path::absolute(&target)?.display());
            println!(
                "  main file:     {}  (compile: texify build {})",
                tex.file_name().unwrap_or_default().to_string_lossy(),
                tex.display()
            );
            Ok(())
        }
        Cmd::New { template, file, force } => cmd_new(&template, file, force),
        Cmd::Templates { action, name, to, force } => cmd_templates(action, name, to, force),
        Cmd::Init { force } => cmd_init(force),
        Cmd::Merge { inputs, output, sort, bookmarks, force } => {
            ensure_new(&output, force, "-f")?;
            let mode = match sort {
                SortMode::None => "none",
                SortMode::Name => "name",
                SortMode::Date => "date",
            };
            let files = pdftools::sort_paths(pdftools::collect_inputs(&inputs, &["pdf"])?, mode);
            let report = pdftools::merge(&files, &output, bookmarks, None)?;
            for (f, n) in &report {
                println!("  + {} ({n} p.)", f.display());
            }
            let total: usize = report.iter().map(|(_, n)| n).sum();
            println!("Done: {} files, {total} pages -> {}", report.len(), std::path::absolute(&output)?.display());
            Ok(())
        }
        Cmd::Pages { source, ranges, output } => {
            let n = pdftools::select_pages(&source, &ranges, &output)?;
            println!("✓ {n} pages -> {}", std::path::absolute(&output)?.display());
            Ok(())
        }
        Cmd::Info { files } => {
            for f in files {
                let i = pdftools::info(&f)?;
                let size = i.page_size_mm.map_or("-".into(), |(w, h)| format!("{w}×{h} mm"));
                let title = i.title.map(|t| format!(", title: {t}")).unwrap_or_default();
                println!("{}: {} pages, {size}, {} KB{title}", f.display(), i.pages, i.bytes / 1024);
            }
            Ok(())
        }
        Cmd::Import { source, output, template, force } => {
            let mut dest = output.unwrap_or_else(|| source.with_extension("md"));
            if std::path::absolute(&dest)? == std::path::absolute(&source)? {
                let stem = source.file_stem().unwrap_or_default().to_string_lossy();
                dest = source.with_file_name(format!("{stem}-import.md"));
            }
            ensure_new(&dest, force, "--force")?;
            std::fs::write(&dest, importers::to_markdown(&source, template.as_deref())?)?;
            println!("✓ {} -> {}", source.display(), dest.display());
            println!(
                "  Check the structure; to fit it to a template with AI: texify ai fill <template> {}",
                dest.display()
            );
            Ok(())
        }
        Cmd::Doctor => cmd_doctor(),
        Cmd::Ai { action } => cmd_ai(action),
        Cmd::Mcp(a) => cmd_mcp(a),
    }
}

pub fn run() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli.cmd) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e @ Error::Compile(_)) => {
            report_error(&e);
            ExitCode::from(2)
        }
        Err(e) => {
            report_error(&e);
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_config_formats() {
        let ws = Path::new("/docs");
        let http: serde_json::Value = serde_json::from_str(&mcp_client_config(
            "http",
            ConfigFormat::Json,
            ws,
            "http://localhost:8747/mcp",
            "texify:latest",
            "texify",
        ))
        .unwrap();
        assert_eq!(http, json!({"mcpServers": {"texify": {"type": "http", "url": "http://localhost:8747/mcp"}}}));
        let vs: serde_json::Value =
            serde_json::from_str(&mcp_client_config("stdio", ConfigFormat::Vscode, ws, "", "img", "/bin/texify"))
                .unwrap();
        assert_eq!(vs["servers"]["texify"]["args"][0], "mcp");
        let toml = mcp_client_config("docker", ConfigFormat::Toml, ws, "", "texify:latest", "texify");
        assert!(toml.starts_with("[mcp_servers.texify]") && toml.contains("\"docker\""));
    }
}

//! TeXify MCP server — one server for every MCP client.
//!
//! Works with any client that speaks the Model Context Protocol (Claude,
//! ChatGPT, VS Code/Copilot, Cursor, Windsurf, Zed, Codex, Hermes, ...):
//!
//! * stdio:           the client starts `texify mcp` itself
//! * Streamable HTTP: `texify mcp --http` runs ONE server (e.g. in Docker) that
//!   any number of clients connect to at `http://<host>:<port>/mcp`
//!
//! The server never calls an AI model itself: the client's model writes
//! documents, builds them with the deterministic template pipeline and – only
//! where no template fits – designs new templates (saved only after a
//! successful test build). All paths are confined to the workspace.

use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    Implementation, ListResourceTemplatesResult, ListResourcesResult, PaginatedRequestParams, PromptMessage,
    ReadResourceRequestParams, ReadResourceResult, Resource, ResourceContents, ResourceTemplate, Role,
    ServerCapabilities, ServerInfo,
};
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, prompt, prompt_handler, prompt_router, tool,
    tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::ai::{
    GUIDE, TemplateFiles, fill_system_prompt, install_template, pick_reference, template_system_prompt, test_template,
};
use crate::build::{BuildOptions, build, render_to_string};
use crate::config::Config;
use crate::error::Error;

/// Not named `Result`: the rmcp macros expand to code using the std name.
type TxResult<T> = crate::error::Result<T>;
use crate::importers::to_markdown;
use crate::pdftools::{collect_inputs, info, merge};
use crate::templates::{discover, get_template};
use crate::{VERSION, bail};

pub const DEFAULT_PORT: u16 = 8747;
const TEXT_SUFFIXES: [&str; 9] = ["md", "markdown", "txt", "tex", "yaml", "yml", "toml", "log", "sty"];
const INSTRUCTIONS: &str = "TeXify turns Markdown/Text with YAML frontmatter into professionally designed PDFs via \
LaTeX templates (CVs, cover letters, invoices, quotes, contracts, minutes, invitations, certificates, ...). \
Workflow: list_templates -> describe_template -> write_document -> build_document. Prefer an existing template. \
Only if none fits, read template_guide and call create_template (it test-builds before saving). \
Never invent personal data; ask the user for missing required fields.";

#[derive(Clone)]
pub struct TexifyServer {
    root: Arc<PathBuf>,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

// ---------------------------------------------------------------- arguments

#[derive(Deserialize, JsonSchema)]
pub struct DescribeArgs {
    /// Template name, e.g. "invoice"
    name: String,
    /// Also return template.tex / template.yaml (reference for new templates)
    #[serde(default)]
    include_source: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateTemplateArgs {
    /// Template name (lower-case letters, digits, '-' and '_')
    name: String,
    template_tex: String,
    template_yaml: String,
    example_md: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct WriteArgs {
    /// Path relative to the workspace
    path: String,
    content: String,
    #[serde(default)]
    overwrite: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct PathArgs {
    /// Path relative to the workspace
    path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Glob pattern, e.g. "**/*.md" (default: all files)
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct BuildArgs {
    /// Source document (.md/.txt/.yaml/.tex) relative to the workspace
    source: String,
    /// Target PDF (default: next to the source or [build] out_dir)
    #[serde(default)]
    output: Option<String>,
    /// Force a template instead of the frontmatter
    #[serde(default)]
    template: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct MergeArgs {
    /// PDF files, folders or globs, in order
    inputs: Vec<String>,
    output: String,
    #[serde(default)]
    bookmarks: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct ImportArgs {
    source: String,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    template: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateDocumentArgs {
    /// Template to use, e.g. "cv"
    template: String,
    /// Notes or unstructured text with the content
    content: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct DesignTemplateArgs {
    /// Name of the new template
    name: String,
    /// What the document is for and how it should look
    description: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct BundleArgs {
    position: String,
    company: String,
}

// ---------------------------------------------------------------- helpers

fn ok(v: Value) -> String {
    let mut v = v;
    v["ok"] = Value::Bool(true);
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

fn fail(e: &Error) -> String {
    let mut v = json!({"ok": false, "error": e.to_string()});
    if let Error::Compile(c) = e {
        v["details"] = Value::String(c.details());
    }
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// Run blocking work (LaTeX, file IO) off the async runtime; errors become tool results.
async fn blocking(f: impl FnOnce() -> TxResult<Value> + Send + 'static) -> String {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => ok(v),
        Ok(Err(e)) => fail(&e),
        Err(e) => fail(&Error::Msg(e.to_string())),
    }
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Resolve a workspace-relative path; refuses paths outside the workspace.
fn ws_path(root: &Path, rel: &str) -> TxResult<PathBuf> {
    let p = normalize(&root.join(rel));
    if !p.starts_with(root) {
        bail!("Path is outside the workspace: {rel}");
    }
    Ok(p)
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| p.display().to_string())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.file_name().is_some_and(|n| n == ".git") {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let mut re = String::from("^");
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' if chars.peek() == Some(&'*') => {
                chars.next();
                if chars.peek() == Some(&'/') {
                    chars.next();
                    re.push_str("(?:.*/)?");
                } else {
                    re.push_str(".*");
                }
            }
            '*' => re.push_str("[^/]*"),
            '?' => re.push_str("[^/]"),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    regex::Regex::new(&re).is_ok_and(|r| r.is_match(path))
}

fn user_message(text: String) -> Vec<PromptMessage> {
    vec![PromptMessage::new_text(Role::User, text)]
}

// ---------------------------------------------------------------- tools

#[tool_router]
impl TexifyServer {
    pub fn new(root: PathBuf) -> Self {
        Self { root: Arc::new(root), tool_router: Self::tool_router(), prompt_router: Self::prompt_router() }
    }

    fn cfg(&self) -> TxResult<Config> {
        Config::load(&self.root)
    }

    #[tool(description = "List all available document templates (built-in, user and project).")]
    async fn list_templates(&self) -> String {
        let s = self.clone();
        blocking(move || {
            let cfg = s.cfg()?;
            let list: Vec<Value> = discover(&cfg)?
                .values()
                .map(|t| json!({"name": t.name, "description": t.description(), "origin": t.origin.as_str()}))
                .collect();
            Ok(json!({"templates": list}))
        })
        .await
    }

    #[tool(
        description = "Fields, heading conventions and a complete example document for a template. include_source=true also returns template.tex/template.yaml (reference before creating a new template)."
    )]
    async fn describe_template(&self, Parameters(a): Parameters<DescribeArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let cfg = s.cfg()?;
            let t = get_template(&a.name, &cfg)?;
            let fields: Vec<Value> = t
                .fields()
                .iter()
                .map(|f| json!({"name": f.name, "description": f.description, "required": f.required}))
                .collect();
            let mut v = json!({
                "name": t.name, "description": t.description(), "fields": fields,
                "headings": t.headings(), "example_md": t.example(), "project_defaults": cfg.defaults(),
            });
            if a.include_source {
                v["template_tex"] = Value::String(t.source());
                v["template_yaml"] = Value::String(t.meta_source());
            }
            Ok(v)
        })
        .await
    }

    #[tool(
        description = "Authoring guide for TeXify templates (syntax, filters, design rules). Read before create_template."
    )]
    async fn template_guide(&self) -> String {
        GUIDE.to_string()
    }

    #[tool(
        description = "Create a new template in <workspace>/templates/<name>. The example is built first; the template is saved only if that succeeds. On failure the LaTeX log is returned – fix it and call again."
    )]
    async fn create_template(&self, Parameters(a): Parameters<CreateTemplateArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let cfg = s.cfg()?;
            let files = TemplateFiles { tex: a.template_tex, yaml: a.template_yaml, example: a.example_md };
            let preview = s.root.join(format!("{}-example.pdf", a.name));
            if let Some(report) = test_template(&a.name, &files, &cfg, None, Some(&preview))? {
                bail!("test build failed:\n{report}");
            }
            let path = install_template(&a.name, &files, &cfg.root().join("templates"), a.overwrite)?;
            Ok(json!({"template": rel(&s.root, &path), "preview_pdf": rel(&s.root, &preview)}))
        })
        .await
    }

    #[tool(description = "Write a Markdown/Text/YAML document (or texify.toml) into the workspace.")]
    async fn write_document(&self, Parameters(a): Parameters<WriteArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let p = ws_path(&s.root, &a.path)?;
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !TEXT_SUFFIXES.contains(&ext.as_str()) {
                bail!("Only text files allowed ({})", TEXT_SUFFIXES.join(", "));
            }
            if p.exists() && !a.overwrite {
                bail!("{} already exists (overwrite=true)", a.path);
            }
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&p, a.content)?;
            Ok(json!({"written": rel(&s.root, &p)}))
        })
        .await
    }

    #[tool(description = "Read a text file from the workspace (.md, .tex, .yaml, .toml, .log, ...).")]
    async fn read_file(&self, Parameters(a): Parameters<PathArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let p = ws_path(&s.root, &a.path)?;
            if !p.is_file() {
                bail!("Not found: {}", a.path);
            }
            let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
            if !TEXT_SUFFIXES.contains(&ext.as_str()) {
                bail!("Binary file – use pdf_info or import_document for PDFs");
            }
            let content = String::from_utf8_lossy(&std::fs::read(&p)?).into_owned();
            Ok(json!({"path": rel(&s.root, &p), "content": content}))
        })
        .await
    }

    #[tool(description = "List workspace files matching a glob pattern (e.g. '**/*.md').")]
    async fn list_files(&self, Parameters(a): Parameters<ListArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let pattern = a.pattern.unwrap_or_else(|| "**/*".into());
            let mut all = Vec::new();
            walk(&s.root, &mut all);
            let mut files: Vec<String> =
                all.iter().map(|p| rel(&s.root, p)).filter(|r| glob_match(&pattern, r)).collect();
            files.sort();
            let total = files.len();
            files.truncate(a.limit.unwrap_or(200));
            Ok(json!({"files": files, "total": total}))
        })
        .await
    }

    #[tool(
        description = "Build a document (.md/.txt/.yaml/.tex) into a PDF. Returns the PDF path or the LaTeX error with log excerpt."
    )]
    async fn build_document(&self, Parameters(a): Parameters<BuildArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let source = ws_path(&s.root, &a.source)?;
            let output = a.output.as_deref().map(|o| ws_path(&s.root, o)).transpose()?;
            let r = build(&source, BuildOptions { output, template: a.template.as_deref(), ..Default::default() })?;
            Ok(json!({"pdf": rel(&s.root, &r.pdf), "pages": info(&r.pdf)?.pages, "template": r.template, "engine": r.engine}))
        })
        .await
    }

    #[tool(description = "Return the generated LaTeX source of a document without compiling it.")]
    async fn render_latex(&self, Parameters(a): Parameters<BuildArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let source = ws_path(&s.root, &a.source)?;
            Ok(json!({"latex": render_to_string(&source, None, a.template.as_deref())?}))
        })
        .await
    }

    #[tool(description = "Merge PDFs (files, folders or globs, in the given order) into one PDF.")]
    async fn merge_pdfs(&self, Parameters(a): Parameters<MergeArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let inputs: Vec<String> = a
                .inputs
                .iter()
                .map(|i| ws_path(&s.root, i).map(|p| p.display().to_string()))
                .collect::<TxResult<_>>()?;
            let files = collect_inputs(&inputs, &["pdf"])?;
            let out = ws_path(&s.root, &a.output)?;
            let report = merge(&files, &out, a.bookmarks, None)?;
            let parts: Vec<Value> = report.iter().map(|(f, n)| json!({"file": rel(&s.root, f), "pages": n})).collect();
            Ok(json!({"pdf": a.output, "parts": parts}))
        })
        .await
    }

    #[tool(description = "Extract text from a PDF/DOCX/TXT into a Markdown document (no AI involved).")]
    async fn import_document(&self, Parameters(a): Parameters<ImportArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let src = ws_path(&s.root, &a.source)?;
            let text = to_markdown(&src, a.template.as_deref())?;
            let mut dest = match &a.output {
                Some(o) => ws_path(&s.root, o)?,
                None => src.with_extension("md"),
            };
            if dest.exists() {
                dest = dest
                    .with_file_name(format!("{}-import.md", dest.file_stem().unwrap_or_default().to_string_lossy()));
            }
            std::fs::write(&dest, &text)?;
            let preview: String = text.chars().take(20000).collect();
            Ok(json!({"markdown_file": rel(&s.root, &dest), "content": preview}))
        })
        .await
    }

    #[tool(description = "Page count, page size and metadata of a PDF.")]
    async fn pdf_info(&self, Parameters(a): Parameters<PathArgs>) -> String {
        let s = self.clone();
        blocking(move || {
            let p = ws_path(&s.root, &a.path)?;
            let mut v = serde_json::to_value(info(&p)?).unwrap_or_default();
            v["file"] = Value::String(rel(&s.root, &p));
            Ok(v)
        })
        .await
    }
}

// ---------------------------------------------------------------- prompts

#[prompt_router]
impl TexifyServer {
    /// Turn notes or unstructured text into a finished PDF using an existing template.
    #[prompt(name = "create_document")]
    async fn create_document(&self, Parameters(a): Parameters<CreateDocumentArgs>) -> Vec<PromptMessage> {
        let text = match self.cfg().and_then(|cfg| get_template(&a.template, &cfg)) {
            Ok(t) => format!(
                "{}\n\nInstead of printing the <document> block, save it with write_document (e.g. to a .md file \
                 named after the content) and then call build_document. If the build fails, fix the document and \
                 build again.\n\nContent:\n\n{}",
                fill_system_prompt(&t),
                a.content
            ),
            Err(e) => format!("{e}\nCall list_templates to see the available templates."),
        };
        user_message(text)
    }

    /// Design a new template for a document type that has no template yet.
    #[prompt(name = "design_template")]
    async fn design_template(&self, Parameters(a): Parameters<DesignTemplateArgs>) -> Vec<PromptMessage> {
        let text = match self.cfg().and_then(|cfg| get_template(pick_reference(&a.description), &cfg)) {
            Ok(reference) => format!(
                "{}\n\nInstead of printing the three blocks, pass them to the tool create_template. It test-builds \
                 the example; if it reports an error, fix it and call create_template again (overwrite=true) until \
                 it succeeds.\n\nCreate the template `{}`.\n\nRequirements:\n{}",
                template_system_prompt(&reference),
                a.name,
                a.description
            ),
            Err(e) => e.to_string(),
        };
        user_message(text)
    }

    /// Assemble a job application: cover page, cover letter and CV merged into one PDF.
    #[prompt(name = "application_bundle")]
    async fn application_bundle(&self, Parameters(a): Parameters<BundleArgs>) -> Vec<PromptMessage> {
        user_message(format!(
            "Create a job application for the position '{}' at '{}'.\n\
             1. Read texify.toml (read_file) and existing documents (list_files) to reuse the applicant's data; \
             ask the user for anything missing – never invent facts.\n\
             2. Use describe_template for 'coverpage', 'letter' and 'cv' (or 'cv-cards').\n\
             3. Write the three documents with write_document and check each with build_document.\n\
             4. Merge them with merge_pdfs (bookmarks=true) into application.pdf, appending any certificates the \
             user provides.",
            a.position, a.company
        ))
    }
}

// ---------------------------------------------------------------- handler

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for TexifyServer {
    fn get_info(&self) -> ServerInfo {
        let mut implementation = Implementation::from_build_env();
        implementation.name = "texify".into();
        implementation.version = VERSION.into();
        ServerInfo::new(ServerCapabilities::builder().enable_tools().enable_prompts().enable_resources().build())
            .with_server_info(implementation)
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let mut guide = Resource::new("texify://guide", "guide");
        guide.description = Some("Template authoring guide".into());
        guide.mime_type = Some("text/markdown".into());
        Ok(ListResourcesResult::with_all_items(vec![guide]))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        let mut t = ResourceTemplate::new("texify://templates/{name}", "template");
        t.description = Some("Description, fields and example document of one template".into());
        t.mime_type = Some("text/markdown".into());
        Ok(ListResourceTemplatesResult::with_all_items(vec![t]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, McpError> {
        let uri = request.uri.clone();
        let text = if uri == "texify://guide" {
            GUIDE.to_string()
        } else if let Some(name) = uri.strip_prefix("texify://templates/") {
            let t = self
                .cfg()
                .and_then(|cfg| get_template(name, &cfg))
                .map_err(|e| McpError::invalid_params(e.to_string(), None))?;
            let fields: Vec<String> = t
                .fields()
                .iter()
                .map(|f| format!("- `{}`: {}{}", f.name, f.description, if f.required { " (required)" } else { "" }))
                .collect();
            format!(
                "# Template `{}`\n\n{}\n\n## Fields\n\n{}\n\n## Example\n\n```markdown\n{}```\n",
                t.name,
                t.description(),
                fields.join("\n"),
                t.example()
            )
        } else {
            return Err(McpError::invalid_params(format!("Unknown resource: {uri}"), None));
        };
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, uri)]))
    }
}

// ---------------------------------------------------------------- entry point

/// Serve over stdio (the client starts the process).
pub async fn serve_stdio(root: PathBuf) -> anyhow::Result<()> {
    let service = TexifyServer::new(root).serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}

/// Serve over Streamable HTTP at `http://host:port/mcp` (one server, many clients).
pub async fn serve_http(root: PathBuf, host: &str, port: u16, allowed_hosts: Vec<String>) -> anyhow::Result<()> {
    use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
    use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};

    let mut config = StreamableHttpServerConfig::default();
    config.allowed_hosts.extend(allowed_hosts);
    let server = TexifyServer::new(root.clone());
    let service =
        StreamableHttpService::new(move || Ok(server.clone()), Arc::new(LocalSessionManager::default()), config);
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    eprintln!("TeXify {VERSION} MCP server: http://{host}:{port}/mcp  (workspace: {})", root.display());
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_stay_in_workspace() {
        let root = PathBuf::from("/work");
        assert!(ws_path(&root, "docs/a.md").is_ok());
        assert!(ws_path(&root, "../etc/passwd").is_err());
        assert!(ws_path(&root, "a/../../b").is_err());
    }

    #[test]
    fn globbing() {
        assert!(glob_match("**/*.md", "a.md"));
        assert!(glob_match("**/*.md", "x/y/a.md"));
        assert!(!glob_match("*.md", "x/a.md"));
    }

    #[test]
    fn tools_and_prompts_registered() {
        let s = TexifyServer::new(PathBuf::from("."));
        let tools: Vec<String> = s.tool_router.list_all().iter().map(|t| t.name.to_string()).collect();
        for name in ["list_templates", "build_document", "create_template", "merge_pdfs", "pdf_info"] {
            assert!(tools.contains(&name.to_string()), "{name}");
        }
        assert_eq!(tools.len(), 12);
        let prompts: Vec<String> = s.prompt_router.list_all().iter().map(|p| p.name.to_string()).collect();
        assert_eq!(prompts.len(), 3);
    }
}

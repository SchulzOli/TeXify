//! AI assistance — only where deterministic code cannot help.
//!
//! 1. [`generate_template`]: design a new template for a document type that has
//!    no template yet, validated by a real test build; errors are fed back to the
//!    model until it compiles.
//! 2. [`fill`]: turn unstructured text into a document for a given template.
//!
//! TeXify is provider-agnostic. Either an MCP client's model does the work
//! (tools/prompts of `texify mcp`), or `[ai] command` names any program that
//! reads a prompt on stdin and prints the answer (`llm -m gpt-4o`,
//! `ollama run llama3.1`, `claude -p`, ...).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::build::{BuildOptions, build, parse_document, render_tex};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::templates::{EXAMPLE_FILE, META_FILE, TEMPLATE_FILE, Template, get_template, parse_meta};
use crate::{bail, err};

pub const GUIDE: &str = include_str!("../assets/guide.md");
const BLOCKS: [&str; 3] = ["template_tex", "template_yaml", "example_md"];

pub struct Message {
    pub user: bool,
    pub content: String,
}

/// The configured AI command (`TEXIFY_AI_COMMAND` or `[ai] command`).
pub fn ai_command(cfg: &Config) -> Result<Vec<String>> {
    let raw = std::env::var("TEXIFY_AI_COMMAND")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| cfg.str("ai", "command"));
    if raw.trim().is_empty() {
        bail!(
            "No AI command configured. Either\n  \
             • use TeXify through its MCP server from any AI client (texify mcp), or\n  \
             • set [ai] command in texify.toml (or TEXIFY_AI_COMMAND) to any tool that reads\n    \
             a prompt on stdin, e.g. \"llm -m gpt-4o\", \"ollama run llama3.1\", \"claude -p\"."
        );
    }
    let mut parts = shlex::split(&raw).ok_or_else(|| err!("Cannot parse AI command: {raw}"))?;
    if parts.is_empty() {
        bail!("AI command is empty");
    }
    let exe = which::which(&parts[0]).map_err(|_| err!("AI command not found: {}", parts[0]))?;
    parts[0] = exe.display().to_string();
    Ok(parts)
}

/// Send a conversation to the configured AI command and return its answer.
pub fn complete(system: &str, messages: &[Message], cfg: &Config) -> Result<String> {
    let cmd = ai_command(cfg)?;
    let mut prompt = vec![system.to_string(), String::new()];
    for m in messages {
        prompt.push(format!("=== {} ===", if m.user { "TASK" } else { "YOUR PREVIOUS ANSWER" }));
        prompt.push(m.content.clone());
        prompt.push(String::new());
    }
    prompt.push("Answer now, strictly in the required output format.".into());
    let timeout = Duration::from_secs(cfg.int("ai", "timeout", 900).max(1) as u64);

    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| err!("AI command failed to start: {e}"))?;
    let input = prompt.join("\n");
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || stdin.write_all(input.as_bytes()));
    let start = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            bail!("AI command timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = writer.join();
    let out = child.wait_with_output()?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if !out.status.success() {
        let msg =
            if out.stderr.is_empty() { stdout.clone() } else { String::from_utf8_lossy(&out.stderr).into_owned() };
        bail!("AI command failed: {}", tail(msg.trim(), 800));
    }
    if stdout.trim().is_empty() {
        bail!("AI command returned an empty answer");
    }
    Ok(stdout)
}

fn tail(s: &str, n: usize) -> &str {
    let start = s.len().saturating_sub(n);
    let start = (start..=s.len()).find(|&i| s.is_char_boundary(i)).unwrap_or(s.len());
    &s[start..]
}

// --------------------------------------------------------------------------
// Template generation
// --------------------------------------------------------------------------

const REFERENCE_HINTS: [(&str, &[&str]); 9] = [
    ("invoice", &["invoice", "rechnung", "quote", "angebot", "receipt", "quittung", "credit note"]),
    ("contract", &["contract", "vertrag", "agreement", "lease", "terms"]),
    ("minutes", &["minutes", "protokoll", "agenda", "tagesordnung", "meeting"]),
    ("invitation", &["invitation", "einladung", "event", "party", "feier"]),
    ("certificate", &["certificate", "urkunde", "award", "diploma", "zertifikat"]),
    ("letter", &["letter", "brief", "anschreiben", "cancellation", "kündigung", "complaint", "notice"]),
    ("cv", &["cv", "resume", "résumé", "lebenslauf", "vita"]),
    ("confirmation", &["confirmation", "bestätigung", "attestation", "reference", "power of attorney"]),
    ("proposal", &["proposal", "antrag", "grant", "concept", "pitch"]),
];

/// Built-in template that is closest to a description (style reference).
pub fn pick_reference(description: &str) -> &'static str {
    let d = description.to_lowercase();
    REFERENCE_HINTS.iter().find(|(_, words)| words.iter().any(|w| d.contains(w))).map_or("article", |(name, _)| name)
}

pub fn template_system_prompt(reference: &Template) -> String {
    format!(
        "You are an expert LaTeX typographer creating templates for TeXify.\n\
TeXify turns readable Markdown + YAML frontmatter into professional PDFs; the\n\
design lives entirely in the template.\n\n{GUIDE}\n\n\
# Reference template '{name}' (follow its style and conventions)\n\n\
<template_tex>\n{tex}\n</template_tex>\n<template_yaml>\n{yaml}\n</template_yaml>\n<example_md>\n{example}\n</example_md>\n\n\
# Output format\n\
Output EXACTLY three blocks, no other text and no Markdown code fences:\n\
<template_tex>…complete template.tex…</template_tex>\n\
<template_yaml>…complete template.yaml…</template_yaml>\n\
<example_md>…a realistic, complete example document with fictional data…</example_md>\n\
The example must contain `template: <name>` in its frontmatter and fill all required fields.",
        name = reference.name,
        tex = reference.source(),
        yaml = reference.meta_source(),
        example = reference.example(),
    )
}

/// Extract `<name>…</name>` blocks from a model answer.
pub fn extract_blocks(text: &str, names: &[&str]) -> Result<Vec<String>> {
    let fence = regex::Regex::new(r"^```[a-z]*\n|\n```$").unwrap();
    names
        .iter()
        .map(|name| {
            let re = regex::Regex::new(&format!(r"(?s)<{name}>\s*(.*?)\s*</{name}>")).unwrap();
            let m = re.captures(text).ok_or_else(|| err!("Answer contains no <{name}> block"))?;
            Ok(format!("{}\n", fence.replace_all(m[1].trim(), "").trim()))
        })
        .collect()
}

/// Template files: (template.tex, template.yaml, example.md).
pub struct TemplateFiles {
    pub tex: String,
    pub yaml: String,
    pub example: String,
}

impl TemplateFiles {
    fn write(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(TEMPLATE_FILE), &self.tex)?;
        std::fs::write(dir.join(META_FILE), &self.yaml)?;
        std::fs::write(dir.join(EXAMPLE_FILE), &self.example)?;
        Ok(())
    }
}

/// Build the template's example in a temp folder. `Ok(None)` = success,
/// `Ok(Some(report))` = an error report for the model.
pub fn test_template(
    name: &str,
    files: &TemplateFiles,
    cfg: &Config,
    engine: Option<&str>,
    pdf_out: Option<&Path>,
) -> Result<Option<String>> {
    let tmp = tempfile::Builder::new().prefix("texify-tpl-").tempdir()?;
    let root = tmp.path().join("templates");
    files.write(&root.join(name))?;
    let check = || -> Result<()> {
        parse_meta(&files.yaml, "template.yaml")?;
        let doc = parse_document(&files.example, None)?;
        if doc.meta["template"].as_str() != Some(name) {
            bail!("example_md must contain `template: {name}` in its frontmatter.");
        }
        let example = tmp.path().join("example.md");
        std::fs::write(&example, &files.example)?;
        let result = build(
            &example,
            BuildOptions {
                output: Some(tmp.path().join("example.pdf")),
                cfg: Some(cfg.with_template_dir(&root)),
                engine,
                ..Default::default()
            },
        )?;
        if let Some(out) = pdf_out {
            if let Some(p) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(p)?;
            }
            std::fs::copy(&result.pdf, out)?;
        }
        Ok(())
    };
    match check() {
        Ok(()) => Ok(None),
        Err(Error::Compile(c)) => Ok(Some(format!("LaTeX compilation failed:\n{}", c.details()))),
        Err(e) => Ok(Some(format!("Error: {e}"))),
    }
}

pub fn install_template(name: &str, files: &TemplateFiles, target_root: &Path, overwrite: bool) -> Result<PathBuf> {
    if !regex::Regex::new(r"^[a-z0-9][a-z0-9_-]*$").unwrap().is_match(name) {
        bail!("Template name: lower-case letters, digits, '-' and '_' only");
    }
    let target = target_root.join(name);
    if target.exists() {
        if !overwrite {
            bail!("Template already exists: {} (allow overwriting)", target.display());
        }
        std::fs::remove_dir_all(&target)?;
    }
    files.write(&target)?;
    Ok(target)
}

#[allow(clippy::too_many_arguments)]
pub fn generate_template(
    name: &str,
    description: &str,
    cfg: &Config,
    target_root: &Path,
    base: Option<&str>,
    engine: Option<&str>,
    overwrite: bool,
    preview: Option<&Path>,
    progress: &dyn Fn(&str),
) -> Result<PathBuf> {
    let reference = get_template(base.unwrap_or(pick_reference(description)), cfg)?;
    let system = template_system_prompt(&reference);
    let mut messages =
        vec![Message { user: true, content: format!("Create the template `{name}`.\n\nRequirements:\n{description}") }];
    let attempts = cfg.int("ai", "attempts", 3).max(1);
    let mut last_error = String::new();
    for attempt in 1..=attempts {
        progress(&format!(
            "AI is designing template '{name}' (attempt {attempt}/{attempts}, reference: {}) …",
            reference.name
        ));
        let reply = complete(&system, &messages, cfg)?;
        match extract_blocks(&reply, &BLOCKS) {
            Err(e) => last_error = e.to_string(),
            Ok(blocks) => {
                let files =
                    TemplateFiles { tex: blocks[0].clone(), yaml: blocks[1].clone(), example: blocks[2].clone() };
                progress("Test build of the example …");
                match test_template(name, &files, cfg, engine, preview)? {
                    None => return install_template(name, &files, target_root, overwrite),
                    Some(report) => last_error = report,
                }
            }
        }
        progress(&format!("  ✗ {}", last_error.lines().next().unwrap_or_default()));
        messages.push(Message { user: false, content: reply });
        messages.push(Message {
            user: true,
            content: format!(
                "The test build failed:\n\n{last_error}\n\nFix the problem and output all three blocks again, complete."
            ),
        });
    }
    bail!("Could not create the template in {attempts} attempts.\nLast error:\n{last_error}")
}

// --------------------------------------------------------------------------
// Fill: unstructured text -> document for a template
// --------------------------------------------------------------------------

pub fn fill_system_prompt(template: &Template) -> String {
    let fields: Vec<String> = template
        .fields()
        .iter()
        .map(|f| format!("- {}: {}{}", f.name, f.description, if f.required { " (required)" } else { "" }))
        .collect();
    let headings =
        template.headings().map_or_else(|| "default (# section, ## subsection)".to_string(), |h| h.to_string());
    format!(
        "You turn unstructured content into a TeXify document for the template '{name}'.\n\n\
Template description: {desc}\n\nFields (template.yaml):\n{fields}\n\
Heading convention (headings): {headings}\n\n\
Example document for this template:\n<example>\n{example}\n</example>\n\n\
Rules:\n\
- Use ONLY information from the user's content. Do not invent anything.\n\
- If required information is missing, use the value \"TODO: <what is missing>\".\n\
- Running text and sections belong in the Markdown body, not in YAML; YAML is for structured data only.\n\
- Document language = language of the content (field lang: en or de).\n\
- Output: exactly one block <document>…</document> with the complete document\n  \
(YAML frontmatter with `template: {name}` + Markdown), no code fences.",
        name = template.name,
        desc = template.description(),
        fields = fields.join("\n"),
        example = template.example(),
    )
}

pub fn fill(template_name: &str, content: &str, cfg: &Config, progress: &dyn Fn(&str)) -> Result<String> {
    let template = get_template(template_name, cfg)?;
    let system = fill_system_prompt(&template);
    let mut messages = vec![Message { user: true, content: format!("Content:\n\n{content}") }];
    let mut error = String::new();
    for attempt in 1..=2 {
        progress(&format!("AI is structuring the content for template '{}' (attempt {attempt}/2) …", template.name));
        let reply = complete(&system, &messages, cfg)?;
        let check = || -> Result<String> {
            let doc_text = extract_blocks(&reply, &["document"])?.remove(0);
            let mut doc = parse_document(&doc_text, None)?;
            doc.meta["template"] = serde_json::Value::String(template.name.clone());
            let tmp = tempfile::Builder::new().prefix("texify-fill-").tempdir()?;
            render_tex(&doc, cfg, tmp.path(), None)?;
            Ok(doc_text)
        };
        match check() {
            Ok(doc) => return Ok(doc),
            Err(e) => {
                error = e.to_string();
                messages.push(Message { user: false, content: reply });
                messages.push(Message {
                    user: true,
                    content: format!("The document is invalid:\n{error}\nPlease fix it."),
                });
            }
        }
    }
    bail!("AI result is invalid: {error}")
}

//! Development tasks:
//!
//!   cargo xtask examples          build every example, the application bundle,
//!                                 preview images and examples/README.md
//!   cargo xtask third-party       regenerate THIRD_PARTY.md
//!   cargo xtask third-party --check   fail if THIRD_PARTY.md is outdated (CI)

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;
use texify::build::{BuildOptions, build};
use texify::config::Config;
use texify::pdftools::merge;

const BUNDLE: [&str; 3] = ["13-application-cover", "05-cover-letter", "11-cv-classic"];
const COLUMNS: usize = 3;
const TYPES: [(&str, &str); 21] = [
    ("01-report", "Report"),
    ("02-how-to-guide", "How-to guide"),
    ("03-requirements-spec", "Requirements specification"),
    ("04-press-release", "Press release"),
    ("05-cover-letter", "Cover letter"),
    ("06-cancellation", "Cancellation letter"),
    ("07-payment-reminder", "Payment reminder"),
    ("08-invoice", "Invoice"),
    ("09-invoice-small-business", "Invoice (VAT-exempt small business)"),
    ("10-quote", "Quote"),
    ("11-cv-classic", "CV (classic, tabular)"),
    ("12-cv-modern", "CV (modern, cards)"),
    ("13-application-cover", "Application cover page"),
    ("14-project-proposal", "Project proposal"),
    ("15-certificate-of-attendance", "Certificate of attendance"),
    ("16-employment-reference", "Employment reference"),
    ("17-power-of-attorney", "Power of attorney"),
    ("18-meeting-minutes", "Meeting minutes"),
    ("19-invitation", "Invitation"),
    ("20-certificate", "Certificate"),
    ("21-service-agreement", "Service agreement"),
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("examples") => examples(),
        Some("third-party") => third_party(args.iter().any(|a| a == "--check")),
        _ => bail!("usage: cargo xtask examples | third-party [--check]"),
    }
}

// --------------------------------------------------------------------------
// examples
// --------------------------------------------------------------------------

fn type_of(stem: &str) -> String {
    TYPES.iter().find(|(s, _)| *s == stem).map_or_else(|| stem.to_string(), |(_, t)| t.to_string())
}

/// First page as PNG, 420 px wide (pdftoppm locally, else inside the texify image).
fn render_preview(pdf: &Path, png: &Path) -> Result<()> {
    let prefix = png.with_extension("");
    let local = Command::new("pdftoppm").arg("-v").output().is_ok();
    let status = if local {
        Command::new("pdftoppm")
            .args(["-png", "-f", "1", "-l", "1", "-scale-to-x", "420", "-scale-to-y", "-1", "-singlefile"])
            .arg(pdf)
            .arg(&prefix)
            .status()?
    } else {
        let dir = pdf.parent().unwrap().parent().unwrap();
        let rel = |p: &Path| p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
        Command::new("docker")
            .args(["run", "--rm", "--mount"])
            .arg(format!("type=bind,source={},target=/work", dir.display()))
            .args(["-w", "/work", "--entrypoint", "pdftoppm", "texify:latest"])
            .args(["-png", "-f", "1", "-l", "1", "-scale-to-x", "420", "-scale-to-y", "-1", "-singlefile"])
            .arg(rel(pdf))
            .arg(rel(&prefix))
            .status()
            .context("pdftoppm not found locally and docker not available")?
    };
    if !status.success() {
        bail!("preview rendering failed for {}", pdf.display());
    }
    Ok(())
}

fn examples() -> Result<()> {
    let dir = root().join("examples");
    let pdf_dir = dir.join("pdf");
    let previews = dir.join("previews");
    std::fs::create_dir_all(&pdf_dir)?;
    std::fs::create_dir_all(&previews)?;
    let cfg = Config::load(&dir).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut sources: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.ends_with(".md") && n.chars().take(2).all(|c| c.is_ascii_digit()) && n.as_bytes().get(2) == Some(&b'-')
        })
        .collect();
    sources.sort();
    let mut rows = Vec::new();
    for src in &sources {
        let stem = src.file_stem().unwrap().to_string_lossy().into_owned();
        let r = build(
            src,
            BuildOptions {
                output: Some(pdf_dir.join(format!("{stem}.pdf"))),
                cfg: Some(cfg.clone()),
                ..Default::default()
            },
        )
        .map_err(|e| match e {
            texify::Error::Compile(c) => anyhow::anyhow!("{}: {}", src.display(), c.details()),
            other => anyhow::anyhow!("{}: {other}", src.display()),
        })?;
        let template = r.template.unwrap_or_default();
        println!("✓ {:36} [{template}]", src.file_name().unwrap().to_string_lossy());
        render_preview(&r.pdf, &previews.join(format!("{stem}.png")))?;
        rows.push((stem, template));
    }
    let parts: Vec<PathBuf> = BUNDLE.iter().map(|n| pdf_dir.join(format!("{n}.pdf"))).collect();
    let titles: Vec<String> = ["Cover page", "Cover letter", "Curriculum vitae"].map(String::from).to_vec();
    merge(&parts, &pdf_dir.join("application.pdf"), true, Some(&titles)).map_err(|e| anyhow::anyhow!("{e}"))?;
    println!("✓ application.pdf");
    write_gallery(&dir, &rows)?;
    println!("✓ examples/README.md");
    Ok(())
}

fn write_gallery(dir: &Path, rows: &[(String, String)]) -> Result<()> {
    let bundle = BUNDLE.map(|n| format!("{n}.md")).join(" ");
    let mut lines = vec![
        "# Examples".to_string(),
        String::new(),
        format!("{} document types, each generated from a short Markdown file.", rows.len()),
        "Sender and bank details come from [texify.toml](texify.toml), so no".into(),
        "document has to repeat them.".into(),
        String::new(),
        "```bash".into(),
        "cd examples".into(),
        "texify build 08-invoice.md            # -> pdf/08-invoice.pdf".into(),
        "cargo xtask examples                  # rebuild every example + this gallery".into(),
        "```".into(),
        String::new(),
        "**Job application bundle** from cover page, cover letter and CV, with bookmarks:".into(),
        String::new(),
        "```bash".into(),
        format!("texify build {bundle} -o pdf/application.pdf --bookmarks"),
        "```".into(),
        String::new(),
        "Result: [pdf/application.pdf](pdf/application.pdf)".into(),
        String::new(),
        "<!-- Generated by `cargo xtask examples` – do not edit by hand. -->".into(),
        String::new(),
        format!("|{}", " |".repeat(COLUMNS)),
        format!("|{}", "---|".repeat(COLUMNS)),
    ];
    for chunk in rows.chunks(COLUMNS) {
        let mut cells: Vec<String> = chunk
            .iter()
            .map(|(stem, template)| {
                format!(
                    "<a href=\"pdf/{stem}.pdf\"><img src=\"previews/{stem}.png\" width=\"240\"></a><br>**{}**<br>[`{stem}.md`]({stem}.md) · `{template}`",
                    type_of(stem)
                )
            })
            .collect();
        cells.resize(COLUMNS, " ".into());
        lines.push(format!("| {} |", cells.join(" | ")));
    }
    lines.extend([
        "".into(),
        "## Overview".into(),
        "".into(),
        "| Type | File | Template |".into(),
        "|---|---|---|".into(),
    ]);
    for (stem, template) in rows {
        lines.push(format!("| {} | [{stem}.md]({stem}.md) | `{template}` |", type_of(stem)));
    }
    std::fs::write(dir.join("README.md"), lines.join("\n") + "\n")?;
    Ok(())
}

// --------------------------------------------------------------------------
// third-party list
// --------------------------------------------------------------------------

fn rust_deps() -> Result<Vec<(String, String, String)>> {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--manifest-path"])
        .arg(root().join("Cargo.toml"))
        .output()
        .context("cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    let meta: Value = serde_json::from_slice(&out.stdout)?;
    let packages = meta["packages"].as_array().cloned().unwrap_or_default();
    let texify = packages.iter().find(|p| p["name"] == "texify").context("texify package")?;
    let mut rows = Vec::new();
    for dep in texify["dependencies"].as_array().into_iter().flatten() {
        if !dep["kind"].is_null() {
            continue; // dev/build dependencies
        }
        let name = dep["name"].as_str().unwrap_or_default().to_string();
        let license = packages
            .iter()
            .find(|p| p["name"] == name.as_str())
            .and_then(|p| p["license"].as_str())
            .unwrap_or("?")
            .to_string();
        rows.push((name.clone(), license, format!("https://crates.io/crates/{name}")));
    }
    rows.sort();
    Ok(rows)
}

fn latex_packages() -> Result<BTreeMap<String, Vec<String>>> {
    let re = regex::Regex::new(r"\\(?:usepackage|RequirePackage|documentclass)(?:\[[^\]]*\])?\{([^}]+)\}")?;
    let mut used: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let templates = root().join("assets").join("templates");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&templates)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    dirs.sort();
    for dir in dirs {
        let name = dir.file_name().unwrap().to_string_lossy().into_owned();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
        files.sort();
        for f in files.iter().filter(|f| f.extension().is_some_and(|e| e == "tex" || e == "sty")) {
            for cap in re.captures_iter(&std::fs::read_to_string(f)?) {
                for pkg in cap[1].split(',').map(str::trim) {
                    if pkg.is_empty() || pkg.contains('\\') || pkg == "texify" || pkg == "texify-letter" {
                        continue;
                    }
                    let users = used.entry(pkg.to_string()).or_default();
                    if !users.contains(&name) {
                        users.push(name.clone());
                    }
                }
            }
        }
    }
    Ok(used)
}

fn system_components() -> Result<Vec<String>> {
    let text = std::fs::read_to_string(root().join("Dockerfile"))?;
    let mut out = Vec::new();
    for cap in regex::Regex::new(r"(?m)^FROM\s+(\S+)")?.captures_iter(&text) {
        out.push(format!("Docker base image `{}`", &cap[1]));
    }
    let apt = regex::Regex::new(r"(?s)apt-get install[^\n]*\n(.*?)&&")?;
    if let Some(block) = apt.captures(&text) {
        let pkg = regex::Regex::new(r"(?m)^\s+([a-z0-9][a-z0-9.+-]+)\s*\\?$")?;
        out.extend(pkg.captures_iter(&block[1]).map(|c| format!("Debian package `{}`", &c[1])));
    }
    Ok(out)
}

fn third_party(check: bool) -> Result<()> {
    let mut lines = vec![
        "# Third-party components".to_string(),
        String::new(),
        "Generated by `cargo xtask third-party` – do not edit by hand.".into(),
        "Sources: `Cargo.toml` (via `cargo metadata`), all templates (`\\usepackage`), `Dockerfile`.".into(),
        String::new(),
        "## Rust crates".into(),
        String::new(),
        "| Crate | License | Project |".into(),
        "|---|---|---|".into(),
    ];
    for (name, license, url) in rust_deps()? {
        lines.push(format!("| {name} | {license} | {url} |"));
    }
    lines.extend([
        "".into(),
        "## LaTeX packages (TeX Live / CTAN)".into(),
        "".into(),
        "| Package | used in |".into(),
        "|---|---|".into(),
    ]);
    for (pkg, users) in latex_packages()? {
        lines.push(format!("| [{pkg}](https://ctan.org/pkg/{pkg}) | {} |", users.join(", ")));
    }
    lines.extend(["".into(), "## System".into(), "".into()]);
    lines.extend(system_components()?.into_iter().map(|c| format!("- {c}")));
    lines.push("- LaTeX engines (optional, local): Tectonic, TeX Live, MiKTeX".into());
    let content = lines.join("\n") + "\n";
    let path = root().join("THIRD_PARTY.md");
    if check {
        if std::fs::read_to_string(&path).unwrap_or_default() != content {
            bail!("THIRD_PARTY.md is outdated: cargo xtask third-party");
        }
        println!("THIRD_PARTY.md is up to date");
    } else {
        std::fs::write(&path, content)?;
        println!("THIRD_PARTY.md written");
    }
    Ok(())
}

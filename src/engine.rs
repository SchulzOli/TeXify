//! LaTeX engine detection and compilation.
//!
//! Supported: tectonic (single binary, fetches packages on demand), latexmk,
//! pdflatex/xelatex/lualatex (TeX Live / MiKTeX) and docker (texify image).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;

use regex::Regex;

use crate::config::Config;
use crate::error::{CompileError, Error, Result};
use crate::{bail, err};

pub const ENGINES: [&str; 6] = ["tectonic", "latexmk", "pdflatex", "xelatex", "lualatex", "docker"];
const AUX_SUFFIXES: [&str; 10] = ["aux", "out", "log", "toc", "fls", "fdb_latexmk", "gz", "xdv", "nav", "snm"];

fn docker_image(cfg: &Config) -> String {
    let image = cfg.str("build", "docker_image");
    if image.is_empty() { "texify:latest".into() } else { image }
}

fn docker_ready(image: &str) -> bool {
    which::which("docker").is_ok()
        && Command::new("docker")
            .args(["image", "inspect", image])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

/// Which engines are usable on this machine.
pub fn available(cfg: &Config) -> Vec<(&'static str, bool)> {
    ENGINES
        .iter()
        .map(|&e| (e, if e == "docker" { docker_ready(&docker_image(cfg)) } else { which::which(e).is_ok() }))
        .collect()
}

/// Engine to use: explicit choice > TEXIFY_ENGINE > config > first available.
pub fn resolve(cfg: &Config, preferred: Option<&str>) -> Result<String> {
    let choice = preferred
        .map(String::from)
        .or_else(|| std::env::var("TEXIFY_ENGINE").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| cfg.str("build", "engine"));
    if !choice.is_empty() && choice != "auto" {
        if !ENGINES.contains(&choice.as_str()) {
            bail!("Unknown engine '{choice}'. Allowed: auto, {}", ENGINES.join(", "));
        }
        return Ok(choice);
    }
    if let Some((engine, _)) = available(cfg).into_iter().find(|(_, ok)| *ok) {
        return Ok(engine.to_string());
    }
    bail!(
        "No LaTeX engine found. Options:\n  \
         • Tectonic (single binary):  https://tectonic-typesetting.github.io\n  \
         • TeX Live / MiKTeX (pdflatex, latexmk)\n  \
         • Docker:  docker compose build   (image 'texify:latest')\n\
         Check with: texify doctor"
    )
}

fn commands(engine: &str, tex: &Path, cfg: &Config) -> Vec<Vec<String>> {
    let name = tex.file_name().unwrap().to_string_lossy().into_owned();
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match engine {
        "tectonic" => vec![s(&["tectonic", "--keep-logs", "--chatter", "minimal", &name])],
        "latexmk" => vec![s(&["latexmk", "-pdf", "-interaction=nonstopmode", "-halt-on-error", &name])],
        "docker" => {
            let dir = tex.parent().unwrap().display().to_string();
            vec![s(&[
                "docker",
                "run",
                "--rm",
                "--mount",
                &format!("type=bind,source={dir},target=/work"),
                "-w",
                "/work",
                "--entrypoint",
                "latexmk",
                &docker_image(cfg),
                "-pdf",
                "-interaction=nonstopmode",
                "-halt-on-error",
                &name,
            ])]
        }
        // two passes for references / remember-picture overlays
        other => {
            let cmd = s(&[other, "-interaction=nonstopmode", "-halt-on-error", &name]);
            vec![cmd.clone(), cmd]
        }
    }
}

fn run(cmd: &[String], cwd: &Path) -> Result<std::process::Output> {
    Command::new(&cmd[0]).args(&cmd[1..]).current_dir(cwd).stdin(Stdio::null()).output().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => err!("Engine '{}' is not installed (texify doctor)", cmd[0]),
        _ => err!("{}: {e}", cmd[0]),
    })
}

/// Compile `tex` in its own folder; returns the PDF path or a compile error.
pub fn compile(tex: &Path, engine: &str, cfg: &Config) -> Result<PathBuf> {
    let tex = std::path::absolute(tex)?;
    let dir = tex.parent().unwrap().to_path_buf();
    let pdf = tex.with_extension("pdf");
    let _ = std::fs::remove_file(&pdf);
    let cmds = commands(engine, &tex, cfg);
    for cmd in &cmds {
        let out = run(cmd, &dir)?;
        if !out.status.success() {
            return Err(Error::Compile(compile_error(engine, &tex, &out)));
        }
    }
    if !["tectonic", "latexmk", "docker"].contains(&engine) {
        let log = std::fs::read_to_string(tex.with_extension("log")).unwrap_or_default();
        if log.contains("Rerun to get") {
            run(&cmds[0], &dir)?;
        }
    }
    if !pdf.is_file() {
        return Err(Error::Compile(CompileError {
            message: format!("{engine} finished but produced no PDF ({})", pdf.display()),
            log: String::new(),
            source: String::new(),
        }));
    }
    Ok(pdf)
}

static LINE_REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^l\.(\d+) (.*)$").unwrap());

fn compile_error(engine: &str, tex: &Path, out: &std::process::Output) -> CompileError {
    let log_text = std::fs::read_to_string(tex.with_extension("log")).unwrap_or_default();
    let lines: Vec<&str> = log_text.lines().collect();
    let (message, excerpt) = match lines.iter().position(|l| l.starts_with("! ")) {
        Some(i) => (lines[i][2..].trim().to_string(), lines[i..(i + 14).min(lines.len())].join("\n")),
        None => {
            let all = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let tail: Vec<&str> = all.trim().lines().collect();
            let msg =
                tail.iter().rev().find(|l| l.to_lowercase().contains("error")).unwrap_or(&"LaTeX error").to_string();
            (msg, tail[tail.len().saturating_sub(30)..].join("\n"))
        }
    };
    // "l.<n> <text>": only show source if line n of the main file really holds
    // <text> (the error may come from a package such as texify.sty)
    let mut source = String::new();
    if let Some(m) = LINE_REF.captures(&excerpt) {
        let n: usize = m[1].parse().unwrap_or(0);
        let snippet: String = m[2].replace("^^M", "").trim().trim_start_matches('.').chars().take(20).collect();
        if let Ok(src) = std::fs::read_to_string(tex) {
            let src: Vec<&str> = src.lines().collect();
            if n > 0 && n <= src.len() && !snippet.is_empty() && src[n - 1].contains(&snippet) {
                let (lo, hi) = (n.saturating_sub(5), (n + 3).min(src.len()));
                source = (lo..hi)
                    .map(|i| format!("{}{:5}| {}", if i + 1 == n { '>' } else { ' ' }, i + 1, src[i]))
                    .collect::<Vec<_>>()
                    .join("\n");
            }
        }
    }
    CompileError { message: format!("{engine}: {message}"), log: excerpt, source }
}

/// Remove auxiliary files next to an in-place compiled .tex file.
pub fn clean_aux(tex: &Path) {
    let (Some(dir), Some(stem)) = (tex.parent(), tex.file_stem()) else { return };
    let stem = stem.to_string_lossy();
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(rest) = name.strip_prefix(&format!("{stem}.")) {
            let ext = rest.rsplit('.').next().unwrap_or_default();
            if AUX_SUFFIXES.contains(&ext) || rest == "synctex.gz" {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

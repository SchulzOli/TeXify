//! Markdown -> LaTeX renderer (comrak AST, deterministic output).
//!
//! The output only relies on packages/macros provided by `texify.sty`.
//! Templates control how headings are rendered via a heading map, e.g. a CV maps
//! `## 2020 – 2022 | Master of Science` to `\cventry{2020 – 2022}{Master of Science}`.

use std::collections::BTreeMap;
use std::sync::Arc;

use comrak::nodes::{AstNode, ListType, NodeValue, TableAlignment};
use comrak::{Arena, Options, parse_document};
use serde_json::Value;

use crate::error::Result;
use crate::latex::{Lang, escape, escape_url};
use crate::{bail, err};

/// Table columns up to this many characters keep their natural width.
const SHORT_COLUMN: usize = 18;

/// How one (normalised) heading level is rendered.
#[derive(Debug, Clone)]
pub struct HeadingStyle {
    /// LaTeX command, emitted as `\cmd{arg1}{arg2}...`
    pub cmd: Option<String>,
    /// LaTeX environment; wraps everything up to the next heading of the same or a higher level
    pub env: Option<String>,
    /// number of arguments; the heading text is split by `split` into args
    pub args: usize,
    pub split: Option<String>,
    /// pad missing args at the start instead of the end
    pub pad_start: bool,
}

impl HeadingStyle {
    fn cmd(name: &str) -> Self {
        HeadingStyle { cmd: Some(name.into()), env: None, args: 1, split: None, pad_start: false }
    }

    pub fn from_spec(spec: &Value) -> Result<Self> {
        if let Some(s) = spec.as_str() {
            return Ok(HeadingStyle::cmd(s));
        }
        let Some(obj) = spec.as_object() else { bail!("Invalid heading style: {spec}") };
        for key in obj.keys() {
            if !["cmd", "env", "args", "split", "pad"].contains(&key.as_str()) {
                bail!("Unknown heading option: {key}");
            }
        }
        let style = HeadingStyle {
            cmd: obj.get("cmd").and_then(Value::as_str).map(String::from),
            env: obj.get("env").and_then(Value::as_str).map(String::from),
            args: obj.get("args").and_then(Value::as_u64).unwrap_or(1).max(1) as usize,
            split: obj.get("split").and_then(Value::as_str).map(String::from),
            pad_start: obj.get("pad").and_then(Value::as_str) == Some("start"),
        };
        if style.cmd.is_some() == style.env.is_some() {
            bail!("A heading style needs exactly one of 'cmd' or 'env'");
        }
        Ok(style)
    }
}

/// One top-level section of a document.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Section {
    pub title: String,
    pub raw_title: String,
    pub slug: String,
    pub body: String,
}

/// Copies a referenced file (image, logo) into the build and returns its build path.
pub type AssetFn = Arc<dyn Fn(&str) -> Result<String> + Send + Sync>;

#[derive(Clone)]
pub struct Renderer {
    lang: Lang,
    headings: Arc<BTreeMap<usize, HeadingStyle>>,
    asset: Option<AssetFn>,
}

fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.table = true;
    o.extension.strikethrough = true;
    o.parse.smart = true;
    o
}

pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn heading_offset<'a>(root: &'a AstNode<'a>) -> usize {
    root.descendants()
        .filter_map(|n| match &n.data.borrow().value {
            NodeValue::Heading(h) => Some(h.level as usize),
            _ => None,
        })
        .min()
        .map_or(0, |m| m - 1)
}

/// Raw source text of a heading (without `#` markers).
fn heading_raw<'a>(node: &'a AstNode<'a>, lines: &[&str]) -> String {
    let data = node.data.borrow();
    let line = lines.get(data.sourcepos.start.line.saturating_sub(1)).copied().unwrap_or("");
    let t = line.trim();
    match &data.value {
        NodeValue::Heading(h) if !h.setext => {
            let t = t.trim_start_matches('#').trim();
            // optional closing sequence: "## Title ##"
            let stripped = t.trim_end_matches('#');
            if stripped.len() < t.len() && (stripped.is_empty() || stripped.ends_with(' ')) {
                stripped.trim().to_string()
            } else {
                t.to_string()
            }
        }
        _ => t.to_string(),
    }
}

/// Raw Markdown of a table cell (formatting characters count towards the width).
fn raw_cell<'a>(cell: &'a AstNode<'a>, lines: &[&str]) -> String {
    let pos = cell.data.borrow().sourcepos;
    let line = lines.get(pos.start.line.saturating_sub(1)).copied().unwrap_or("");
    let (a, b) = (pos.start.column.saturating_sub(1), pos.end.column.min(line.len()));
    match line.get(a..b) {
        Some(raw) if pos.start.line == pos.end.line => raw.trim().trim_matches('|').trim().to_string(),
        _ => plain_text(cell),
    }
}

fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    node.descendants()
        .filter_map(|n| match &n.data.borrow().value {
            NodeValue::Text(t) => Some(t.to_string()),
            NodeValue::Code(c) => Some(c.literal.clone()),
            _ => None,
        })
        .collect()
}

impl Renderer {
    pub fn new(lang: Lang, headings: Option<&Value>, asset: Option<AssetFn>) -> Result<Self> {
        let headings = match headings.and_then(Value::as_object) {
            Some(map) if !map.is_empty() => {
                let mut out = BTreeMap::new();
                for (k, v) in map {
                    let level: usize = k.parse().map_err(|_| err!("Heading level must be a number: {k}"))?;
                    out.insert(level, HeadingStyle::from_spec(v)?);
                }
                out
            }
            _ => BTreeMap::from([
                (1, HeadingStyle::cmd("section")),
                (2, HeadingStyle::cmd("subsection")),
                (3, HeadingStyle::cmd("subsubsection")),
                (4, HeadingStyle::cmd("paragraph")),
            ]),
        };
        Ok(Renderer { lang, headings: Arc::new(headings), asset })
    }

    // ------------------------------------------------------------------ API

    /// Render a full Markdown document body.
    pub fn render(&self, text: &str) -> Result<String> {
        self.render_with_base(text, 1)
    }

    /// Render one line of inline Markdown (`**bold**`, `*em*`, `` `code` ``, links).
    pub fn render_inline(&self, text: &str) -> Result<String> {
        let joined = text.trim().lines().map(str::trim).collect::<Vec<_>>().join(" ");
        if joined.is_empty() {
            return Ok(String::new());
        }
        let arena = Arena::new();
        let root = parse_document(&arena, &joined, &options());
        let mut out = String::new();
        for block in root.children() {
            if matches!(block.data.borrow().value, NodeValue::Paragraph) {
                out.push_str(&self.inline_children(block)?);
            } else {
                // e.g. "1. item" or "# x" in a field: keep it as plain text
                return Ok(escape(&joined));
            }
        }
        Ok(out)
    }

    /// Split a document at its top-level headings and render each part.
    pub fn sections(&self, text: &str) -> Result<Vec<Section>> {
        let arena = Arena::new();
        let root = parse_document(&arena, text, &options());
        let offset = heading_offset(root);
        let lines: Vec<&str> = text.lines().collect();
        let mut cuts = Vec::new();
        for node in root.children() {
            let data = node.data.borrow();
            if let NodeValue::Heading(h) = &data.value {
                if h.level as usize - offset == 1 {
                    let raw = heading_raw(node, &lines);
                    cuts.push((data.sourcepos.start.line - 1, data.sourcepos.end.line, raw));
                }
            }
        }
        let mut result = Vec::new();
        let mut start = 0;
        let mut title = String::new();
        cuts.push((lines.len(), lines.len(), String::new()));
        for (h_start, h_end, h_text) in cuts {
            let chunk = lines[start.min(h_start)..h_start].join("\n");
            if !chunk.trim().is_empty() || !title.is_empty() {
                result.push(Section {
                    title: self.render_inline(&title)?,
                    raw_title: title.clone(),
                    slug: slug(&title),
                    body: self.render_with_base(&chunk, 2)?,
                });
            }
            start = h_end;
            title = h_text;
        }
        Ok(result)
    }

    /// If the document starts with a level-1 heading, split it off as title.
    pub fn extract_title(&self, text: &str) -> (Option<String>, String) {
        let arena = Arena::new();
        let root = parse_document(&arena, text, &options());
        let lines: Vec<&str> = text.lines().collect();
        if let Some(first) = root.first_child() {
            let data = first.data.borrow();
            if let NodeValue::Heading(h) = &data.value {
                if h.level == 1 {
                    let raw = heading_raw(first, &lines);
                    let (a, b) = (data.sourcepos.start.line - 1, data.sourcepos.end.line);
                    let rest: Vec<&str> =
                        lines[..a].iter().chain(lines[b.min(lines.len())..].iter()).copied().collect();
                    return (Some(raw), rest.join("\n"));
                }
            }
        }
        (None, text.to_string())
    }

    // ------------------------------------------------------------ internals

    fn render_with_base(&self, text: &str, base: usize) -> Result<String> {
        let arena = Arena::new();
        let root = parse_document(&arena, text, &options());
        let lines: Vec<&str> = text.lines().collect();
        let offset = heading_offset(root) as isize - (base as isize - 1);
        let mut out = String::new();
        let mut open_envs: Vec<(usize, String)> = Vec::new();
        for node in root.children() {
            let level = match &node.data.borrow().value {
                NodeValue::Heading(h) => Some((h.level as isize - offset).max(1) as usize),
                _ => None,
            };
            match level {
                Some(level) => {
                    while open_envs.last().is_some_and(|(l, _)| *l >= level) {
                        let (_, env) = open_envs.pop().unwrap();
                        out.push_str(&format!("\\end{{{env}}}\n"));
                    }
                    out.push_str(&self.heading(node, level, &lines, &mut open_envs)?);
                }
                None => out.push_str(&self.block(node, &lines)?),
            }
        }
        while let Some((_, env)) = open_envs.pop() {
            out.push_str(&format!("\\end{{{env}}}\n"));
        }
        Ok(format!("{}\n", out.trim()))
    }

    fn style(&self, level: usize) -> &HeadingStyle {
        if let Some(s) = self.headings.get(&level) {
            return s;
        }
        self.headings
            .range(..=level)
            .next_back()
            .or_else(|| self.headings.iter().next())
            .map(|(_, s)| s)
            .expect("heading map is never empty")
    }

    fn heading<'a>(
        &self,
        node: &'a AstNode<'a>,
        level: usize,
        lines: &[&str],
        open_envs: &mut Vec<(usize, String)>,
    ) -> Result<String> {
        let style = self.style(level);
        let raw = heading_raw(node, lines);
        let mut parts: Vec<String> = match &style.split {
            Some(sep) => raw.split(sep.as_str()).map(|p| p.trim().to_string()).collect(),
            None => vec![raw.trim().to_string()],
        };
        if parts.len() > style.args {
            // too many separators: merge the tail
            let sep = format!(" {} ", style.split.as_deref().unwrap_or(""));
            let tail = parts.split_off(style.args - 1).join(&sep);
            parts.push(tail);
        }
        while parts.len() < style.args {
            if style.pad_start {
                parts.insert(0, String::new());
            } else {
                parts.push(String::new());
            }
        }
        let mut args = String::new();
        for p in &parts {
            args.push('{');
            args.push_str(&self.render_inline(p)?);
            args.push('}');
        }
        if let Some(env) = &style.env {
            open_envs.push((level, env.clone()));
            return Ok(format!("\\begin{{{env}}}{args}\n"));
        }
        Ok(format!("\\{}{args}\n\n", style.cmd.as_deref().unwrap_or("section")))
    }

    fn block<'a>(&self, node: &'a AstNode<'a>, lines: &[&str]) -> Result<String> {
        let value = node.data.borrow().value.clone();
        Ok(match value {
            NodeValue::Paragraph => {
                let kids: Vec<_> = node.children().collect();
                if kids.len() == 1 && matches!(kids[0].data.borrow().value, NodeValue::Image(_)) {
                    format!("\\begin{{center}}\n{}\n\\end{{center}}\n\n", self.inline(kids[0])?)
                } else {
                    let content = self.inline_children(node)?;
                    if self.is_tight(node) { format!("{content}\n") } else { format!("{content}\n\n") }
                }
            }
            NodeValue::List(list) => {
                let env = if list.list_type == ListType::Bullet { "itemize" } else { "enumerate" };
                let opts = if list.list_type == ListType::Ordered && list.start != 1 {
                    format!("[start={}]", list.start)
                } else {
                    String::new()
                };
                let mut items = String::new();
                for child in node.children() {
                    items.push_str(&self.block(child, lines)?);
                }
                format!("\\begin{{{env}}}{opts}\n{items}\\end{{{env}}}\n\n")
            }
            NodeValue::Item(_) => {
                let mut body = String::new();
                for child in node.children() {
                    body.push_str(&self.block(child, lines)?);
                }
                format!("  \\item {}\n", body.trim())
            }
            NodeValue::BlockQuote => {
                let mut body = String::new();
                for child in node.children() {
                    body.push_str(&self.block(child, lines)?);
                }
                format!("\\begin{{quote}}\n{}\n\\end{{quote}}\n\n", body.trim())
            }
            NodeValue::CodeBlock(cb) => {
                let info = cb.info.trim();
                if info == "{=latex}" || info == "=latex" {
                    format!("{}\n\n", cb.literal.trim_end())
                } else {
                    format!(
                        "\\begin{{Verbatim}}[fontsize=\\small]\n{}\n\\end{{Verbatim}}\n\n",
                        cb.literal.trim_end_matches('\n')
                    )
                }
            }
            NodeValue::ThematicBreak => "\\texifyrule\n\n".into(),
            NodeValue::Table(t) => self.table(node, &t.alignments, lines)?,
            NodeValue::HtmlBlock(h) => format!("{}\n\n", escape(&h.literal)),
            NodeValue::Heading(_) => format!("\\textbf{{{}}}\n\n", self.inline_children(node)?),
            _ => {
                let mut out = String::new();
                for child in node.children() {
                    out.push_str(&self.block(child, lines)?);
                }
                out
            }
        })
    }

    fn is_tight<'a>(&self, para: &'a AstNode<'a>) -> bool {
        let Some(item) = para.parent() else { return false };
        if !matches!(item.data.borrow().value, NodeValue::Item(_)) {
            return false;
        }
        item.parent().is_some_and(|list| matches!(&list.data.borrow().value, NodeValue::List(l) if l.tight))
    }

    fn table<'a>(&self, node: &'a AstNode<'a>, alignments: &[TableAlignment], lines: &[&str]) -> Result<String> {
        let rows: Vec<(bool, Vec<&'a AstNode<'a>>)> = node
            .children()
            .filter_map(|r| match r.data.borrow().value {
                NodeValue::TableRow(header) => Some((header, r.children().collect())),
                _ => None,
            })
            .collect();
        if rows.is_empty() {
            return Ok(String::new());
        }
        let ncols = alignments.len();
        let mut widths = vec![0usize; ncols];
        for (_, cells) in &rows {
            for (i, cell) in cells.iter().take(ncols).enumerate() {
                widths[i] = widths[i].max(raw_cell(cell, lines).chars().count());
            }
        }
        let mut kinds: Vec<String> = alignments
            .iter()
            .enumerate()
            .map(|(i, a)| match a {
                TableAlignment::Right => "r".into(),
                TableAlignment::Center => "c".into(),
                _ if widths[i] <= SHORT_COLUMN => "l".into(),
                _ => "X".into(),
            })
            .collect();
        if !kinds.iter().any(|k| k == "X") {
            // let the widest text column fill the line
            // first of the widest columns (stable for ties)
            if let Some(i) = (0..ncols).filter(|&i| kinds[i] == "l").rev().max_by_key(|&i| widths[i]) {
                kinds[i] = "X".into();
            }
        }
        let flex: Vec<usize> = (0..ncols).filter(|&i| kinds[i] == "X").collect();
        if flex.len() == 1 {
            kinds[flex[0]] = "L".into();
        } else if !flex.is_empty() {
            // share the remaining width by (damped) content length
            let weights: Vec<f64> = flex.iter().map(|&i| (widths[i].max(1) as f64).sqrt()).collect();
            let total: f64 = weights.iter().sum();
            for (j, &i) in flex.iter().enumerate() {
                kinds[i] = format!("Y{{{:.2}}}", flex.len() as f64 * weights[j] / total);
            }
        }
        let has_x = !flex.is_empty();
        let spec = format!("@{{}}{}@{{}}", kinds.join(" "));
        let mut lines = Vec::new();
        for (is_head, cells) in &rows {
            let mut rendered = Vec::new();
            for cell in cells {
                let c = self.inline_children(cell)?;
                rendered.push(if *is_head && !c.is_empty() { format!("\\textbf{{{c}}}") } else { c });
            }
            lines.push(format!("{} \\\\", rendered.join(" & ")));
            if *is_head {
                lines.push("\\midrule".into());
            }
        }
        let (begin, end) = if has_x {
            (format!("\\begin{{tabularx}}{{\\linewidth}}{{{spec}}}"), "\\end{tabularx}")
        } else {
            (format!("\\begin{{tabular}}{{{spec}}}"), "\\end{tabular}")
        };
        Ok(format!(
            "\\par\\medskip\\noindent\n{begin}\n\\toprule\n{}\n\\bottomrule\n{end}\n\\par\\medskip\n\n",
            lines.join("\n")
        ))
    }

    fn inline_children<'a>(&self, node: &'a AstNode<'a>) -> Result<String> {
        let mut out = String::new();
        for child in node.children() {
            out.push_str(&self.inline(child)?);
        }
        Ok(out)
    }

    fn text(&self, t: &str) -> String {
        let t = match self.lang {
            // smart punctuation yields English quotes; German uses „…“
            Lang::De => t.replace('\u{201c}', "\u{201e}").replace('\u{201d}', "\u{201c}"),
            Lang::En => t.to_string(),
        };
        escape(&t)
    }

    fn inline<'a>(&self, node: &'a AstNode<'a>) -> Result<String> {
        let value = node.data.borrow().value.clone();
        Ok(match value {
            NodeValue::Text(t) => self.text(&t),
            NodeValue::SoftBreak => "\n".into(),
            NodeValue::LineBreak => "\\\\\n".into(),
            NodeValue::Strong => format!("\\textbf{{{}}}", self.inline_children(node)?),
            NodeValue::Emph => format!("\\emph{{{}}}", self.inline_children(node)?),
            NodeValue::Strikethrough => format!("\\sout{{{}}}", self.inline_children(node)?),
            NodeValue::Code(c) => format!("\\texttt{{{}}}", escape(&c.literal)),
            NodeValue::Link(l) => format!("\\href{{{}}}{{{}}}", escape_url(&l.url), self.inline_children(node)?),
            NodeValue::Image(l) => {
                let path = match &self.asset {
                    Some(f) => f(&l.url)?,
                    None => l.url.clone(),
                };
                format!("\\texifyimage{{{path}}}")
            }
            NodeValue::HtmlInline(h) => escape(&h),
            _ => self.inline_children(node)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn r(lang: Lang) -> Renderer {
        Renderer::new(lang, None, None).unwrap()
    }

    #[test]
    fn inline_markup_and_quotes() {
        let out = r(Lang::En).render_inline("**bold** *em* `a_b` [x](https://e.de/a#b) ~~gone~~").unwrap();
        assert_eq!(out, r"\textbf{bold} \emph{em} \texttt{a\_b} \href{https://e.de/a\#b}{x} \sout{gone}");
        assert_eq!(r(Lang::De).render_inline("\"Hallo\"").unwrap(), "„Hallo“");
        assert_eq!(r(Lang::En).render_inline("\"Hi\"").unwrap(), "“Hi”");
    }

    #[test]
    fn headings_are_normalised_and_mapped() {
        assert!(r(Lang::En).render("## A\n\n### B\n").unwrap().contains("\\section{A}"));
        let map = json!({"1": "cvsection", "2": {"cmd": "cventry", "args": 2, "split": "|", "pad": "start"}});
        let rr = Renderer::new(Lang::En, Some(&map), None).unwrap();
        let out = rr.render("# Work\n\n## 2020 – 2022 | Master\n\n## No date\n\n## a | b | c\n").unwrap();
        assert!(out.contains("\\cvsection{Work}"));
        assert!(out.contains("\\cventry{2020 – 2022}{Master}"));
        assert!(out.contains("\\cventry{}{No date}"));
        assert!(out.contains("\\cventry{a}{b | c}"));
    }

    #[test]
    fn env_headings_wrap_content() {
        let map = json!({"1": {"env": "box"}});
        let out = Renderer::new(Lang::En, Some(&map), None).unwrap().render("# One\ntext\n# Two\nmore\n").unwrap();
        assert!(out.find("\\end{box}").unwrap() < out.find("\\begin{box}{Two}").unwrap());
        assert!(out.trim_end().ends_with("\\end{box}"));
    }

    #[test]
    fn lists_tables_raw_latex() {
        let out = r(Lang::En)
            .render(
                "- a\n- b\n\n3. x\n4. y\n\n| Name | Amount |\n|---|--:|\n| A | 1 |\n\n```{=latex}\n\\newpage\n```\n",
            )
            .unwrap();
        assert!(out.contains("\\begin{itemize}\n  \\item a\n  \\item b\n\\end{itemize}"));
        assert!(out.contains("\\begin{enumerate}[start=3]"));
        assert!(out.contains("\\begin{tabularx}{\\linewidth}{@{}L r@{}}"));
        assert!(out.contains("\\newpage"));
        let md =
            format!("| Nr | Description | Note |\n|---|---|---|\n| 1 | {} | {} |\n", "x".repeat(80), "y".repeat(30));
        assert!(r(Lang::En).render(&md).unwrap().contains("{@{}l Y{"));
    }

    #[test]
    fn sections_and_title() {
        let rr = r(Lang::En);
        let (title, rest) = rr.extract_title("# Title\n\nText\n\n## Part\nContent\n");
        assert_eq!(title.as_deref(), Some("Title"));
        let secs = rr.sections(&rest).unwrap();
        assert_eq!(secs.iter().map(|s| s.raw_title.as_str()).collect::<Vec<_>>(), ["", "Part"]);
        assert!(secs[1].body.contains("Content"));
    }
}

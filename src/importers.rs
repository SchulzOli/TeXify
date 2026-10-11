//! Import existing documents (PDF, DOCX, TXT) as Markdown.
//!
//! Deterministic extraction only; restructuring the result for a specific
//! template is an AI task (`texify ai fill cv old_cv.md` or the MCP prompt).

use std::io::Read;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

use crate::build::ext_of;
use crate::error::Result;
use crate::pdftools::extract_text;
use crate::{bail, err};

fn docx_to_markdown(path: &Path) -> Result<String> {
    let file = std::fs::File::open(path)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| err!("{}: not a DOCX file: {e}", path.display()))?;
    let mut xml = String::new();
    zip.by_name("word/document.xml").map_err(|e| err!("{}: {e}", path.display()))?.read_to_string(&mut xml)?;

    let mut reader = Reader::from_str(&xml);
    let mut out: Vec<String> = Vec::new();
    let mut para = String::new();
    let mut style = String::new();
    let mut in_text = false;
    let mut table_depth = 0usize;
    let mut row: Vec<String> = Vec::new();
    let mut rows: Vec<Vec<String>> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match e.local_name().as_ref() {
                b"t" => in_text = true,
                b"tab" => para.push(' '),
                b"br" => para.push(' '),
                b"pStyle" => {
                    for a in e.attributes().flatten() {
                        if a.key.local_name().as_ref() == b"val" {
                            style = String::from_utf8_lossy(&a.value).to_lowercase();
                        }
                    }
                }
                b"tbl" => table_depth += 1,
                _ => {}
            },
            Ok(Event::Text(t)) if in_text => para.push_str(&t.decode().unwrap_or_default()),
            Ok(Event::GeneralRef(r)) if in_text => {
                let name = String::from_utf8_lossy(&r);
                para.push_str(match name.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
            }
            Ok(Event::End(e)) => match e.local_name().as_ref() {
                b"t" => in_text = false,
                b"p" => {
                    let text = std::mem::take(&mut para).trim().to_string();
                    if table_depth > 0 {
                        row.push(text.replace('|', "/"));
                        // several paragraphs in one cell: join them
                        if row.len() > 1 && text.is_empty() {
                            row.pop();
                        }
                    } else if text.is_empty() {
                        out.push(String::new());
                    } else if let Some(level) =
                        style.strip_prefix("heading").and_then(|n| n.trim().parse::<usize>().ok())
                    {
                        out.extend([String::new(), format!("{} {text}", "#".repeat(level.clamp(1, 4))), String::new()]);
                    } else if style.contains("list") {
                        out.push(format!("- {text}"));
                    } else {
                        out.extend([text, String::new()]);
                    }
                    style.clear();
                }
                b"tc" => {
                    // merge the paragraphs collected for this cell
                    let cell = row.join(" ");
                    row.clear();
                    rows.last_mut().map(|r| r.push(cell.clone())).unwrap_or_else(|| rows.push(vec![cell]));
                }
                b"tr" => rows.push(Vec::new()),
                b"tbl" => {
                    table_depth -= 1;
                    let table: Vec<Vec<String>> =
                        std::mem::take(&mut rows).into_iter().filter(|r| !r.is_empty()).collect();
                    if let Some(head) = table.first() {
                        out.push(String::new());
                        out.push(format!("| {} |", head.join(" | ")));
                        out.push(format!("|{}", "---|".repeat(head.len())));
                        for r in &table[1..] {
                            out.push(format!("| {} |", r.join(" | ")));
                        }
                        out.push(String::new());
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(e) => bail!("{}: invalid DOCX XML: {e}", path.display()),
            _ => {}
        }
    }
    let text = out.join("\n");
    let collapsed = regex::Regex::new(r"\n{3,}").unwrap().replace_all(&text, "\n\n");
    Ok(format!("{}\n", collapsed.trim()))
}

fn pdf_to_markdown(path: &Path) -> Result<String> {
    let text = extract_text(path)?;
    if text.trim().is_empty() {
        bail!("{}: no text layer found (scanned?). Run OCR first.", path.display());
    }
    // A line continues the previous one only if that one did not end a sentence
    // and this one starts in lower case (hard-wrapped paragraph); otherwise it
    // is its own paragraph (headings, address lines, table rows, ...).
    let mut paragraphs: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let continues = paragraphs.last().is_some_and(|prev| {
            !prev.ends_with(['.', ':', '!', '?']) && line.chars().next().is_some_and(char::is_lowercase)
        });
        if continues {
            let prev = paragraphs.last_mut().unwrap();
            prev.push(' ');
            prev.push_str(line);
        } else {
            paragraphs.push(line.to_string());
        }
    }
    Ok(format!("{}\n", paragraphs.join("\n\n")))
}

/// Convert a PDF/DOCX/TXT file into a Markdown document (MD files are returned unchanged).
pub fn to_markdown(path: &Path, template: Option<&str>) -> Result<String> {
    let body = match ext_of(path).as_str() {
        "pdf" => pdf_to_markdown(path)?,
        "docx" => docx_to_markdown(path)?,
        "md" | "markdown" => return Ok(std::fs::read_to_string(path)?.trim_start_matches('\u{feff}').to_string()),
        "txt" => std::fs::read_to_string(path)?.trim_start_matches('\u{feff}').to_string(),
        "" => bail!("Cannot import files without extension (supported: pdf, docx, txt, md)"),
        other => bail!("Cannot import .{other} (supported: pdf, docx, txt, md)"),
    };
    let title = path.file_stem().unwrap_or_default().to_string_lossy().replace(['_', '-'], " ").trim().to_string();
    Ok(format!(
        "---\ntemplate: {}\ntitle: {}\n---\n\n{body}",
        template.unwrap_or("article"),
        serde_json::to_string(&title).unwrap_or_default()
    ))
}

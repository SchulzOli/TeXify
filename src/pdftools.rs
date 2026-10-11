//! PDF organisation: merge (formerly OnePDF), page selection, info, text.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lopdf::{Bookmark, Dictionary, Document, Object, ObjectId};

use crate::error::Result;
use crate::{bail, err};

const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Resolve files, folders and glob patterns into an ordered list of paths.
/// Folders contribute their files (matching `exts`) sorted by name.
pub fn collect_inputs(inputs: &[String], exts: &[&str]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for item in inputs {
        let p = Path::new(item);
        if p.is_dir() {
            let mut found: Vec<PathBuf> = std::fs::read_dir(p)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|f| f.is_file() && exts.contains(&ext(f).as_str()))
                .collect();
            found.sort_by_key(|f| f.file_name().map(|n| n.to_string_lossy().to_lowercase()));
            files.extend(found);
        } else if item.contains(['*', '?', '[']) {
            let mut matches = glob_paths(item)?;
            if matches.is_empty() {
                bail!("No matches for '{item}'");
            }
            matches.sort_by_key(|f| f.to_string_lossy().to_lowercase());
            files.extend(matches);
        } else if p.is_file() {
            files.push(p.to_path_buf());
        } else {
            bail!("Not found: {item}");
        }
    }
    Ok(files)
}

fn ext(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Minimal glob: wildcards in the last path component (`scans/*.pdf`).
fn glob_paths(pattern: &str) -> Result<Vec<PathBuf>> {
    let p = Path::new(pattern);
    let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if dir.to_string_lossy().contains(['*', '?', '[']) {
        bail!("Wildcards are only supported in the file name: {pattern}");
    }
    let mut re = String::from("(?i)^");
    for c in name.chars() {
        match c {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            c => re.push_str(&regex::escape(&c.to_string())),
        }
    }
    re.push('$');
    let re = regex::Regex::new(&re).map_err(|e| err!("{e}"))?;
    Ok(std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && re.is_match(&e.file_name().to_string_lossy()))
        .map(|e| if dir == Path::new(".") { PathBuf::from(e.file_name()) } else { e.path() })
        .collect())
}

pub fn sort_paths(mut files: Vec<PathBuf>, mode: &str) -> Vec<PathBuf> {
    match mode {
        "name" => files.sort_by_key(|f| f.file_name().map(|n| n.to_string_lossy().to_lowercase())),
        "date" => files.sort_by_key(|f| f.metadata().and_then(|m| m.modified()).ok()),
        _ => {}
    }
    files
}

fn open(path: &Path) -> Result<Document> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut doc = Document::load(path).map_err(|e| err!("Cannot read PDF {name}: {e}"))?;
    if doc.is_encrypted() && doc.decrypt("").is_err() {
        bail!("PDF is password protected: {name}");
    }
    Ok(doc)
}

/// Page dictionary with inherited attributes (Resources, MediaBox, …) made explicit.
fn standalone_page(doc: &Document, page_id: ObjectId) -> Option<Dictionary> {
    let mut page = doc.get_object(page_id).ok()?.as_dict().ok()?.clone();
    let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
    while let Some(pid) = parent {
        let Ok(node) = doc.get_object(pid).and_then(Object::as_dict) else { break };
        for key in INHERITABLE {
            if !page.has(key) {
                if let Ok(v) = node.get(key) {
                    page.set(key.to_vec(), v.clone());
                }
            }
        }
        parent = node.get(b"Parent").and_then(Object::as_reference).ok();
    }
    Some(page)
}

/// Merge PDFs in order; returns `[(file, pages)]`. `titles` override bookmark labels.
pub fn merge(
    files: &[PathBuf],
    output: &Path,
    bookmarks: bool,
    titles: Option<&[String]>,
) -> Result<Vec<(PathBuf, usize)>> {
    let out_abs = std::path::absolute(output)?;
    let files: Vec<&PathBuf> =
        files.iter().filter(|f| std::path::absolute(f).ok().as_ref() != Some(&out_abs)).collect();
    if files.is_empty() {
        bail!("No PDF files to merge");
    }
    let mut merged = Document::with_version("1.5");
    let mut max_id = 1;
    let mut pages: Vec<(ObjectId, Dictionary)> = Vec::new();
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut report = Vec::new();
    for (i, path) in files.iter().enumerate() {
        let mut doc = open(path)?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        let doc_pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        if bookmarks {
            if let Some(first) = doc_pages.first() {
                let label = titles
                    .and_then(|t| t.get(i).cloned())
                    .unwrap_or_else(|| path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
                merged.add_bookmark(Bookmark::new(label, [0.0, 0.0, 0.0], 0, *first), None);
            }
        }
        for id in &doc_pages {
            if let Some(dict) = standalone_page(&doc, *id) {
                pages.push((*id, dict));
            }
        }
        report.push(((*path).clone(), doc_pages.len()));
        objects.extend(doc.objects);
    }

    let pages_id = (max_id, 0);
    let catalog_id = (max_id + 1, 0);
    for (id, obj) in &objects {
        match obj.type_name().unwrap_or(b"") {
            b"Catalog" | b"Pages" | b"Page" | b"Outlines" | b"Outline" => {}
            _ => {
                merged.objects.insert(*id, obj.clone());
            }
        }
    }
    let mut kids = Vec::new();
    for (id, mut dict) in pages {
        dict.set("Parent", pages_id);
        merged.objects.insert(id, Object::Dictionary(dict));
        kids.push(Object::Reference(id));
    }
    let mut pages_dict = Dictionary::new();
    pages_dict.set("Type", "Pages");
    pages_dict.set("Count", kids.len() as i64);
    pages_dict.set("Kids", kids);
    merged.objects.insert(pages_id, Object::Dictionary(pages_dict));
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", pages_id);
    merged.objects.insert(catalog_id, Object::Dictionary(catalog));
    merged.trailer.set("Root", catalog_id);
    merged.max_id = max_id + 1;
    merged.renumber_objects();
    merged.adjust_zero_pages();
    if let Some(outline) = merged.build_outline() {
        let root = merged.trailer.get(b"Root").and_then(Object::as_reference).ok();
        if let Some(Ok(Object::Dictionary(dict))) = root.map(|r| merged.get_object_mut(r)) {
            dict.set("Outlines", Object::Reference(outline));
            dict.set("PageMode", "UseOutlines");
        }
    }
    merged.compress();
    if let Some(p) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(p)?;
    }
    merged.save(output).map_err(|e| err!("Cannot write {}: {e}", output.display()))?;
    Ok(report)
}

/// `"1-3,5,8-"` -> 1-based page numbers (inclusive ranges).
pub fn parse_ranges(spec: &str, total: u32) -> Result<Vec<u32>> {
    let mut pages = Vec::new();
    for part in spec.replace(' ', "").split(',').filter(|p| !p.is_empty()) {
        let (lo, hi) = match part.split_once('-') {
            Some((a, b)) => (
                if a.is_empty() { 1 } else { a.parse().map_err(|_| err!("Invalid page range: {part:?}"))? },
                if b.is_empty() { total } else { b.parse().map_err(|_| err!("Invalid page range: {part:?}"))? },
            ),
            None => {
                let n = part.parse().map_err(|_| err!("Invalid page range: {part:?}"))?;
                (n, n)
            }
        };
        if !(1 <= lo && lo <= hi && hi <= total) {
            bail!("Page range {part:?} outside 1–{total}");
        }
        pages.extend(lo..=hi);
    }
    if pages.is_empty() {
        bail!("No pages selected");
    }
    Ok(pages)
}

/// Write the selected pages (in the given order) to `output`; returns the page count.
pub fn select_pages(source: &Path, spec: &str, output: &Path) -> Result<usize> {
    let mut doc = open(source)?;
    let all = doc.get_pages();
    let wanted = parse_ranges(spec, all.len() as u32)?;
    let catalog_id = doc.trailer.get(b"Root").and_then(Object::as_reference).map_err(|e| err!("{e}"))?;
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for n in wanted {
        let id = all[&n];
        if !seen.insert(id) {
            continue; // the same page object may appear only once
        }
        if let Some(mut dict) = standalone_page(&doc, id) {
            dict.set("Parent", pages_id);
            doc.objects.insert(id, Object::Dictionary(dict));
            kids.push(Object::Reference(id));
        }
    }
    let count = kids.len();
    let mut pages = Dictionary::new();
    pages.set("Type", "Pages");
    pages.set("Count", count as i64);
    pages.set("Kids", kids);
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    if let Ok(Object::Dictionary(cat)) = doc.get_object_mut(catalog_id) {
        cat.set("Pages", pages_id);
        cat.remove(b"Outlines");
    }
    doc.prune_objects();
    doc.compress();
    if let Some(p) = output.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(p)?;
    }
    doc.save(output).map_err(|e| err!("Cannot write {}: {e}", output.display()))?;
    Ok(count)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PdfInfo {
    pub file: String,
    pub pages: usize,
    pub title: Option<String>,
    pub author: Option<String>,
    pub producer: Option<String>,
    pub page_size_mm: Option<(u32, u32)>,
    pub bytes: u64,
}

fn info_string(doc: &Document, key: &[u8]) -> Option<String> {
    let info = doc.trailer.get(b"Info").ok()?;
    let dict = match info {
        Object::Reference(id) => doc.get_object(*id).ok()?.as_dict().ok()?,
        Object::Dictionary(d) => d,
        _ => return None,
    };
    let obj = dict.get(key).ok()?;
    let s = lopdf::decode_text_string(obj).ok()?;
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

pub fn info(path: &Path) -> Result<PdfInfo> {
    let doc = open(path)?;
    let pages = doc.get_pages();
    let size = pages.values().next().and_then(|id| standalone_page(&doc, *id)).and_then(|p| {
        let mb = p.get(b"MediaBox").ok()?.as_array().ok()?.clone();
        let num = |o: &Object| o.as_float().ok().or_else(|| o.as_i64().ok().map(|i| i as f32));
        let (x0, y0, x1, y1) = (num(&mb[0])?, num(&mb[1])?, num(&mb[2])?, num(&mb[3])?);
        let mm = |pt: f32| (pt * 25.4 / 72.0).round() as u32;
        Some((mm(x1 - x0), mm(y1 - y0)))
    });
    Ok(PdfInfo {
        file: path.display().to_string(),
        pages: pages.len(),
        title: info_string(&doc, b"Title"),
        author: info_string(&doc, b"Author"),
        producer: info_string(&doc, b"Producer"),
        page_size_mm: size,
        bytes: std::fs::metadata(path)?.len(),
    })
}

pub fn extract_text(path: &Path) -> Result<String> {
    let doc = open(path)?;
    let numbers: Vec<u32> = doc.get_pages().keys().copied().collect();
    let mut out = Vec::new();
    for n in numbers {
        out.push(doc.extract_text(&[n]).unwrap_or_default().trim().to_string());
    }
    Ok(out.join("\n\n"))
}

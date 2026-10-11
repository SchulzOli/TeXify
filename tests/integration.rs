//! End-to-end tests of the library (rendering, config, PDF tools, AI command).
//! LaTeX compile tests run only when an engine is available.

use std::path::{Path, PathBuf};

use lopdf::{Document, Object, Stream, dictionary};
use texify::build::{BuildOptions, build, render_to_string};
use texify::config::Config;
use texify::pdftools::{info, merge, parse_ranges, select_pages};

fn isolate() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: all tests set the same kind of value; only read by texify_home()
    unsafe { std::env::set_var("TEXIFY_HOME", home.path().join("home")) };
    home
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    p
}

fn builtin_examples() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/templates");
    let mut out: Vec<PathBuf> = std::fs::read_dir(root)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .map(|p| p.join("example.md"))
        .collect();
    out.sort();
    out
}

#[test]
fn every_builtin_example_renders() {
    let _home = isolate();
    let usepackage = regex::Regex::new(r"\\usepackage\[(de|en),(sans|serif)\]\{texify\}").unwrap();
    for example in builtin_examples() {
        let tex = render_to_string(&example, None, None).unwrap_or_else(|e| panic!("{}: {e}", example.display()));
        assert!(usepackage.is_match(&tex), "{}", example.display());
        assert!(!tex.contains("\\VAR{") && !tex.contains("\\BLOCK{"), "{}", example.display());
        assert_eq!(tex.matches("\\begin{document}").count(), 1);
    }
}

#[test]
fn missing_required_fields_are_reported() {
    let _home = isolate();
    let dir = tempfile::tempdir().unwrap();
    let doc = write(dir.path(), "r.md", "---\ntemplate: invoice\nnumber: 1\n---\nText\n");
    let err = render_to_string(&doc, None, None).unwrap_err().to_string();
    assert!(err.contains("recipient") && err.contains("items"), "{err}");
}

#[test]
fn project_defaults_fill_the_sender() {
    let _home = isolate();
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "texify.toml",
        "[defaults.sender]\nname = \"Jordan\"\nstreet = \"Way 1\"\nzip = \"01234\"\ncity = \"Town\"\n",
    );
    let doc = write(dir.path(), "l.md", "---\ntemplate: letter\nrecipient: |\n  A\n  B\n---\nHello\n");
    let tex = render_to_string(&doc, None, None).unwrap();
    assert!(tex.contains("\\setkomavar{fromname}{Jordan}"));
    assert!(tex.contains("01234 Town"));
}

#[test]
fn values_are_escaped_and_template_defaults_apply() {
    let _home = isolate();
    let dir = tempfile::tempdir().unwrap();
    let doc = write(dir.path(), "a.md", "---\ntitle: R&D 100%\n---\nText\n");
    assert!(render_to_string(&doc, None, None).unwrap().contains("R\\&D 100\\%"));
    let contract = write(
        dir.path(),
        "c.md",
        "---\ntemplate: contract\nparties: [{role: A, name: B, address: C}]\n---\n# Contract\n",
    );
    assert!(render_to_string(&contract, None, None).unwrap().contains("\\usepackage[en,serif]{texify}"));
}

#[test]
fn project_template_overrides_builtin() {
    let _home = isolate();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("templates/letter")).unwrap();
    write(
        &dir.path().join("templates/letter"),
        "template.tex",
        "\\documentclass{article}\\begin{document}X\\end{document}\n",
    );
    write(dir.path(), "texify.toml", "");
    let cfg = Config::load(dir.path()).unwrap();
    let all = texify::templates::discover(&cfg).unwrap();
    assert_eq!(all["letter"].origin.as_str(), "project");
}

fn blank_pdf(path: &Path, pages: usize) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let kids: Vec<Object> = (0..pages)
        .map(|_| {
            let content = doc.add_object(Stream::new(dictionary! {}, Vec::new()));
            doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            })
            .into()
        })
        .collect();
    doc.objects
        .insert(pages_id, Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => pages as i64}));
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages_id});
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

#[test]
fn merge_pages_and_info() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b, out) = (dir.path().join("a.pdf"), dir.path().join("b.pdf"), dir.path().join("out.pdf"));
    blank_pdf(&a, 2);
    blank_pdf(&b, 3);
    let report = merge(&[a.clone(), b.clone(), out.clone()], &out, true, None).unwrap();
    assert_eq!(report.iter().map(|(_, n)| *n).collect::<Vec<_>>(), [2, 3]); // target itself skipped
    let i = info(&out).unwrap();
    assert_eq!(i.pages, 5);
    assert_eq!(i.page_size_mm, Some((210, 297)));
    let merged = Document::load(&out).unwrap();
    assert!(merged.catalog().unwrap().has(b"Outlines"));

    assert_eq!(parse_ranges("1-2,4,6-", 7).unwrap(), [1, 2, 4, 6, 7]);
    assert!(parse_ranges("5-9", 6).is_err());
    assert_eq!(select_pages(&out, "5,1", &dir.path().join("sel.pdf")).unwrap(), 2);
    assert_eq!(info(&dir.path().join("sel.pdf")).unwrap().pages, 2);
}

#[cfg(unix)]
#[test]
fn ai_command_is_provider_agnostic() {
    let _home = isolate();
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "texify.toml", "");
    let cfg = Config::load(dir.path()).unwrap();
    // SAFETY: test-local override, read only by ai_command()
    unsafe { std::env::remove_var("TEXIFY_AI_COMMAND") };
    let err = texify::ai::ai_command(&cfg).unwrap_err().to_string();
    assert!(err.contains("MCP"), "{err}");
    // any stdin -> stdout program works as a "model": cat echoes the prompt
    unsafe { std::env::set_var("TEXIFY_AI_COMMAND", "cat") };
    let msg = texify::ai::Message { user: true, content: "<document>hello</document>".into() };
    let answer = texify::ai::complete("system", &[msg], &cfg).unwrap();
    assert_eq!(texify::ai::extract_blocks(&answer, &["document"]).unwrap(), ["hello\n"]);
    unsafe { std::env::remove_var("TEXIFY_AI_COMMAND") };
}

#[test]
fn compile_examples_when_engine_available() {
    let _home = isolate();
    let cfg = Config::load_cwd().unwrap();
    if texify::engine::resolve(&cfg, None).is_err() {
        eprintln!("no LaTeX engine – skipping compile test");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for name in ["letter", "invoice", "cv"] {
        let src = dir.path().join(format!("{name}.md"));
        let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/templates").join(name).join("example.md");
        std::fs::copy(example, &src).unwrap();
        let r = build(&src, BuildOptions::default()).unwrap_or_else(|e| match e {
            texify::Error::Compile(c) => panic!("{}", c.details()),
            other => panic!("{other}"),
        });
        assert!(info(&r.pdf).unwrap().pages >= 1);
    }
}

"""TeXify MCP server (stdio).

Lets an MCP host (Claude Desktop/Code, ...) write Markdown documents, build
them with the deterministic template pipeline, organise PDFs and – where no
template exists yet – design new templates. New templates are only stored
after a successful test build of their example.

All paths are relative to the workspace (default: current directory) and may
not escape it.
"""

from __future__ import annotations

import functools
import json
from pathlib import Path

from mcp.server.fastmcp import FastMCP

from . import TexifyError
from .ai import GUIDE, install_template, test_template
from .build import build, render_to_string
from .config import load_config
from .engine import CompileError
from .importers import to_markdown
from .pdftools import collect_inputs, info, merge
from .templates import META_FILE, discover, get_template

TEXT_SUFFIXES = {".md", ".markdown", ".txt", ".tex", ".yaml", ".yml", ".toml", ".log", ".sty"}

mcp = FastMCP(
    "texify",
    instructions=(
        "TeXify turns Markdown/Text with YAML frontmatter into professionally designed PDFs via "
        "LaTeX templates. Workflow: list_templates -> describe_template -> write_document -> "
        "build_document. Use an existing template whenever one fits. Only if none fits, read "
        "template_guide and call create_template (it test-builds before saving). "
        "Never invent personal data; ask the user for missing required fields."
    ),
)
_ROOT: Path = Path.cwd()


def _path(rel: str) -> Path:
    p = (_ROOT / rel).resolve()
    if p != _ROOT and _ROOT not in p.parents:
        raise TexifyError(f"Pfad liegt außerhalb des Arbeitsordners: {rel}")
    return p


def _rel(p: Path) -> str:
    try:
        return p.resolve().relative_to(_ROOT).as_posix()
    except ValueError:
        return str(p)


def _safe(fn):
    """Convert TeXify errors into tool results the model can act on."""
    @functools.wraps(fn)  # keeps the signature FastMCP derives the tool schema from
    def wrapper(*args, **kwargs):
        try:
            return fn(*args, **kwargs)
        except CompileError as exc:
            return {"ok": False, "error": str(exc), "details": exc.details()}
        except TexifyError as exc:
            return {"ok": False, "error": str(exc)}
    return wrapper


@mcp.tool()
@_safe
def list_templates() -> dict:
    """List all available document templates (built-in, user and project)."""
    cfg = load_config(_ROOT)
    return {"ok": True, "templates": [
        {"name": t.name, "description": t.description, "origin": t.origin}
        for t in sorted(discover(cfg).values(), key=lambda t: t.name)
    ]}


@mcp.tool()
@_safe
def describe_template(name: str, include_source: bool = False) -> dict:
    """Fields, heading conventions and a full example document for a template.

    Set include_source=true to also get template.tex / template.yaml (useful as
    a reference before creating a new template).
    """
    cfg = load_config(_ROOT)
    t = get_template(name, cfg)
    result = {
        "ok": True, "name": t.name, "description": t.description, "fields": t.fields,
        "headings": t.headings, "example_md": t.example(),
        "project_defaults": cfg.defaults,
    }
    if include_source:
        result["template_tex"] = t.source()
        result["template_yaml"] = (t.path / META_FILE).read_text(encoding="utf-8")
    return result


@mcp.tool()
def template_guide() -> str:
    """Authoring guide for TeXify templates (syntax, filters, design rules). Read before create_template."""
    return GUIDE


@mcp.tool()
@_safe
def create_template(name: str, template_tex: str, template_yaml: str, example_md: str,
                    overwrite: bool = False) -> dict:
    """Create a new template in <workspace>/templates/<name>.

    The example is built first; the template is only saved if that succeeds.
    On failure the LaTeX log is returned – fix the problem and call again.
    """
    cfg = load_config(_ROOT)
    files = {"template_tex": template_tex, "template_yaml": template_yaml, "example_md": example_md}
    preview = _ROOT / f"{name}-beispiel.pdf"
    error = test_template(name, files, cfg, pdf_out=preview)
    if error:
        return {"ok": False, "error": "Test-Build fehlgeschlagen", "details": error}
    path = install_template(name, files, cfg.root / "templates", overwrite)
    return {"ok": True, "template": _rel(path), "preview_pdf": _rel(preview)}


@mcp.tool()
@_safe
def write_document(path: str, content: str, overwrite: bool = False) -> dict:
    """Write a Markdown/Text/YAML document (or texify.toml) into the workspace."""
    p = _path(path)
    if p.suffix.lower() not in TEXT_SUFFIXES:
        raise TexifyError(f"Nur Textdateien erlaubt ({', '.join(sorted(TEXT_SUFFIXES))})")
    if p.exists() and not overwrite:
        raise TexifyError(f"{path} existiert bereits (overwrite=true)")
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(content, encoding="utf-8")
    return {"ok": True, "written": _rel(p)}


@mcp.tool()
@_safe
def read_file(path: str) -> dict:
    """Read a text file from the workspace (.md, .tex, .yaml, .toml, .log, ...)."""
    p = _path(path)
    if not p.is_file():
        raise TexifyError(f"Nicht gefunden: {path}")
    if p.suffix.lower() not in TEXT_SUFFIXES:
        raise TexifyError("Binärdatei – für PDFs pdf_info oder import_document verwenden")
    return {"ok": True, "path": _rel(p), "content": p.read_text(encoding="utf-8", errors="replace")}


@mcp.tool()
@_safe
def list_files(pattern: str = "**/*", limit: int = 200) -> dict:
    """List workspace files matching a glob pattern (e.g. '**/*.md')."""
    files = [p for p in _ROOT.glob(pattern) if p.is_file() and ".git" not in p.parts]
    return {"ok": True, "files": [_rel(p) for p in sorted(files)[:limit]], "total": len(files)}


@mcp.tool()
@_safe
def build_document(source: str, output: str | None = None, template: str | None = None) -> dict:
    """Build a document (.md/.txt/.yaml/.tex) into a PDF. Returns the PDF path or the LaTeX error."""
    r = build(_path(source), output=_path(output) if output else None, template=template)
    return {"ok": True, "pdf": _rel(r.pdf), "pages": info(r.pdf)["pages"],
            "template": r.template, "engine": r.engine}


@mcp.tool()
@_safe
def render_latex(source: str, template: str | None = None) -> dict:
    """Return the generated LaTeX source for a document without compiling it."""
    return {"ok": True, "latex": render_to_string(_path(source), template=template)}


@mcp.tool()
@_safe
def merge_pdfs(inputs: list[str], output: str, bookmarks: bool = False) -> dict:
    """Merge PDFs (files, folders or globs, in the given order) into one PDF."""
    files = collect_inputs([str(_path(i)) for i in inputs])
    report = merge(files, _path(output), bookmarks=bookmarks)
    return {"ok": True, "pdf": output, "parts": [{"file": _rel(f), "pages": n} for f, n in report]}


@mcp.tool()
@_safe
def import_document(source: str, output: str | None = None, template: str | None = None) -> dict:
    """Extract text from a PDF/DOCX/TXT into a Markdown document (no AI involved)."""
    src = _path(source)
    text = to_markdown(src, template)
    dest = _path(output) if output else src.with_suffix(".md")
    if dest.exists():
        dest = dest.with_name(dest.stem + "-import.md")
    dest.write_text(text, encoding="utf-8")
    return {"ok": True, "markdown_file": _rel(dest), "content": text[:20000]}


@mcp.tool()
@_safe
def pdf_info(path: str) -> dict:
    """Page count, page size and metadata of a PDF."""
    return {"ok": True, **json.loads(json.dumps(info(_path(path)), default=str))}


def run(workspace: Path | None = None) -> None:
    global _ROOT
    _ROOT = (workspace or Path.cwd()).resolve()
    mcp.run()

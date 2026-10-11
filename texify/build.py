"""Pipeline: document (Markdown/Text/YAML + frontmatter) -> LaTeX -> PDF."""

from __future__ import annotations

import re
import shutil
import tempfile
from dataclasses import dataclass, field
from pathlib import Path

import yaml

from . import TexifyError
from . import engine as engines
from .config import Config, deep_merge, load_config
from .latex import Tex, color_hex, load_yaml, normalize_lang
from .markdown import LatexRenderer
from .templates import SHARED_DIR, Template, get_template, make_environment, render_template

SOURCE_SUFFIXES = {".md", ".markdown", ".txt", ".yaml", ".yml"}
DEFAULT_TEMPLATE = "article"
FONTS = {"sans", "serif"}

_FRONTMATTER = re.compile(r"\A﻿?---[ \t]*\r?\n(.*?)\r?\n(?:---|\.\.\.)[ \t]*(?:\r?\n|\Z)", re.S)


@dataclass
class Document:
    meta: dict
    body: str
    path: Path | None = None

    @property
    def base_dir(self) -> Path:
        return self.path.parent if self.path else Path.cwd()

    @property
    def stem(self) -> str:
        return self.path.stem if self.path else "dokument"


def parse_document(text: str, path: Path | None = None) -> Document:
    m = _FRONTMATTER.match(text)
    if not m:
        return Document({}, text, path)
    try:
        meta = load_yaml(m.group(1)) or {}
    except yaml.YAMLError as exc:
        where = f"{path}: " if path else ""
        raise TexifyError(f"{where}Frontmatter ist kein gültiges YAML: {exc}") from None
    if not isinstance(meta, dict):
        raise TexifyError(f"{path or 'Dokument'}: Frontmatter muss ein YAML-Mapping sein")
    return Document(meta, text[m.end():], path)


def load_document(path: Path) -> Document:
    if not path.is_file():
        raise TexifyError(f"Datei nicht gefunden: {path}")
    text = path.read_text(encoding="utf-8-sig")
    if path.suffix.lower() in {".yaml", ".yml"}:
        try:
            meta = load_yaml(text) or {}
        except yaml.YAMLError as exc:
            raise TexifyError(f"{path}: ungültiges YAML: {exc}") from None
        body = meta.pop("body", "") if isinstance(meta, dict) else ""
        return Document(meta, body or "", path)
    return parse_document(text, path)


# --------------------------------------------------------------------------- #
# Rendering
# --------------------------------------------------------------------------- #


def _asset_resolver(search_dirs: list[Path], build_dir: Path):
    copied: dict[Path, str] = {}

    def asset(src: str) -> str:
        if re.match(r"^[a-z][a-z0-9+.-]*://", src, re.I):
            raise TexifyError(f"Nur lokale Dateien werden eingebunden, keine URLs: {src}")
        for base in search_dirs:
            p = (base / src).resolve()
            if p.is_file():
                break
        else:
            raise TexifyError(f"Datei nicht gefunden: {src} (gesucht in: {', '.join(map(str, search_dirs))})")
        if p not in copied:
            safe = re.sub(r"[^A-Za-z0-9-]+", "-", p.stem).strip("-").lower() or "datei"
            name = f"assets/a{len(copied) + 1}-{safe}{p.suffix.lower()}"
            (build_dir / "assets").mkdir(exist_ok=True)
            shutil.copy2(p, build_dir / name)
            copied[p] = name
        return copied[p]

    return asset


def build_context(doc: Document, template: Template, cfg: Config, renderer: LatexRenderer) -> dict:
    ctx = deep_merge(deep_merge(template.defaults, cfg.defaults), doc.meta)
    body = doc.body
    if template.title_from_heading and not ctx.get("title"):
        title, body = renderer.extract_title(body)
        if title:
            ctx["title"] = title
    ctx["body"] = renderer.render(body)
    ctx["sections"] = renderer.sections(body)
    ctx["markdown"] = body
    lang = ctx["lang"]
    strings = template.strings
    ctx["txt"] = deep_merge(strings.get("de", {}), strings.get(lang, {}))
    ctx["template"] = template.name
    return ctx


def render_tex(
    doc: Document,
    cfg: Config,
    build_dir: Path,
    template_name: str | None = None,
) -> tuple[Path, Template]:
    """Render the document into ``build_dir`` (tex + shared style + assets)."""
    name = template_name or doc.meta.get("template") or cfg.defaults.get("template") or DEFAULT_TEMPLATE
    template = get_template(str(name), cfg)

    merged = deep_merge(deep_merge(template.defaults, cfg.defaults), doc.meta)
    lang = normalize_lang(merged.get("lang"))
    font = str(merged.get("font", "sans")).lower()
    if font not in FONTS:
        raise TexifyError(f"font muss 'sans' oder 'serif' sein, nicht {font!r}")
    doc.meta = {**doc.meta, "lang": lang, "font": font,
                "accent": color_hex(merged.get("accent", "1F4E79"))}

    build_dir.mkdir(parents=True, exist_ok=True)
    asset = _asset_resolver([doc.base_dir, cfg.root], build_dir)
    renderer = LatexRenderer(lang=lang, headings=template.headings, asset=asset)
    ctx = build_context(doc, template, cfg, renderer)

    missing = template.missing_fields(ctx)
    if missing:
        fields = template.fields
        lines = [f"  • {k}: {fields[k].get('description', '')}" for k in missing]
        raise TexifyError(
            f"Template '{template.name}' braucht noch folgende Angaben (Frontmatter oder texify.toml [defaults]):\n"
            + "\n".join(lines)
        )

    for src in [*SHARED_DIR.iterdir(), *template.asset_files()]:
        if src.is_file():
            shutil.copy2(src, build_dir / src.name)
    env = make_environment(template, lang, str(ctx.get("currency", "EUR")), renderer, asset)
    tex_source = render_template(env, template, ctx)
    tex_path = build_dir / f"{_safe_stem(doc.stem)}.tex"
    tex_path.write_text(tex_source, encoding="utf-8")
    return tex_path, template


def _safe_stem(stem: str) -> str:
    return re.sub(r"[^A-Za-z0-9._-]+", "_", stem).strip("_") or "dokument"


# --------------------------------------------------------------------------- #
# Build
# --------------------------------------------------------------------------- #


@dataclass
class BuildResult:
    source: Path
    pdf: Path
    template: str | None = None
    engine: str | None = None
    tex_dir: Path | None = None
    notes: list[str] = field(default_factory=list)


def default_output(source: Path, cfg: Config) -> Path:
    out_dir = cfg.section("build").get("out_dir") or ""
    folder = (cfg.root / out_dir) if out_dir else source.parent
    return folder / (source.stem + ".pdf")


def build(
    source: Path,
    output: Path | None = None,
    cfg: Config | None = None,
    engine: str | None = None,
    template: str | None = None,
    tex_dir: Path | None = None,
) -> BuildResult:
    """Build one source file into a PDF.

    ``tex_dir`` additionally exports the complete, standalone-compilable LaTeX
    project (tex + texify.sty + assets) for manual design work.
    """
    source = source.resolve()
    cfg = cfg or load_config(source)
    suffix = source.suffix.lower()
    output = (output or default_output(source, cfg)).resolve()

    if suffix == ".pdf":
        return BuildResult(source, source)
    if suffix == ".tex":
        eng = engines.resolve(cfg, engine)
        pdf = engines.compile_tex(source, eng, cfg)
        engines.clean_aux(source)
        if pdf != output:
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(pdf, output)
        return BuildResult(source, output, engine=eng)
    if suffix not in SOURCE_SUFFIXES:
        raise TexifyError(f"Nicht unterstützter Dateityp: {source.name} (erlaubt: .md .txt .yaml .tex .pdf)")

    doc = load_document(source)
    eng = engines.resolve(cfg, engine)
    with tempfile.TemporaryDirectory(prefix="texify-") as tmp:
        tex_path, tpl = render_tex(doc, cfg, Path(tmp), template)
        if tex_dir:
            _export(Path(tmp), tex_dir)
        pdf = engines.compile_tex(tex_path, eng, cfg)
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(pdf, output)
    return BuildResult(source, output, template=tpl.name, engine=eng, tex_dir=tex_dir)


def export_tex(source: Path, target_dir: Path, cfg: Config | None = None, template: str | None = None) -> Path:
    """Render only (no compile): write a standalone LaTeX project to ``target_dir``."""
    source = source.resolve()
    cfg = cfg or load_config(source)
    doc = load_document(source)
    with tempfile.TemporaryDirectory(prefix="texify-") as tmp:
        tex_path, _ = render_tex(doc, cfg, Path(tmp), template)
        _export(Path(tmp), target_dir)
    return target_dir / tex_path.name


def render_to_string(source: Path, cfg: Config | None = None, template: str | None = None) -> str:
    source = source.resolve()
    cfg = cfg or load_config(source)
    doc = load_document(source)
    with tempfile.TemporaryDirectory(prefix="texify-") as tmp:
        tex_path, _ = render_tex(doc, cfg, Path(tmp), template)
        return tex_path.read_text(encoding="utf-8")


def _export(src_dir: Path, target: Path) -> None:
    target.mkdir(parents=True, exist_ok=True)
    shutil.copytree(src_dir, target, dirs_exist_ok=True)


__all__ = [
    "Document", "BuildResult", "Tex", "build", "export_tex", "load_document",
    "parse_document", "render_tex", "render_to_string", "default_output",
]

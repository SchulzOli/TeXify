"""texify command line interface."""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import textwrap
import time
from pathlib import Path

from . import TexifyError, __version__
from . import engine as engines
from .build import SOURCE_SUFFIXES, build, export_tex
from .config import CONFIG_NAME, INIT_TEMPLATE, load_config, texify_home
from .engine import CompileError
from .templates import BUILTIN_DIR, discover, get_template

BUILD_INPUTS = SOURCE_SUFFIXES | {".tex", ".pdf"}


def out(msg: str = "") -> None:
    print(msg, flush=True)


def _open_file(path: Path) -> None:
    if sys.platform == "win32":
        os.startfile(path)  # type: ignore[attr-defined]
    else:
        subprocess.Popen(["open" if sys.platform == "darwin" else "xdg-open", str(path)])


def _pages(pdf: Path) -> int:
    from pypdf import PdfReader
    return len(PdfReader(pdf).pages)


# --------------------------------------------------------------------------- #
# build / tex
# --------------------------------------------------------------------------- #


def cmd_build(a: argparse.Namespace) -> int:
    from .pdftools import collect_inputs, merge

    sources = collect_inputs(a.sources, BUILD_INPUTS - {".pdf"} if not a.output else BUILD_INPUTS)
    if not sources:
        raise TexifyError("Keine Eingabedateien gefunden")
    target = Path(a.output) if a.output else None
    bundle = target is not None and target.suffix.lower() == ".pdf" and len(sources) > 1

    def run_once() -> Path | None:
        if bundle:
            with tempfile.TemporaryDirectory(prefix="texify-bundle-") as tmp:
                parts = []
                for i, src in enumerate(sources):
                    if src.suffix.lower() == ".pdf":
                        parts.append(src)
                        out(f"  + {src.name}")
                        continue
                    r = build(src, output=Path(tmp) / f"{i:03d}-{src.stem}.pdf",
                              engine=a.engine, template=a.template)
                    parts.append(r.pdf)
                    out(f"  ✓ {src.name}  [{r.template or 'tex'}]")
                report = merge(parts, target, bookmarks=a.bookmarks, titles=[s.stem for s in sources])
            pages = sum(n for _, n in report)
            out(f"Fertig: {len(report)} Teile, {pages} Seiten -> {target.resolve()}")
            return target
        last = None
        for src in sources:
            if src.suffix.lower() == ".pdf":
                continue
            if target and len(sources) == 1:
                dest = target
            elif target:  # several sources + folder
                dest = target / (src.stem + ".pdf")
            else:
                dest = None
            tex_dir = Path(a.tex_dir) / src.stem if a.tex_dir and len(sources) > 1 else (
                Path(a.tex_dir) if a.tex_dir else None)
            r = build(src, output=dest, engine=a.engine, template=a.template, tex_dir=tex_dir)
            info = f"{_pages(r.pdf)} S." + (f", Template {r.template}" if r.template else "")
            out(f"✓ {src.name} -> {r.pdf}  ({info}, {r.engine})")
            if r.tex_dir:
                out(f"  LaTeX-Projekt: {r.tex_dir.resolve()}")
            last = r.pdf
        return last

    result = run_once()
    if a.open and result:
        _open_file(result)
    if a.watch:
        _watch(sources, run_once)
    return 0


def _watch(sources: list[Path], rebuild) -> None:
    cfg = load_config(sources[0])
    def snapshot() -> dict:
        files = [s for s in sources if s.exists()]
        if cfg.project_file:
            files.append(cfg.project_file)
        for d in cfg.template_dirs():
            if d.is_dir():
                files += [p for p in d.rglob("*") if p.is_file()]
        return {f: f.stat().st_mtime for f in files}
    out("Beobachte Änderungen … (Strg+C beendet)")
    state = snapshot()
    try:
        while True:
            time.sleep(1)
            now = snapshot()
            if now != state:
                state = now
                out(time.strftime("[%H:%M:%S] Änderung erkannt"))
                try:
                    rebuild()
                except CompileError as exc:
                    print(exc.details(), file=sys.stderr)
                except TexifyError as exc:
                    print(f"Fehler: {exc}", file=sys.stderr)
    except KeyboardInterrupt:
        out("")


def cmd_tex(a: argparse.Namespace) -> int:
    src = Path(a.source)
    target = Path(a.output) if a.output else src.with_name(src.stem + "-tex")
    tex = export_tex(src, target, template=a.template)
    out(f"✓ LaTeX-Projekt: {target.resolve()}")
    out(f"  Hauptdatei:    {tex.name}  (kompilieren: texify build {tex})")
    return 0


# --------------------------------------------------------------------------- #
# templates / new / init
# --------------------------------------------------------------------------- #


def cmd_templates(a: argparse.Namespace) -> int:
    cfg = load_config()
    if a.action == "guide":
        out((Path(__file__).parent / "guide.md").read_text(encoding="utf-8"))
        return 0
    if a.action == "list" or not a.action:
        templates = discover(cfg)
        width = max(len(n) for n in templates)
        for name, t in sorted(templates.items()):
            origin = "" if t.origin == "builtin" else f"  [{t.origin}]"
            desc = textwrap.shorten(t.description, 78, placeholder=" …")
            out(f"  {name:<{width}}  {desc}{origin}")
        out("\nDetails: texify templates show <name>   Neu per KI: texify ai template <name> \"…\"")
        return 0
    if not a.name:
        raise TexifyError(f"texify templates {a.action} <name>")
    t = get_template(a.name, cfg)
    if a.action == "show":
        out(f"{t.name}  ({t.origin}: {t.path})\n\n{t.description}\n\nFelder:")
        for key, spec in t.fields.items():
            req = " (Pflicht)" if spec.get("required") else ""
            out(f"  {key:<18} {spec.get('description', '')}{req}")
        if t.headings:
            out("\nÜberschriften:")
            for lvl, spec in t.headings.items():
                out(f"  {'#' * int(lvl):<4} -> {spec}")
        out(f"\nBeispiel anlegen: texify new {t.name}")
        return 0
    if a.action == "eject":
        dest_root = Path(a.to) if a.to else load_config().root / "templates"
        dest = dest_root / t.name
        if dest.exists() and not a.force:
            raise TexifyError(f"{dest} existiert bereits (--force zum Überschreiben)")
        shutil.copytree(t.path, dest, dirs_exist_ok=True)
        out(f"✓ Template '{t.name}' kopiert nach {dest.resolve()}")
        out("  Diese Kopie hat Vorrang vor der eingebauten Version – frei anpassen.")
        out(f"  Gemeinsames Designpaket zum Nachlesen: {BUILTIN_DIR / '_shared' / 'texify.sty'}")
        return 0
    raise TexifyError(f"Unbekannte Aktion: {a.action}")


def cmd_new(a: argparse.Namespace) -> int:
    t = get_template(a.template, load_config())
    dest = Path(a.file or f"{t.name}.md")
    if dest.exists() and not a.force:
        raise TexifyError(f"{dest} existiert bereits (--force zum Überschreiben)")
    dest.write_text(t.example(), encoding="utf-8")
    out(f"✓ {dest} angelegt (Vorlage '{t.name}'). Bearbeiten, dann: texify build {dest}")
    return 0


def cmd_init(a: argparse.Namespace) -> int:
    dest = Path(CONFIG_NAME)
    if dest.exists() and not a.force:
        raise TexifyError(f"{CONFIG_NAME} existiert bereits (--force zum Überschreiben)")
    dest.write_text(INIT_TEMPLATE, encoding="utf-8")
    out(f"✓ {dest.resolve()} angelegt – Absenderdaten eintragen, sie gelten für alle Dokumente.")
    return 0


# --------------------------------------------------------------------------- #
# pdf organisation
# --------------------------------------------------------------------------- #


def cmd_merge(a: argparse.Namespace) -> int:
    from .pdftools import collect_inputs, merge, sort_paths

    target = Path(a.output)
    if target.exists() and not a.force:
        raise TexifyError(f"{target} existiert bereits (-f zum Überschreiben)")
    files = sort_paths(collect_inputs(a.inputs), a.sort)
    report = merge(files, target, bookmarks=a.bookmarks)
    for f, n in report:
        out(f"  + {f.name} ({n} S.)")
    out(f"Fertig: {len(report)} Dateien, {sum(n for _, n in report)} Seiten -> {target.resolve()}")
    return 0


def cmd_pages(a: argparse.Namespace) -> int:
    from .pdftools import select_pages

    n = select_pages(Path(a.source), a.ranges, Path(a.output))
    out(f"✓ {n} Seiten -> {Path(a.output).resolve()}")
    return 0


def cmd_info(a: argparse.Namespace) -> int:
    from .pdftools import info

    for f in a.files:
        i = info(Path(f))
        size = f"{i['page_size_mm'][0]}×{i['page_size_mm'][1]} mm" if i["page_size_mm"] else "-"
        out(f"{Path(f).name}: {i['pages']} Seiten, {size}, {i['bytes'] / 1024:.0f} KB"
            + (f", Titel: {i['title']}" if i["title"] else ""))
    return 0


def cmd_import(a: argparse.Namespace) -> int:
    from .importers import to_markdown

    src = Path(a.source)
    dest = Path(a.output) if a.output else src.with_suffix(".md")
    if dest.resolve() == src.resolve():
        dest = src.with_name(src.stem + "-import.md")
    if dest.exists() and not a.force:
        raise TexifyError(f"{dest} existiert bereits (--force zum Überschreiben)")
    dest.write_text(to_markdown(src, a.template), encoding="utf-8")
    out(f"✓ {src.name} -> {dest}")
    out("  Struktur prüfen; für ein bestimmtes Template per KI: texify ai fill <template> " + str(dest))
    return 0


# --------------------------------------------------------------------------- #
# doctor / ai / mcp
# --------------------------------------------------------------------------- #


def cmd_doctor(a: argparse.Namespace) -> int:
    cfg = load_config()
    out(f"TeXify {__version__}  (Python {sys.version.split()[0]})")
    out(f"Projekt-Konfiguration: {cfg.project_file or '– (texify init)'}")
    out(f"Benutzer-Verzeichnis:  {texify_home()}")
    out("\nLaTeX-Engines:")
    found = engines.available(cfg)
    for name in engines.ENGINES:
        out(f"  {'✓' if found[name] else '·'} {name}")
    try:
        out(f"  -> verwendet: {engines.resolve(cfg)}")
    except TexifyError:
        out("  -> keine Engine verfügbar (siehe README: Installation)")
    out("\nOptionale Module:")
    for mod, extra in (("mcp", "mcp"), ("anthropic", "ai"), ("docx", "docx")):
        try:
            __import__(mod)
            out(f"  ✓ {mod}")
        except ImportError:
            out(f"  · {mod}  (pip install \"texify[{extra}]\")")
    from .ai import backend_name
    try:
        out(f"\nKI-Backend: {backend_name(cfg)}")
    except TexifyError:
        out("\nKI-Backend: – (nur über MCP oder nach Einrichtung, siehe README)")
    out("\nTemplate-Pfade:")
    for d in [*cfg.template_dirs(), BUILTIN_DIR]:
        out(f"  {'✓' if d.is_dir() else '·'} {d}")
    return 0


def cmd_ai(a: argparse.Namespace) -> int:
    from . import ai
    from .importers import to_markdown

    cfg = load_config()
    if a.ai_cmd == "template":
        root = texify_home() / "templates" if a.user else cfg.root / "templates"
        preview = Path(f"{a.name}-beispiel.pdf")
        path = ai.generate_template(a.name, a.description, cfg, root, base=a.base,
                                    engine=a.engine, overwrite=a.force, preview=preview, progress=out)
        out(f"✓ Template '{a.name}' erstellt: {path}")
        out(f"  Vorschau: {preview.resolve()}")
        out(f"  Dokument anlegen: texify new {a.name}")
        return 0
    if a.ai_cmd == "fill":
        src = Path(a.input)
        content = to_markdown(src) if src.suffix.lower() in {".pdf", ".docx"} else src.read_text(encoding="utf-8-sig")
        text = ai.fill(a.template, content, cfg, progress=out)
        dest = Path(a.output) if a.output else src.with_name(f"{src.stem}-{a.template}.md")
        if dest.exists() and not a.force:
            raise TexifyError(f"{dest} existiert bereits (--force zum Überschreiben)")
        dest.write_text(text, encoding="utf-8")
        todos = text.count("TODO")
        out(f"✓ {dest}" + (f"  ({todos}× TODO – bitte ergänzen)" if todos else ""))
        out(f"  Bauen: texify build {dest}")
        return 0
    raise TexifyError("texify ai template|fill …")


def cmd_mcp(a: argparse.Namespace) -> int:
    from .mcp_server import run
    run(Path(a.workspace) if a.workspace else None)
    return 0


# --------------------------------------------------------------------------- #
# parser
# --------------------------------------------------------------------------- #


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="texify",
        description="Markdown/Text -> LaTeX -> PDF. Professionelle Dokumente aus einfachen Textdateien.",
        epilog="Schnellstart:  texify new letter brief.md  &&  texify build brief.md",
    )
    p.add_argument("--version", action="version", version=f"texify {__version__}")
    sub = p.add_subparsers(dest="cmd", metavar="<befehl>")

    b = sub.add_parser("build", help="Dokument(e) zu PDF bauen; mehrere + -o x.pdf = Mappe")
    b.add_argument("sources", nargs="+", help=".md/.txt/.yaml/.tex, Ordner, Glob – mit -o x.pdf auch .pdf")
    b.add_argument("-o", "--output", help="Ziel-PDF (bei mehreren Quellen: zusammengeführt) oder Ordner")
    b.add_argument("-t", "--template", help="Template erzwingen (statt Frontmatter)")
    b.add_argument("-e", "--engine", choices=engines.ENGINES, help="LaTeX-Engine erzwingen")
    b.add_argument("--tex-dir", help="vollständiges LaTeX-Projekt zusätzlich hierhin exportieren")
    b.add_argument("--bookmarks", action="store_true", help="Lesezeichen je Teil (bei Mappen)")
    b.add_argument("--open", action="store_true", help="PDF danach öffnen")
    b.add_argument("-w", "--watch", action="store_true", help="bei Änderungen neu bauen")
    b.set_defaults(func=cmd_build)

    t = sub.add_parser("tex", help="nur LaTeX erzeugen (Projektordner zum Weiterbearbeiten)")
    t.add_argument("source")
    t.add_argument("-o", "--output", help="Zielordner (Standard: <name>-tex/)")
    t.add_argument("-t", "--template")
    t.set_defaults(func=cmd_tex)

    n = sub.add_parser("new", help="neues Dokument aus dem Beispiel eines Templates")
    n.add_argument("template")
    n.add_argument("file", nargs="?", help="Dateiname (Standard: <template>.md)")
    n.add_argument("--force", action="store_true")
    n.set_defaults(func=cmd_new)

    tp = sub.add_parser("templates", help="Templates: list | show <name> | eject <name> | guide")
    tp.add_argument("action", nargs="?", choices=["list", "show", "eject", "guide"], default="list")
    tp.add_argument("name", nargs="?")
    tp.add_argument("--to", help="eject: Zielordner (Standard: ./templates)")
    tp.add_argument("--force", action="store_true")
    tp.set_defaults(func=cmd_templates)

    i = sub.add_parser("init", help=f"{CONFIG_NAME} mit Absender-/Design-Vorgaben anlegen")
    i.add_argument("--force", action="store_true")
    i.set_defaults(func=cmd_init)

    m = sub.add_parser("merge", help="PDFs zu einem zusammenführen")
    m.add_argument("inputs", nargs="+", help="PDF-Dateien, Ordner oder Glob-Muster")
    m.add_argument("-o", "--output", default="merged.pdf")
    m.add_argument("--sort", choices=["none", "name", "date"], default="none")
    m.add_argument("--bookmarks", action="store_true", help="Lesezeichen je Datei")
    m.add_argument("-f", "--force", action="store_true", help="Ziel überschreiben")
    m.set_defaults(func=cmd_merge)

    pg = sub.add_parser("pages", help="Seiten auswählen/umsortieren, z. B. 1-3,5,8-")
    pg.add_argument("source")
    pg.add_argument("ranges")
    pg.add_argument("-o", "--output", required=True)
    pg.set_defaults(func=cmd_pages)

    inf = sub.add_parser("info", help="Seitenzahl/Format/Metadaten von PDFs")
    inf.add_argument("files", nargs="+")
    inf.set_defaults(func=cmd_info)

    im = sub.add_parser("import", help="PDF/DOCX/TXT als Markdown übernehmen")
    im.add_argument("source")
    im.add_argument("-o", "--output")
    im.add_argument("-t", "--template", help="Template im Frontmatter vorbelegen")
    im.add_argument("--force", action="store_true")
    im.set_defaults(func=cmd_import)

    d = sub.add_parser("doctor", help="Umgebung prüfen (Engines, Module, KI)")
    d.set_defaults(func=cmd_doctor)

    ai = sub.add_parser("ai", help="KI-Unterstützung: neue Templates, Text strukturieren")
    ai_sub = ai.add_subparsers(dest="ai_cmd", metavar="<aktion>")
    at = ai_sub.add_parser("template", help="neues Template entwerfen + per Test-Build prüfen")
    at.add_argument("name", help="Name, z. B. 'kuendigung' oder 'spendenquittung'")
    at.add_argument("description", help="Was soll das Dokument leisten, wie soll es aussehen?")
    at.add_argument("--base", help="Referenz-Template (Standard: automatisch)")
    at.add_argument("--user", action="store_true", help="in ~/.texify/templates statt ./templates")
    at.add_argument("-e", "--engine", choices=engines.ENGINES)
    at.add_argument("--force", action="store_true")
    af = ai_sub.add_parser("fill", help="unstrukturierten Text in ein Template-Dokument überführen")
    af.add_argument("template")
    af.add_argument("input", help="Text-, Markdown-, PDF- oder DOCX-Datei")
    af.add_argument("-o", "--output")
    af.add_argument("--force", action="store_true")
    ai.set_defaults(func=cmd_ai)

    mc = sub.add_parser("mcp", help="MCP-Server (stdio) für Claude Desktop/Code & Co.")
    mc.add_argument("--workspace", help="Arbeitsordner (Standard: aktuelles Verzeichnis)")
    mc.set_defaults(func=cmd_mcp)
    return p


def main(argv: list[str] | None = None) -> int:
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")  # type: ignore[attr-defined]
        except (AttributeError, ValueError):
            pass
    p = parser()
    a = p.parse_args(argv)
    if not getattr(a, "func", None):
        p.print_help()
        return 0
    try:
        return a.func(a)
    except CompileError as exc:
        print(exc.details(), file=sys.stderr)
        return 2
    except TexifyError as exc:
        print(f"Fehler: {exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())

"""texify command line interface."""

from __future__ import annotations

import argparse
import json
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
        raise TexifyError("No input files found")
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
            out(f"Done: {len(report)} parts, {pages} pages -> {target.resolve()}")
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
            details = f"{_pages(r.pdf)} p." + (f", template {r.template}" if r.template else "")
            out(f"✓ {src.name} -> {r.pdf}  ({details}, {r.engine})")
            if r.tex_dir:
                out(f"  LaTeX project: {r.tex_dir.resolve()}")
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

    out("Watching for changes … (Ctrl+C to stop)")
    state = snapshot()
    try:
        while True:
            time.sleep(1)
            now = snapshot()
            if now != state:
                state = now
                out(time.strftime("[%H:%M:%S] change detected"))
                try:
                    rebuild()
                except CompileError as exc:
                    print(exc.details(), file=sys.stderr)
                except TexifyError as exc:
                    print(f"Error: {exc}", file=sys.stderr)
    except KeyboardInterrupt:
        out("")


def cmd_tex(a: argparse.Namespace) -> int:
    src = Path(a.source)
    target = Path(a.output) if a.output else src.with_name(src.stem + "-tex")
    tex = export_tex(src, target, template=a.template)
    out(f"✓ LaTeX project: {target.resolve()}")
    out(f"  main file:     {tex.name}  (compile: texify build {tex})")
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
        out("\nDetails: texify templates show <name>   New one with AI: texify ai template <name> \"…\"")
        return 0
    if not a.name:
        raise TexifyError(f"texify templates {a.action} <name>")
    t = get_template(a.name, cfg)
    if a.action == "show":
        out(f"{t.name}  ({t.origin}: {t.path})\n\n{t.description}\n\nFields:")
        for key, spec in t.fields.items():
            req = " (required)" if spec.get("required") else ""
            out(f"  {key:<18} {spec.get('description', '')}{req}")
        if t.headings:
            out("\nHeadings:")
            for lvl, spec in t.headings.items():
                out(f"  {'#' * int(lvl):<4} -> {spec}")
        out(f"\nStart a document: texify new {t.name}")
        return 0
    if a.action == "eject":
        dest_root = Path(a.to) if a.to else load_config().root / "templates"
        dest = dest_root / t.name
        if dest.exists() and not a.force:
            raise TexifyError(f"{dest} already exists (--force to overwrite)")
        shutil.copytree(t.path, dest, dirs_exist_ok=True)
        out(f"✓ Template '{t.name}' copied to {dest.resolve()}")
        out("  This copy takes precedence over the built-in one – adapt it freely.")
        out(f"  Shared design package for reference: {BUILTIN_DIR / '_shared' / 'texify.sty'}")
        return 0
    raise TexifyError(f"Unknown action: {a.action}")


def cmd_new(a: argparse.Namespace) -> int:
    t = get_template(a.template, load_config())
    dest = Path(a.file or f"{t.name}.md")
    if dest.exists() and not a.force:
        raise TexifyError(f"{dest} already exists (--force to overwrite)")
    dest.write_text(t.example(), encoding="utf-8")
    out(f"✓ created {dest} (template '{t.name}'). Edit it, then: texify build {dest}")
    return 0


def cmd_init(a: argparse.Namespace) -> int:
    dest = Path(CONFIG_NAME)
    if dest.exists() and not a.force:
        raise TexifyError(f"{CONFIG_NAME} already exists (--force to overwrite)")
    dest.write_text(INIT_TEMPLATE, encoding="utf-8")
    out(f"✓ created {dest.resolve()} – enter your sender details once, they apply to every document.")
    return 0


# --------------------------------------------------------------------------- #
# pdf organisation
# --------------------------------------------------------------------------- #


def cmd_merge(a: argparse.Namespace) -> int:
    from .pdftools import collect_inputs, merge, sort_paths

    target = Path(a.output)
    if target.exists() and not a.force:
        raise TexifyError(f"{target} already exists (-f to overwrite)")
    files = sort_paths(collect_inputs(a.inputs), a.sort)
    report = merge(files, target, bookmarks=a.bookmarks)
    for f, n in report:
        out(f"  + {f.name} ({n} p.)")
    out(f"Done: {len(report)} files, {sum(n for _, n in report)} pages -> {target.resolve()}")
    return 0


def cmd_pages(a: argparse.Namespace) -> int:
    from .pdftools import select_pages

    n = select_pages(Path(a.source), a.ranges, Path(a.output))
    out(f"✓ {n} pages -> {Path(a.output).resolve()}")
    return 0


def cmd_info(a: argparse.Namespace) -> int:
    from .pdftools import info

    for f in a.files:
        i = info(Path(f))
        size = f"{i['page_size_mm'][0]}×{i['page_size_mm'][1]} mm" if i["page_size_mm"] else "-"
        out(f"{Path(f).name}: {i['pages']} pages, {size}, {i['bytes'] / 1024:.0f} KB"
            + (f", title: {i['title']}" if i["title"] else ""))
    return 0


def cmd_import(a: argparse.Namespace) -> int:
    from .importers import to_markdown

    src = Path(a.source)
    dest = Path(a.output) if a.output else src.with_suffix(".md")
    if dest.resolve() == src.resolve():
        dest = src.with_name(src.stem + "-import.md")
    if dest.exists() and not a.force:
        raise TexifyError(f"{dest} already exists (--force to overwrite)")
    dest.write_text(to_markdown(src, a.template), encoding="utf-8")
    out(f"✓ {src.name} -> {dest}")
    out(f"  Check the structure; to fit it to a template with AI: texify ai fill <template> {dest}")
    return 0


# --------------------------------------------------------------------------- #
# doctor / ai / mcp
# --------------------------------------------------------------------------- #


def cmd_doctor(a: argparse.Namespace) -> int:
    cfg = load_config()
    out(f"TeXify {__version__}  (Python {sys.version.split()[0]})")
    out(f"Project config:  {cfg.project_file or '– (texify init)'}")
    out(f"User folder:     {texify_home()}")
    out("\nLaTeX engines:")
    found = engines.available(cfg)
    for name in engines.ENGINES:
        out(f"  {'✓' if found[name] else '·'} {name}")
    try:
        out(f"  -> using: {engines.resolve(cfg)}")
    except TexifyError:
        out("  -> no engine available (see README: Installation)")
    out("\nOptional modules:")
    for mod, extra in (("mcp", "mcp"), ("docx", "docx")):
        try:
            __import__(mod)
            out(f"  ✓ {mod}")
        except ImportError:
            out(f"  · {mod}  (pip install \"texify[{extra}]\")")
    from .ai import ai_command
    try:
        out(f"\nAI command (CLI): {' '.join(ai_command(cfg))}")
    except TexifyError:
        out("\nAI command (CLI): – not configured; AI is available through the MCP server (texify mcp)")
    out("\nTemplate folders:")
    for d in [*cfg.template_dirs(), BUILTIN_DIR]:
        out(f"  {'✓' if d.is_dir() else '·'} {d}")
    return 0


def cmd_ai(a: argparse.Namespace) -> int:
    from . import ai
    from .importers import to_markdown

    cfg = load_config()
    if a.ai_cmd == "template":
        root = texify_home() / "templates" if a.user else cfg.root / "templates"
        preview = Path(f"{a.name}-example.pdf")
        path = ai.generate_template(a.name, a.description, cfg, root, base=a.base,
                                    engine=a.engine, overwrite=a.force, preview=preview, progress=out)
        out(f"✓ template '{a.name}' created: {path}")
        out(f"  preview: {preview.resolve()}")
        out(f"  start a document: texify new {a.name}")
        return 0
    if a.ai_cmd == "fill":
        src = Path(a.input)
        content = to_markdown(src) if src.suffix.lower() in {".pdf", ".docx"} else src.read_text(encoding="utf-8-sig")
        text = ai.fill(a.template, content, cfg, progress=out)
        dest = Path(a.output) if a.output else src.with_name(f"{src.stem}-{a.template}.md")
        if dest.exists() and not a.force:
            raise TexifyError(f"{dest} already exists (--force to overwrite)")
        dest.write_text(text, encoding="utf-8")
        todos = text.count("TODO")
        out(f"✓ {dest}" + (f"  ({todos}× TODO – please complete)" if todos else ""))
        out(f"  build: texify build {dest}")
        return 0
    raise TexifyError("texify ai template|fill …")


def mcp_client_config(mode: str, fmt: str, workspace: Path, url: str, image: str) -> str:
    """Configuration snippet for MCP clients (stdio, HTTP or Docker)."""
    if mode == "http":
        entry = {"type": "http", "url": url}
    else:
        if mode == "docker":
            command, args = "docker", ["run", "-i", "--rm", "-v", f"{workspace}:/work", image, "mcp"]
        else:
            command, args = sys.executable, ["-m", "texify", "mcp", "--workspace", str(workspace)]
        entry = {"type": "stdio", "command": command, "args": args}
    if fmt == "vscode":
        return json.dumps({"servers": {"texify": entry}}, indent=2)
    plain = {k: v for k, v in entry.items() if k != "type" or mode == "http"}
    if fmt == "toml":
        lines = ["[mcp_servers.texify]"]
        lines += [f"{k} = {json.dumps(v)}" for k, v in plain.items()]
        return "\n".join(lines)
    if fmt == "yaml":
        lines = ["mcp_servers:", "  texify:"]
        lines += [f"    {k}: {json.dumps(v)}" for k, v in plain.items()]
        return "\n".join(lines)
    return json.dumps({"mcpServers": {"texify": plain}}, indent=2)


def cmd_mcp(a: argparse.Namespace) -> int:
    workspace = Path(a.workspace).resolve() if a.workspace else Path.cwd().resolve()
    if a.action == "config":
        mode = "http" if a.http else "docker" if a.docker else "stdio"
        url = a.url or f"http://localhost:{a.port}/mcp"
        image = load_config(workspace).section("build").get("docker_image", "texify:latest")
        out(mcp_client_config(mode, a.format, workspace, url, image))
        return 0
    try:
        from .mcp_server import run
    except ImportError:
        raise TexifyError("The MCP server needs the mcp package:  pip install \"texify[mcp]\"") from None
    run(workspace, http=a.http, host=a.host, port=a.port, allowed_hosts=a.allow_host)
    return 0


# --------------------------------------------------------------------------- #
# parser
# --------------------------------------------------------------------------- #


def parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(
        prog="texify",
        description="Markdown/Text -> LaTeX -> PDF. Professional documents from plain text files.",
        epilog="Quick start:  texify new letter letter.md  &&  texify build letter.md",
    )
    p.add_argument("--version", action="version", version=f"texify {__version__}")
    sub = p.add_subparsers(dest="cmd", metavar="<command>")

    b = sub.add_parser("build", help="build document(s) into PDF; several + -o x.pdf = one bundle")
    b.add_argument("sources", nargs="+", help=".md/.txt/.yaml/.tex, folders, globs – with -o x.pdf also .pdf")
    b.add_argument("-o", "--output", help="target PDF (several sources: merged) or folder")
    b.add_argument("-t", "--template", help="force a template (instead of the frontmatter)")
    b.add_argument("-e", "--engine", choices=engines.ENGINES, help="force a LaTeX engine")
    b.add_argument("--tex-dir", help="also export the complete LaTeX project to this folder")
    b.add_argument("--bookmarks", action="store_true", help="one bookmark per part (bundles)")
    b.add_argument("--open", action="store_true", help="open the PDF afterwards")
    b.add_argument("-w", "--watch", action="store_true", help="rebuild on changes")
    b.set_defaults(func=cmd_build)

    t = sub.add_parser("tex", help="generate LaTeX only (project folder for further design work)")
    t.add_argument("source")
    t.add_argument("-o", "--output", help="target folder (default: <name>-tex/)")
    t.add_argument("-t", "--template")
    t.set_defaults(func=cmd_tex)

    n = sub.add_parser("new", help="start a document from a template's example")
    n.add_argument("template")
    n.add_argument("file", nargs="?", help="file name (default: <template>.md)")
    n.add_argument("--force", action="store_true")
    n.set_defaults(func=cmd_new)

    tp = sub.add_parser("templates", help="templates: list | show <name> | eject <name> | guide")
    tp.add_argument("action", nargs="?", choices=["list", "show", "eject", "guide"], default="list")
    tp.add_argument("name", nargs="?")
    tp.add_argument("--to", help="eject: target folder (default: ./templates)")
    tp.add_argument("--force", action="store_true")
    tp.set_defaults(func=cmd_templates)

    i = sub.add_parser("init", help=f"create {CONFIG_NAME} with sender/design defaults")
    i.add_argument("--force", action="store_true")
    i.set_defaults(func=cmd_init)

    m = sub.add_parser("merge", help="merge PDFs into one")
    m.add_argument("inputs", nargs="+", help="PDF files, folders or glob patterns")
    m.add_argument("-o", "--output", default="merged.pdf")
    m.add_argument("--sort", choices=["none", "name", "date"], default="none")
    m.add_argument("--bookmarks", action="store_true", help="one bookmark per file")
    m.add_argument("-f", "--force", action="store_true", help="overwrite the target")
    m.set_defaults(func=cmd_merge)

    pg = sub.add_parser("pages", help="select/reorder pages, e.g. 1-3,5,8-")
    pg.add_argument("source")
    pg.add_argument("ranges")
    pg.add_argument("-o", "--output", required=True)
    pg.set_defaults(func=cmd_pages)

    inf = sub.add_parser("info", help="page count/format/metadata of PDFs")
    inf.add_argument("files", nargs="+")
    inf.set_defaults(func=cmd_info)

    im = sub.add_parser("import", help="convert PDF/DOCX/TXT to Markdown")
    im.add_argument("source")
    im.add_argument("-o", "--output")
    im.add_argument("-t", "--template", help="pre-set the template in the frontmatter")
    im.add_argument("--force", action="store_true")
    im.set_defaults(func=cmd_import)

    d = sub.add_parser("doctor", help="check the environment (engines, modules, AI)")
    d.set_defaults(func=cmd_doctor)

    ai = sub.add_parser("ai", help="AI assistance via any LLM command: new templates, structure text")
    ai_sub = ai.add_subparsers(dest="ai_cmd", metavar="<action>")
    at = ai_sub.add_parser("template", help="design a new template and verify it with a test build")
    at.add_argument("name", help="name, e.g. 'donation-receipt'")
    at.add_argument("description", help="what the document is for and how it should look")
    at.add_argument("--base", help="reference template (default: chosen automatically)")
    at.add_argument("--user", action="store_true", help="store in ~/.texify/templates instead of ./templates")
    at.add_argument("-e", "--engine", choices=engines.ENGINES)
    at.add_argument("--force", action="store_true")
    af = ai_sub.add_parser("fill", help="turn unstructured text into a document for a template")
    af.add_argument("template")
    af.add_argument("input", help="text, Markdown, PDF or DOCX file")
    af.add_argument("-o", "--output")
    af.add_argument("--force", action="store_true")
    ai.set_defaults(func=cmd_ai)

    mc = sub.add_parser("mcp", help="MCP server for any AI client (stdio or HTTP) | config snippets")
    mc.add_argument("action", nargs="?", choices=["serve", "config"], default="serve",
                    help="serve (default) or config: print the client configuration")
    mc.add_argument("--workspace", help="working folder (default: current directory)")
    mc.add_argument("--http", action="store_true", help="Streamable HTTP instead of stdio (one server, many clients)")
    mc.add_argument("--host", default="127.0.0.1", help="HTTP bind address (Docker: 0.0.0.0)")
    mc.add_argument("--port", type=int, default=8747)
    mc.add_argument("--allow-host", action="append", default=[], metavar="HOST:PORT",
                    help="additional Host header accepted over HTTP (e.g. a LAN name)")
    mc.add_argument("--docker", action="store_true", help="config: run the server in the Docker image")
    mc.add_argument("--url", help="config: URL of a running HTTP server")
    mc.add_argument("--format", choices=["json", "vscode", "toml", "yaml"], default="json",
                    help="config: json = generic mcpServers, vscode = .vscode/mcp.json, toml/yaml for other clients")
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
        print(f"Error: {exc}", file=sys.stderr)
        return 1
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())

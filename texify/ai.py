"""AI assistance — only where deterministic code cannot help.

1. ``generate_template``: design a new template for a document type that has
   no template yet. The result is validated by a real test build; render or
   LaTeX errors are fed back to the model until it compiles.
2. ``fill``: turn unstructured text (notes, an imported old PDF, an e-mail)
   into a document for a given template (frontmatter + Markdown).

TeXify is provider-agnostic. There are two ways to bring a model:

* MCP (recommended): any MCP client (Claude, ChatGPT, VS Code/Copilot, Cursor,
  Hermes, ...) talks to ``texify mcp``. The client's own model does the work
  through the tools ``template_guide`` / ``create_template`` / ``write_document``
  and the prompts ``design_template`` / ``fill_template``.
* CLI: ``[ai] command`` in texify.toml (or ``TEXIFY_AI_COMMAND``) names any
  command that reads a prompt on stdin and prints the answer on stdout, e.g.
  ``llm -m gpt-4o``, ``ollama run llama3.1`` or ``claude -p``.
"""

from __future__ import annotations

import copy
import os
import re
import shlex
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Callable

import yaml

from . import TexifyError
from .build import build, parse_document, render_tex
from .config import Config
from .engine import CompileError
from .templates import EXAMPLE_FILE, META_FILE, TEMPLATE_FILE, get_template, load_template_meta

GUIDE = (Path(__file__).resolve().parent / "guide.md").read_text(encoding="utf-8")
BLOCKS = ("template_tex", "template_yaml", "example_md")

Progress = Callable[[str], None]


# --------------------------------------------------------------------------- #
# Model access: one generic stdin -> stdout command
# --------------------------------------------------------------------------- #


def ai_command(cfg: Config) -> list[str]:
    raw = os.environ.get("TEXIFY_AI_COMMAND") or cfg.section("ai").get("command") or ""
    if not raw.strip():
        raise TexifyError(
            "No AI command configured. Either\n"
            "  • use TeXify through its MCP server from any AI client (texify mcp), or\n"
            "  • set [ai] command in texify.toml (or TEXIFY_AI_COMMAND) to any tool that reads\n"
            "    a prompt on stdin, e.g. \"llm -m gpt-4o\", \"ollama run llama3.1\", \"claude -p\"."
        )
    parts = shlex.split(raw, posix=os.name != "nt")
    # non-POSIX mode (Windows) keeps the quotes around "C:\Program Files\..."
    parts = [p[1:-1] if len(p) > 1 and p[0] == p[-1] and p[0] in "\"'" else p for p in parts]
    exe = shutil.which(parts[0])
    if not exe:
        raise TexifyError(f"AI command not found: {parts[0]}")
    return [exe, *parts[1:]]


def complete(system: str, messages: list[dict], cfg: Config) -> str:
    """Send a conversation to the configured AI command and return its answer."""
    cmd = ai_command(cfg)
    parts = [system, ""]
    for m in messages:
        who = "TASK" if m["role"] == "user" else "YOUR PREVIOUS ANSWER"
        parts += [f"=== {who} ===", m["content"], ""]
    parts.append("Answer now, strictly in the required output format.")
    timeout = int(cfg.section("ai").get("timeout", 900))
    try:
        r = subprocess.run(cmd, input="\n".join(parts), capture_output=True, text=True,
                           encoding="utf-8", errors="replace", timeout=timeout)
    except subprocess.TimeoutExpired:
        raise TexifyError(f"AI command timed out after {timeout}s") from None
    if r.returncode != 0:
        raise TexifyError(f"AI command failed: {(r.stderr or r.stdout).strip()[-800:]}")
    if not r.stdout.strip():
        raise TexifyError("AI command returned an empty answer")
    return r.stdout


# --------------------------------------------------------------------------- #
# Template generation
# --------------------------------------------------------------------------- #

_REFERENCE_HINTS = [
    ("invoice", ("invoice", "rechnung", "quote", "angebot", "receipt", "quittung", "credit note")),
    ("contract", ("contract", "vertrag", "agreement", "lease", "terms")),
    ("minutes", ("minutes", "protokoll", "agenda", "tagesordnung", "meeting")),
    ("invitation", ("invitation", "einladung", "event", "party", "feier")),
    ("certificate", ("certificate", "urkunde", "award", "diploma", "zertifikat")),
    ("letter", ("letter", "brief", "anschreiben", "cancellation", "kündigung", "complaint", "notice")),
    ("cv", ("cv", "resume", "résumé", "lebenslauf", "vita")),
    ("confirmation", ("confirmation", "bestätigung", "attestation", "reference", "power of attorney")),
    ("proposal", ("proposal", "antrag", "grant", "concept", "pitch")),
]


def pick_reference(description: str) -> str:
    d = description.lower()
    for name, words in _REFERENCE_HINTS:
        if any(w in d for w in words):
            return name
    return "article"


def template_system_prompt(reference) -> str:
    return f"""You are an expert LaTeX typographer creating templates for TeXify.
TeXify turns readable Markdown + YAML frontmatter into professional PDFs; the
design lives entirely in the template.

{GUIDE}

# Reference template '{reference.name}' (follow its style and conventions)

<template_tex>
{reference.source()}
</template_tex>
<template_yaml>
{(reference.path / META_FILE).read_text(encoding="utf-8")}
</template_yaml>
<example_md>
{reference.example()}
</example_md>

# Output format
Output EXACTLY three blocks, no other text and no Markdown code fences:
<template_tex>…complete template.tex…</template_tex>
<template_yaml>…complete template.yaml…</template_yaml>
<example_md>…a realistic, complete example document with fictional data…</example_md>
The example must contain `template: <name>` in its frontmatter and fill all required fields."""


def extract_blocks(text: str, names=BLOCKS) -> dict[str, str]:
    out = {}
    for name in names:
        m = re.search(rf"<{name}>\s*(.*?)\s*</{name}>", text, re.S)
        if not m:
            raise TexifyError(f"Answer contains no <{name}> block")
        content = re.sub(r"^```[a-z]*\n|\n```$", "", m.group(1).strip())
        out[name] = content.strip() + "\n"
    return out


def _with_template_dir(cfg: Config, folder: Path) -> Config:
    new = copy.deepcopy(cfg)
    paths = list(new.data.get("templates", {}).get("paths", []))
    new.data.setdefault("templates", {})["paths"] = paths + [str(folder.resolve())]
    return new


def test_template(name: str, files: dict[str, str], cfg: Config, engine: str | None = None,
                  pdf_out: Path | None = None) -> str | None:
    """Write the template to a temp folder and build its example.

    Returns ``None`` on success, otherwise an error report for the model.
    """
    with tempfile.TemporaryDirectory(prefix="texify-tpl-") as tmp:
        root = Path(tmp) / "templates"
        tdir = root / name
        tdir.mkdir(parents=True)
        (tdir / TEMPLATE_FILE).write_text(files["template_tex"], encoding="utf-8")
        (tdir / META_FILE).write_text(files["template_yaml"], encoding="utf-8")
        (tdir / EXAMPLE_FILE).write_text(files["example_md"], encoding="utf-8")
        try:
            load_template_meta(tdir)
            doc = parse_document(files["example_md"])
            if doc.meta.get("template") != name:
                return f"example_md must contain `template: {name}` in its frontmatter."
            example = Path(tmp) / "example.md"
            example.write_text(files["example_md"], encoding="utf-8")
            result = build(example, output=Path(tmp) / "example.pdf",
                           cfg=_with_template_dir(cfg, root), engine=engine)
            if pdf_out:
                pdf_out.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(result.pdf, pdf_out)
        except CompileError as exc:
            return "LaTeX compilation failed:\n" + exc.details()
        except TexifyError as exc:
            return f"Error: {exc}"
        except yaml.YAMLError as exc:
            return f"YAML error: {exc}"
    return None


def install_template(name: str, files: dict[str, str], target_root: Path, overwrite: bool = False) -> Path:
    if not re.fullmatch(r"[a-z0-9][a-z0-9_-]*", name):
        raise TexifyError("Template name: lower-case letters, digits, '-' and '_' only")
    target = target_root / name
    if target.exists():
        if not overwrite:
            raise TexifyError(f"Template already exists: {target} (allow overwriting)")
        shutil.rmtree(target)
    target.mkdir(parents=True)
    (target / TEMPLATE_FILE).write_text(files["template_tex"], encoding="utf-8")
    (target / META_FILE).write_text(files["template_yaml"], encoding="utf-8")
    (target / EXAMPLE_FILE).write_text(files["example_md"], encoding="utf-8")
    return target


def generate_template(
    name: str,
    description: str,
    cfg: Config,
    target_root: Path,
    base: str | None = None,
    engine: str | None = None,
    overwrite: bool = False,
    preview: Path | None = None,
    progress: Progress = print,
) -> Path:
    reference = get_template(base or pick_reference(description), cfg)
    system = template_system_prompt(reference)
    messages = [{"role": "user", "content": f"Create the template `{name}`.\n\nRequirements:\n{description}"}]
    attempts = int(cfg.section("ai").get("attempts", 3))
    last_error = ""
    for attempt in range(1, attempts + 1):
        progress(f"AI is designing template '{name}' (attempt {attempt}/{attempts}, reference: {reference.name}) …")
        reply = complete(system, messages, cfg)
        try:
            files = extract_blocks(reply)
        except TexifyError as exc:
            last_error = str(exc)
        else:
            progress("Test build of the example …")
            last_error = test_template(name, files, cfg, engine, pdf_out=preview) or ""
            if not last_error:
                return install_template(name, files, target_root, overwrite)
        progress(f"  ✗ {last_error.splitlines()[0]}")
        messages += [
            {"role": "assistant", "content": reply},
            {"role": "user", "content": f"The test build failed:\n\n{last_error}\n\n"
                                        "Fix the problem and output all three blocks again, complete."},
        ]
    raise TexifyError(f"Could not create the template in {attempts} attempts.\nLast error:\n{last_error}")


# --------------------------------------------------------------------------- #
# Fill: unstructured text -> document for a template
# --------------------------------------------------------------------------- #


def fill_system_prompt(template) -> str:
    return f"""You turn unstructured content into a TeXify document for the template '{template.name}'.

Template description: {template.description}

Fields (template.yaml):
{yaml.safe_dump(template.fields, allow_unicode=True, sort_keys=False)}
Heading convention (headings): {template.headings or 'default (# section, ## subsection)'}

Example document for this template:
<example>
{template.example()}
</example>

Rules:
- Use ONLY information from the user's content. Do not invent anything.
- If required information is missing, use the value "TODO: <what is missing>".
- Running text and sections belong in the Markdown body, not in YAML; YAML is for structured data only.
- Document language = language of the content (field lang: en or de).
- Output: exactly one block <document>…</document> with the complete document
  (YAML frontmatter with `template: {template.name}` + Markdown), no code fences."""


def fill(template_name: str, content: str, cfg: Config, progress: Progress = print) -> str:
    template = get_template(template_name, cfg)
    system = fill_system_prompt(template)
    messages = [{"role": "user", "content": f"Content:\n\n{content}"}]
    error = ""
    for attempt in (1, 2):
        progress(f"AI is structuring the content for template '{template.name}' (attempt {attempt}/2) …")
        reply = complete(system, messages, cfg)
        try:
            doc_text = extract_blocks(reply, ("document",))["document"]
            doc = parse_document(doc_text)
            doc.meta["template"] = template.name
            with tempfile.TemporaryDirectory(prefix="texify-fill-") as tmp:
                render_tex(doc, cfg, Path(tmp))
            return doc_text
        except TexifyError as exc:
            error = str(exc)
            messages += [
                {"role": "assistant", "content": reply},
                {"role": "user", "content": f"The document is invalid:\n{error}\nPlease fix it."},
            ]
    raise TexifyError(f"AI result is invalid: {error}")

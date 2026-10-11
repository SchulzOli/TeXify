"""AI assistance — only where deterministic code cannot help.

1. ``generate_template``: design a new template for a document type that has
   no template yet. The result is validated by a real test build; render or
   LaTeX errors are fed back to the model until it compiles.
2. ``fill``: turn unstructured text (notes, an imported old PDF, an e-mail)
   into a document for a given template (frontmatter + Markdown).

Backends: Anthropic API (``anthropic`` SDK) or the Claude Code CLI
(``claude -p``). Inside an MCP host (Claude Desktop/Code) no backend is needed:
the host model uses the MCP tools ``template_guide`` / ``create_template``.
"""

from __future__ import annotations

import copy
import os
import re
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
FALLBACK_MODELS = {"claude-opus-5-5", "claude-opus-5", "claude-fable-5-1", "claude-sonnet-5-5"}

Progress = Callable[[str], None]


# --------------------------------------------------------------------------- #
# Backends
# --------------------------------------------------------------------------- #


def backend_name(cfg: Config) -> str:
    choice = cfg.section("ai").get("backend", "auto")
    if choice != "auto":
        return choice
    try:
        import anthropic  # noqa: F401
        has_sdk = True
    except ImportError:
        has_sdk = False
    if has_sdk and (os.environ.get("ANTHROPIC_API_KEY") or os.environ.get("ANTHROPIC_AUTH_TOKEN")):
        return "anthropic"
    if shutil.which("claude"):
        return "claude-cli"
    if has_sdk:
        return "anthropic"  # may still work via an `ant auth login` profile
    raise TexifyError(
        "Kein KI-Backend verfügbar. Optionen:\n"
        "  • pip install \"texify[ai]\" und ANTHROPIC_API_KEY setzen (oder `ant auth login`)\n"
        "  • Claude Code CLI installieren (`claude` im PATH)\n"
        "  • TeXify als MCP-Server in Claude Desktop/Code nutzen (texify mcp)"
    )


def complete(system: str, messages: list[dict], cfg: Config) -> str:
    backend = backend_name(cfg)
    if backend == "anthropic":
        return _complete_anthropic(system, messages, cfg)
    if backend == "claude-cli":
        return _complete_cli(system, messages)
    raise TexifyError(f"Unbekanntes KI-Backend '{backend}' (auto | anthropic | claude-cli)")


def _complete_anthropic(system: str, messages: list[dict], cfg: Config) -> str:
    try:
        import anthropic
    except ImportError:
        raise TexifyError("Anthropic SDK fehlt:  pip install \"texify[ai]\"") from None
    ai = cfg.section("ai")
    model = ai.get("model", "claude-opus-5-5")
    extra: dict = {}
    if model in FALLBACK_MODELS:
        # server-side fallback: a policy decline is retried on a fallback model
        extra = {"betas": ["server-side-fallback-2026-07-01"], "extra_body": {"fallbacks": "default"}}
    client = anthropic.Anthropic()
    try:
        with client.beta.messages.stream(
            model=model,
            max_tokens=64000,
            system=system,
            messages=messages,
            output_config={"effort": ai.get("effort", "high")},
            **extra,
        ) as stream:
            msg = stream.get_final_message()
    except anthropic.AuthenticationError:
        raise TexifyError("Anthropic: keine gültigen Zugangsdaten (ANTHROPIC_API_KEY oder `ant auth login`)") from None
    except anthropic.RateLimitError:
        raise TexifyError("Anthropic: Rate-Limit erreicht, bitte später erneut versuchen") from None
    except anthropic.APIStatusError as exc:
        raise TexifyError(f"Anthropic API-Fehler {exc.status_code}: {exc.message}") from None
    except anthropic.APIConnectionError:
        raise TexifyError("Anthropic: keine Verbindung zur API") from None
    if msg.stop_reason == "refusal":
        raise TexifyError("Die KI hat die Anfrage abgelehnt.")
    if msg.stop_reason == "max_tokens":
        raise TexifyError("Antwort der KI wurde abgeschnitten (max_tokens).")
    return "".join(block.text for block in msg.content if block.type == "text")


def _complete_cli(system: str, messages: list[dict]) -> str:
    exe = shutil.which("claude")
    if not exe:
        raise TexifyError("Claude Code CLI (`claude`) nicht gefunden")
    parts = [system, ""]
    for m in messages:
        who = "AUFGABE" if m["role"] == "user" else "DEINE VORHERIGE ANTWORT"
        parts += [f"=== {who} ===", m["content"], ""]
    parts.append("Antworte jetzt ausschließlich im geforderten Ausgabeformat.")
    try:
        r = subprocess.run(
            [exe, "-p", "--output-format", "text"], input="\n".join(parts),
            capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=1800,
        )
    except subprocess.TimeoutExpired:
        raise TexifyError("Claude CLI: Zeitüberschreitung") from None
    if r.returncode != 0:
        raise TexifyError(f"Claude CLI fehlgeschlagen: {(r.stderr or r.stdout).strip()[-800:]}")
    return r.stdout


# --------------------------------------------------------------------------- #
# Template generation
# --------------------------------------------------------------------------- #

_REFERENCE_HINTS = [
    ("invoice", ("rechnung", "invoice", "angebot", "quote", "gutschrift", "quittung", "beleg")),
    ("letter", ("brief", "anschreiben", "letter", "kündigung", "widerspruch", "einladung", "antrag an")),
    ("cv", ("lebenslauf", "cv", "resume", "vita")),
    ("confirmation", ("bestätigung", "bescheinigung", "zertifikat", "urkunde", "nachweis", "certificate")),
    ("proposal", ("antrag", "proposal", "konzept", "angebotskonzept", "exposé")),
]


def pick_reference(description: str) -> str:
    d = description.lower()
    for name, words in _REFERENCE_HINTS:
        if any(w in d for w in words):
            return name
    return "article"


def _template_system(reference) -> str:
    return f"""Du bist ein erfahrener LaTeX-Typograf und erstellst Templates für TeXify.
TeXify macht aus lesbarem Markdown + YAML-Frontmatter professionelle PDF-Dokumente;
das Design liegt vollständig im Template.

{GUIDE}

# Referenz-Template '{reference.name}' (Stil und Konventionen übernehmen)

<template_tex>
{reference.source()}
</template_tex>
<template_yaml>
{(reference.path / META_FILE).read_text(encoding="utf-8")}
</template_yaml>
<example_md>
{reference.example()}
</example_md>

# Ausgabeformat
Gib GENAU drei Blöcke aus, ohne weiteren Text und ohne Markdown-Codezäune:
<template_tex>…vollständiges template.tex…</template_tex>
<template_yaml>…vollständige template.yaml…</template_yaml>
<example_md>…realistisches, vollständiges Beispieldokument mit fiktiven Daten…</example_md>
Das Beispiel muss `template: <name>` im Frontmatter haben und alle Pflichtfelder füllen."""


def extract_blocks(text: str, names=BLOCKS) -> dict[str, str]:
    out = {}
    for name in names:
        m = re.search(rf"<{name}>\s*(.*?)\s*</{name}>", text, re.S)
        if not m:
            raise TexifyError(f"Antwort enthält keinen <{name}>-Block")
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
                return f"example_md muss im Frontmatter `template: {name}` enthalten."
            example = Path(tmp) / "example.md"
            example.write_text(files["example_md"], encoding="utf-8")
            result = build(example, output=Path(tmp) / "example.pdf",
                           cfg=_with_template_dir(cfg, root), engine=engine)
            if pdf_out:
                pdf_out.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(result.pdf, pdf_out)
        except CompileError as exc:
            return "LaTeX-Kompilierung fehlgeschlagen:\n" + exc.details()
        except TexifyError as exc:
            return f"Fehler: {exc}"
        except yaml.YAMLError as exc:
            return f"YAML-Fehler: {exc}"
    return None


def install_template(name: str, files: dict[str, str], target_root: Path, overwrite: bool = False) -> Path:
    if not re.fullmatch(r"[a-z0-9][a-z0-9_-]*", name):
        raise TexifyError("Template-Name: nur Kleinbuchstaben, Ziffern, '-' und '_'")
    target = target_root / name
    if target.exists():
        if not overwrite:
            raise TexifyError(f"Template existiert bereits: {target} (überschreiben erlauben)")
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
    system = _template_system(reference)
    messages = [{"role": "user", "content": f"Erstelle das Template `{name}`.\n\nAnforderungen:\n{description}"}]
    attempts = int(cfg.section("ai").get("attempts", 3))
    last_error = ""
    for attempt in range(1, attempts + 1):
        progress(f"KI entwirft Template '{name}' (Versuch {attempt}/{attempts}, Referenz: {reference.name}) …")
        reply = complete(system, messages, cfg)
        try:
            files = extract_blocks(reply)
        except TexifyError as exc:
            last_error = str(exc)
        else:
            progress("Test-Build des Beispiels …")
            last_error = test_template(name, files, cfg, engine, pdf_out=preview) or ""
            if not last_error:
                return install_template(name, files, target_root, overwrite)
        progress(f"  ✗ {last_error.splitlines()[0]}")
        messages += [
            {"role": "assistant", "content": reply},
            {"role": "user", "content": f"Der Test-Build ist fehlgeschlagen:\n\n{last_error}\n\n"
                                        "Korrigiere den Fehler und gib alle drei Blöcke vollständig erneut aus."},
        ]
    raise TexifyError(f"Template konnte nach {attempts} Versuchen nicht erzeugt werden.\nLetzter Fehler:\n{last_error}")


# --------------------------------------------------------------------------- #
# Fill: unstructured text -> document for a template
# --------------------------------------------------------------------------- #


def _fill_system(template) -> str:
    return f"""Du überführst unstrukturierten Inhalt in ein TeXify-Dokument für das Template '{template.name}'.

Template-Beschreibung: {template.description}

Felder (template.yaml):
{yaml.safe_dump(template.fields, allow_unicode=True, sort_keys=False)}
Überschriften-Konvention (headings): {template.headings or 'Standard (# Abschnitt, ## Unterabschnitt)'}

Beispieldokument für dieses Template:
<example>
{template.example()}
</example>

Regeln:
- Verwende NUR Informationen aus dem Inhalt des Nutzers. Erfinde nichts.
- Fehlen Pflichtangaben, setze den Wert "TODO: <was fehlt>".
- Fließtext und Abschnitte gehören ins Markdown, nicht ins YAML; YAML nur für strukturierte Daten.
- Sprache des Dokuments = Sprache des Inhalts (Feld lang: de oder en).
- Ausgabe: genau ein Block <document>…</document> mit dem vollständigen Dokument
  (YAML-Frontmatter mit `template: {template.name}` + Markdown), ohne Codezäune."""


def fill(template_name: str, content: str, cfg: Config, progress: Progress = print) -> str:
    template = get_template(template_name, cfg)
    system = _fill_system(template)
    messages = [{"role": "user", "content": f"Inhalt:\n\n{content}"}]
    error = ""
    for attempt in (1, 2):
        progress(f"KI strukturiert Inhalt für Template '{template.name}' (Versuch {attempt}/2) …")
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
                {"role": "user", "content": f"Das Dokument ist ungültig:\n{error}\nBitte korrigieren."},
            ]
    raise TexifyError(f"KI-Ergebnis ungültig: {error}")

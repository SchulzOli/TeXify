"""Build every example, the application bundle, preview images and the gallery.

    python scripts/build_examples.py            (needs a LaTeX engine or Docker)

Outputs (all committed, so GitHub shows the results):
  examples/pdf/<name>.pdf        one PDF per example
  examples/pdf/bewerbungsmappe.pdf   cover page + letter + CV, with bookmarks
  examples/previews/<name>.png   first-page thumbnail (needs pypdfium2)
  examples/README.md             gallery, generated from the example files
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from texify.build import build  # noqa: E402
from texify.config import load_config  # noqa: E402
from texify.pdftools import merge  # noqa: E402
from texify.templates import get_template  # noqa: E402

EXAMPLES = ROOT / "examples"
PDF = EXAMPLES / "pdf"
PREVIEWS = EXAMPLES / "previews"
BUNDLE = ["13-deckblatt", "05-anschreiben", "11-lebenslauf"]
COLUMNS = 3


TYPES = {
    "01-bericht": "Bericht",
    "02-anleitung": "Anleitung",
    "03-lastenheft": "Lastenheft",
    "04-pressemitteilung": "Pressemitteilung",
    "05-anschreiben": "Bewerbungsanschreiben",
    "06-kuendigung": "Kündigung",
    "07-mahnung": "Zahlungserinnerung",
    "08-rechnung": "Rechnung",
    "09-rechnung-kleinunternehmer": "Rechnung (Kleinunternehmer)",
    "10-angebot": "Angebot",
    "11-lebenslauf": "Lebenslauf (tabellarisch)",
    "12-cv-english": "CV (modern, English)",
    "13-deckblatt": "Bewerbungs-Deckblatt",
    "14-projektantrag": "Projektantrag",
    "15-teilnahmebestaetigung": "Teilnahmebestätigung",
    "16-arbeitszeugnis": "Arbeitszeugnis",
    "17-vollmacht": "Vollmacht",
    "18-protokoll": "Protokoll",
    "19-einladung": "Einladung",
    "20-zertifikat": "Zertifikat",
    "21-vertrag": "Vertrag",
}


def title_of(doc_path: Path) -> str:
    return TYPES.get(doc_path.stem, doc_path.stem.split("-", 1)[-1].replace("-", " ").title())


def render_preview(pdf: Path, png: Path) -> None:
    import pypdfium2 as pdfium

    page = pdfium.PdfDocument(pdf)[0]
    scale = 420 / page.get_width()
    page.render(scale=scale).to_pil().save(png, optimize=True)


def main() -> int:
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    PDF.mkdir(exist_ok=True)
    PREVIEWS.mkdir(exist_ok=True)
    cfg = load_config(EXAMPLES)
    rows = []
    for src in sorted(EXAMPLES.glob("[0-9][0-9]-*.md")):
        result = build(src, output=PDF / f"{src.stem}.pdf", cfg=cfg)
        print(f"✓ {src.name:34} [{result.template}]")
        render_preview(result.pdf, PREVIEWS / f"{src.stem}.png")
        tpl = get_template(result.template, cfg)
        rows.append((src, result.template, tpl.description, title_of(src)))

    parts = [PDF / f"{name}.pdf" for name in BUNDLE]
    merge(parts, PDF / "bewerbungsmappe.pdf", bookmarks=True,
          titles=["Deckblatt", "Anschreiben", "Lebenslauf"])
    print("✓ bewerbungsmappe.pdf")

    write_gallery(rows)
    print("✓ examples/README.md")
    return 0


def write_gallery(rows) -> None:
    lines = [
        "# Beispiele",
        "",
        f"{len(rows)} Dokumenttypen, jeweils aus einer kurzen Markdown-Datei erzeugt.",
        "Absender und Bankdaten kommen aus [texify.toml](texify.toml) und müssen",
        "deshalb in keinem Dokument wiederholt werden.",
        "",
        "```bash",
        "cd examples",
        "texify build 08-rechnung.md          # -> pdf/08-rechnung.pdf",
        "python ../scripts/build_examples.py  # alle Beispiele + Galerie neu erzeugen",
        "```",
        "",
        "**Bewerbungsmappe** aus Deckblatt, Anschreiben und Lebenslauf, mit Lesezeichen:",
        "",
        "```bash",
        "texify build " + " ".join(f"{n}.md" for n in BUNDLE) + " -o pdf/bewerbungsmappe.pdf --bookmarks",
        "```",
        "",
        "Ergebnis: [pdf/bewerbungsmappe.pdf](pdf/bewerbungsmappe.pdf)",
        "",
        "<!-- Automatisch erzeugt von scripts/build_examples.py – nicht von Hand bearbeiten. -->",
        "",
    ]
    header = "| " + " | ".join([" "] * COLUMNS) + " |"
    lines += [header, "|" + "---|" * COLUMNS]
    for i in range(0, len(rows), COLUMNS):
        cells = []
        for src, template, _, title in rows[i:i + COLUMNS]:
            stem = src.stem
            cells.append(
                f"<a href=\"pdf/{stem}.pdf\"><img src=\"previews/{stem}.png\" width=\"240\"></a><br>"
                f"**{title}**<br>[`{src.name}`]({src.name}) · `{template}`"
            )
        cells += [" "] * (COLUMNS - len(cells))
        lines.append("| " + " | ".join(cells) + " |")
    lines += ["", "## Übersicht", "", "| Typ | Datei | Template |", "|---|---|---|"]
    for src, template, _, title in rows:
        lines.append(f"| {title} | [{src.name}]({src.name}) | `{template}` |")
    (EXAMPLES / "README.md").write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    sys.exit(main())

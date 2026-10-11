import re
from decimal import Decimal
from pathlib import Path

import pytest

from texify import TexifyError
from texify.build import parse_document, render_to_string
from texify.config import load_config
from texify.templates import BUILTIN_DIR, discover, invoice_totals

EXAMPLES = sorted(p for p in BUILTIN_DIR.iterdir() if p.is_dir() and not p.name.startswith("_"))


@pytest.fixture(autouse=True)
def isolated_home(tmp_path, monkeypatch):
    monkeypatch.setenv("TEXIFY_HOME", str(tmp_path / "home"))
    monkeypatch.chdir(tmp_path)


@pytest.mark.parametrize("folder", EXAMPLES, ids=lambda p: p.name)
def test_every_builtin_example_renders(folder: Path):
    tex = render_to_string(folder / "example.md")
    assert re.search(r"\\usepackage\[(de|en),(sans|serif)\]\{texify\}", tex)
    assert "\\VAR{" not in tex and "\\BLOCK{" not in tex
    assert tex.count("\\begin{document}") == 1


def test_builtin_templates_are_discovered():
    names = set(discover(load_config()))
    assert {"article", "letter", "invoice", "cv", "cv-cards", "proposal", "confirmation"} <= names


def test_project_template_overrides_builtin(tmp_path):
    t = tmp_path / "templates" / "letter"
    t.mkdir(parents=True)
    (t / "template.tex").write_text("\\documentclass{article}\\begin{document}X\\end{document}\n")
    assert discover(load_config(tmp_path))["letter"].origin == "project"


def test_missing_required_fields_are_reported(tmp_path):
    doc = tmp_path / "r.md"
    doc.write_text("---\ntemplate: invoice\nnumber: 1\n---\nText\n", encoding="utf-8")
    with pytest.raises(TexifyError, match="recipient"):
        render_to_string(doc)


def test_project_defaults_fill_sender(tmp_path):
    (tmp_path / "texify.toml").write_text(
        '[defaults.sender]\nname = "Erika"\nstreet = "Weg 1"\nzip = "01234"\ncity = "Ort"\n', encoding="utf-8")
    doc = tmp_path / "b.md"
    doc.write_text("---\ntemplate: letter\nrecipient: |\n  A\n  B\n---\nHallo\n", encoding="utf-8")
    tex = render_to_string(doc)
    assert "\\setkomavar{fromname}{Erika}" in tex
    assert "01234 Ort" in tex


def test_frontmatter_values_are_escaped(tmp_path):
    doc = tmp_path / "a.md"
    doc.write_text("---\ntitle: R&D 100%\n---\nText\n", encoding="utf-8")
    assert "R\\&D 100\\%" in render_to_string(doc)


def test_invoice_totals_are_exact():
    t = invoice_totals([{"desc": "a", "qty": 3, "price": "0,10"}, {"desc": "b", "price": 0.2}], vat=19)
    assert t["net"] == Decimal("0.50")
    assert t["vat"] == Decimal("0.10")
    assert t["gross"] == Decimal("0.60")
    assert invoice_totals([{"desc": "a", "price": 100}], small_business=True)["vat"] == 0


def test_command_before_value_is_separated(tmp_path):
    t = tmp_path / "templates" / "mini"
    t.mkdir(parents=True)
    (t / "template.tex").write_text("\\noindent\\VAR{title}\\par\\BLOCK{if title}\\relax\\BLOCK{endif}\n")
    doc = tmp_path / "d.md"
    doc.write_text("---\ntemplate: mini\ntitle: Erika\n---\n", encoding="utf-8")
    assert "\\noindent{}Erika" in render_to_string(doc)


def test_template_default_font_wins_over_builtin(tmp_path):
    doc = tmp_path / "v.md"
    doc.write_text("---\ntemplate: contract\nparties: [{role: A, name: B, address: C}]\n---\n# Vertrag\n",
                   encoding="utf-8")
    assert "\\usepackage[en,serif]{texify}" in render_to_string(doc)


def test_parse_document_without_frontmatter():
    doc = parse_document("# Nur Text\n")
    assert doc.meta == {} and doc.body.startswith("# Nur")

import shutil
from pathlib import Path

import pytest
from pypdf import PdfReader, PdfWriter

from texify import TexifyError
from texify.build import build
from texify.config import load_config
from texify.engine import available
from texify.pdftools import merge, parse_ranges, select_pages
from texify.templates import BUILTIN_DIR


def _pdf(path: Path, pages: int) -> Path:
    w = PdfWriter()
    for _ in range(pages):
        w.add_blank_page(595, 842)
    w.write(path)
    return path


def test_merge_with_bookmarks(tmp_path):
    a, b = _pdf(tmp_path / "a.pdf", 2), _pdf(tmp_path / "b.pdf", 3)
    out = tmp_path / "out.pdf"
    report = merge([a, b, out], out, bookmarks=True)  # target itself is skipped
    reader = PdfReader(out)
    assert [n for _, n in report] == [2, 3]
    assert len(reader.pages) == 5
    assert [o.title for o in reader.outline] == ["a", "b"]


def test_page_ranges(tmp_path):
    assert parse_ranges("1-2,4,6-", 7) == [0, 1, 3, 5, 6]
    with pytest.raises(TexifyError):
        parse_ranges("5-9", 6)
    src = _pdf(tmp_path / "s.pdf", 4)
    assert select_pages(src, "4,1", tmp_path / "o.pdf") == 2


@pytest.mark.skipif(not any(available(load_config()).values()), reason="keine LaTeX-Engine verfügbar")
@pytest.mark.parametrize("name", ["letter", "invoice", "cv"])
def test_compile_examples(tmp_path, monkeypatch, name):
    monkeypatch.setenv("TEXIFY_HOME", str(tmp_path / "home"))
    src = tmp_path / f"{name}.md"
    shutil.copy(BUILTIN_DIR / name / "example.md", src)
    result = build(src)
    assert result.pdf.is_file() and len(PdfReader(result.pdf).pages) >= 1

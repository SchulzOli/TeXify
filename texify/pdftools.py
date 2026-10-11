"""PDF organisation: merge (formerly OnePDF), page selection, info."""

from __future__ import annotations

import glob
from pathlib import Path

from pypdf import PdfReader, PdfWriter

from . import TexifyError

_GLOB_CHARS = set("*?[")


def collect_inputs(inputs: list[str], suffixes: set[str] | None = None) -> list[Path]:
    """Resolve files, folders and glob patterns into an ordered list of paths.

    Folders contribute their files (matching ``suffixes``) sorted by name.
    """
    suffixes = suffixes or {".pdf"}
    files: list[Path] = []
    for item in inputs:
        p = Path(item)
        if p.is_dir():
            files += sorted((f for f in p.iterdir() if f.suffix.lower() in suffixes),
                            key=lambda f: f.name.lower())
        elif _GLOB_CHARS & set(item):
            matches = sorted(glob.glob(item), key=str.lower)
            if not matches:
                raise TexifyError(f"No matches for '{item}'")
            files += [Path(m) for m in matches]
        elif p.is_file():
            files.append(p)
        else:
            raise TexifyError(f"Not found: {item}")
    return files


def sort_paths(files: list[Path], mode: str) -> list[Path]:
    if mode == "name":
        return sorted(files, key=lambda f: f.name.lower())
    if mode == "date":
        return sorted(files, key=lambda f: f.stat().st_mtime)
    return files


def _open(path: Path) -> PdfReader:
    try:
        reader = PdfReader(path)
    except Exception as exc:  # pypdf raises many different errors
        raise TexifyError(f"Cannot read PDF {path.name}: {exc}") from None
    if reader.is_encrypted and not reader.decrypt(""):
        raise TexifyError(f"PDF is password protected: {path.name}")
    return reader


def merge(files: list[Path], output: Path, bookmarks: bool = False,
          titles: list[str] | None = None) -> list[tuple[Path, int]]:
    """Merge PDFs in order. Returns ``[(file, pages)]``.

    ``titles`` overrides bookmark labels (defaults to file stems).
    """
    if not files:
        raise TexifyError("No PDF files to merge")
    output = output.resolve()
    files = [f for f in files if f.resolve() != output]  # never read the target itself
    writer = PdfWriter()
    report = []
    for i, f in enumerate(files):
        start = len(writer.pages)
        writer.append(_open(f))
        if bookmarks:
            label = titles[i] if titles and i < len(titles) else f.stem
            writer.add_outline_item(label, start)
        report.append((f, len(writer.pages) - start))
    output.parent.mkdir(parents=True, exist_ok=True)
    with open(output, "wb") as fh:
        writer.write(fh)
    return report


def parse_ranges(spec: str, total: int) -> list[int]:
    """'1-3,5,8-' -> zero-based page indices (1-based, inclusive input)."""
    pages: list[int] = []
    for part in spec.replace(" ", "").split(","):
        if not part:
            continue
        try:
            if "-" in part:
                a, b = part.split("-", 1)
                lo = int(a) if a else 1
                hi = int(b) if b else total
            else:
                lo = hi = int(part)
        except ValueError:
            raise TexifyError(f"Invalid page range: {part!r}") from None
        if not (1 <= lo <= hi <= total):
            raise TexifyError(f"Page range {part!r} outside 1–{total}")
        pages += range(lo - 1, hi)
    if not pages:
        raise TexifyError("No pages selected")
    return pages


def select_pages(source: Path, spec: str, output: Path) -> int:
    reader = _open(source)
    writer = PdfWriter()
    for idx in parse_ranges(spec, len(reader.pages)):
        writer.add_page(reader.pages[idx])
    output.parent.mkdir(parents=True, exist_ok=True)
    with open(output, "wb") as fh:
        writer.write(fh)
    return len(writer.pages)


def info(path: Path) -> dict:
    reader = _open(path)
    meta = reader.metadata or {}
    first = reader.pages[0].mediabox if reader.pages else None
    return {
        "file": str(path),
        "pages": len(reader.pages),
        "title": meta.get("/Title"),
        "author": meta.get("/Author"),
        "producer": meta.get("/Producer"),
        "page_size_mm": (round(float(first.width) * 25.4 / 72), round(float(first.height) * 25.4 / 72)) if first else None,
        "bytes": path.stat().st_size,
    }


def extract_text(path: Path) -> str:
    reader = _open(path)
    return "\n\n".join((page.extract_text() or "").strip() for page in reader.pages)

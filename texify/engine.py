"""LaTeX engine detection and compilation.

Supported: tectonic (single binary, fetches packages on demand), latexmk,
pdflatex/xelatex/lualatex (TeX Live / MiKTeX) and docker (texify image).
"""

from __future__ import annotations

import os
import re
import shutil
import subprocess
from pathlib import Path

from . import TexifyError
from .config import Config

ENGINES = ["tectonic", "latexmk", "pdflatex", "xelatex", "lualatex", "docker"]
AUX_SUFFIXES = {".aux", ".out", ".log", ".toc", ".fls", ".fdb_latexmk", ".synctex.gz", ".xdv", ".nav", ".snm"}
TIMEOUT = 600


class CompileError(TexifyError):
    def __init__(self, message: str, log: str = "", source: str = ""):
        super().__init__(message)
        self.log = log
        self.source = source

    def details(self) -> str:
        parts = [str(self)]
        if self.log:
            parts += ["", "--- LaTeX-Log ---", self.log]
        if self.source:
            parts += ["", "--- Quelltext an der Fehlerstelle ---", self.source]
        return "\n".join(parts)


def _docker_ready(image: str) -> bool:
    if not shutil.which("docker"):
        return False
    try:
        r = subprocess.run(["docker", "image", "inspect", image], capture_output=True, timeout=20)
    except (OSError, subprocess.TimeoutExpired):
        return False
    return r.returncode == 0


def available(cfg: Config) -> dict[str, bool]:
    image = cfg.section("build").get("docker_image", "texify:latest")
    found = {e: bool(shutil.which(e)) for e in ENGINES if e != "docker"}
    found["docker"] = _docker_ready(image)
    return found


def resolve(cfg: Config, preferred: str | None = None) -> str:
    choice = preferred or os.environ.get("TEXIFY_ENGINE") or cfg.section("build").get("engine", "auto")
    if choice != "auto":
        if choice not in ENGINES:
            raise TexifyError(f"Unbekannte Engine '{choice}'. Erlaubt: auto, {', '.join(ENGINES)}")
        return choice
    found = available(cfg)
    for engine in ENGINES:
        if found[engine]:
            return engine
    raise TexifyError(
        "Keine LaTeX-Engine gefunden. Optionen:\n"
        "  • Tectonic (empfohlen, ein Binary):  https://tectonic-typesetting.github.io\n"
        "  • TeX Live / MiKTeX (pdflatex, latexmk)\n"
        "  • Docker:  docker compose build   (Image 'texify:latest')\n"
        "Prüfen mit: texify doctor"
    )


def _commands(engine: str, tex: Path, cfg: Config) -> list[list[str]]:
    name = tex.name
    if engine == "tectonic":
        return [["tectonic", "--keep-logs", "--chatter", "minimal", name]]
    if engine == "latexmk":
        return [["latexmk", "-pdf", "-interaction=nonstopmode", "-halt-on-error", name]]
    if engine == "docker":
        image = cfg.section("build").get("docker_image", "texify:latest")
        return [[
            "docker", "run", "--rm",
            "--mount", f"type=bind,source={tex.parent},target=/work",
            "-w", "/work", "--entrypoint", "latexmk", image,
            "-pdf", "-interaction=nonstopmode", "-halt-on-error", name,
        ]]
    cmd = [engine, "-interaction=nonstopmode", "-halt-on-error", name]
    return [cmd, cmd]  # two passes for references / lastpage


def compile_tex(tex: Path, engine: str, cfg: Config) -> Path:
    """Compile ``tex`` in its own folder; returns the PDF path or raises CompileError."""
    tex = tex.resolve()
    pdf = tex.with_suffix(".pdf")
    pdf.unlink(missing_ok=True)
    for cmd in _commands(engine, tex, cfg):
        try:
            r = subprocess.run(
                cmd, cwd=tex.parent, capture_output=True, text=True,
                encoding="utf-8", errors="replace", timeout=TIMEOUT,
                stdin=subprocess.DEVNULL,
            )
        except FileNotFoundError:
            raise TexifyError(f"Engine '{engine}' ist nicht installiert (texify doctor)") from None
        except subprocess.TimeoutExpired:
            raise CompileError(f"{engine}: Zeitüberschreitung nach {TIMEOUT}s") from None
        if r.returncode != 0:
            raise _compile_error(engine, tex, r)
    if engine not in ("tectonic", "latexmk", "docker"):
        log = tex.with_suffix(".log")
        if log.is_file() and "Rerun to get" in log.read_text(encoding="utf-8", errors="replace"):
            subprocess.run(_commands(engine, tex, cfg)[0], cwd=tex.parent, capture_output=True,
                           timeout=TIMEOUT, stdin=subprocess.DEVNULL)
    if not pdf.is_file():
        raise CompileError(f"{engine} lief durch, aber es entstand kein PDF ({pdf.name})")
    return pdf


def _compile_error(engine: str, tex: Path, r: subprocess.CompletedProcess) -> CompileError:
    log_file = tex.with_suffix(".log")
    log_text = log_file.read_text(encoding="utf-8", errors="replace") if log_file.is_file() else ""
    lines = log_text.splitlines()
    first_err = next((i for i, ln in enumerate(lines) if ln.startswith("! ")), None)
    if first_err is not None:
        excerpt = "\n".join(lines[first_err:first_err + 14])
        message = lines[first_err][2:].strip()
    else:
        out = ((r.stdout or "") + "\n" + (r.stderr or "")).strip().splitlines()
        excerpt = "\n".join(out[-30:])
        message = next((ln for ln in reversed(out) if "error" in ln.lower()), "LaTeX-Fehler")
    source = ""
    # "l.<n> <text>": only show source if line n of the main file really holds
    # <text> (the error may come from a package such as texify.sty)
    m = re.search(r"^l\.(\d+) (.*)$", excerpt, re.M)
    if m and tex.is_file():
        n, snippet = int(m.group(1)), m.group(2).replace("^^M", "").strip().lstrip(".")[:20]
        src = tex.read_text(encoding="utf-8", errors="replace").splitlines()
        if 0 < n <= len(src) and snippet and snippet in src[n - 1]:
            lo, hi = max(0, n - 5), min(len(src), n + 3)
            source = "\n".join(f"{'>' if i + 1 == n else ' '}{i + 1:5d}| {src[i]}" for i in range(lo, hi))
    return CompileError(f"{engine}: {message}", log=excerpt, source=source)


def clean_aux(tex: Path, keep_log: bool = False) -> None:
    for f in tex.parent.glob(tex.stem + ".*"):
        if f.name.endswith(".synctex.gz") or f.suffix in AUX_SUFFIXES:
            if keep_log and f.suffix == ".log":
                continue
            f.unlink(missing_ok=True)

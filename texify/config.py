"""Configuration: texify.toml (project) + ~/.texify/config.toml (user).

Precedence (low -> high): built-in < user config < project config.
``[defaults]`` values are merged into every document's frontmatter, so sender
address, bank details, language or accent color are written down only once.
"""

from __future__ import annotations

import copy
import os
import tomllib
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

from . import TexifyError

CONFIG_NAME = "texify.toml"

BUILTIN: dict[str, Any] = {
    # lang/font/currency fall back to de/sans/EUR at render time, so that
    # template defaults (e.g. a serif contract) are not overridden here
    "defaults": {},
    "build": {"engine": "auto", "out_dir": "", "docker_image": "texify:latest"},
    "templates": {"paths": ["templates"]},
    "ai": {"backend": "auto", "model": "claude-opus-5-5", "effort": "high", "attempts": 3},
}


def texify_home() -> Path:
    return Path(os.environ.get("TEXIFY_HOME", Path.home() / ".texify"))


def deep_merge(base: dict, override: dict) -> dict:
    """Recursively merge ``override`` into a copy of ``base``."""
    out = copy.deepcopy(base)
    for key, value in (override or {}).items():
        if isinstance(value, dict) and isinstance(out.get(key), dict):
            out[key] = deep_merge(out[key], value)
        else:
            out[key] = copy.deepcopy(value)
    return out


def find_project_config(start: Path) -> Path | None:
    start = start.resolve()
    folder = start if start.is_dir() else start.parent
    for d in [folder, *folder.parents]:
        candidate = d / CONFIG_NAME
        if candidate.is_file():
            return candidate
    return None


def _load_toml(path: Path) -> dict:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except tomllib.TOMLDecodeError as exc:
        raise TexifyError(f"{path}: ungültiges TOML: {exc}") from None


@dataclass
class Config:
    data: dict = field(default_factory=lambda: copy.deepcopy(BUILTIN))
    project_file: Path | None = None
    user_file: Path | None = None

    @property
    def root(self) -> Path:
        """Project root: folder of texify.toml, else the current directory."""
        return self.project_file.parent if self.project_file else Path.cwd()

    @property
    def defaults(self) -> dict:
        return self.data["defaults"]

    def section(self, name: str) -> dict:
        return self.data.get(name, {})

    def template_dirs(self) -> list[Path]:
        """Search order for templates: project paths, user dir (built-ins are added later)."""
        dirs = [(self.root / p).resolve() for p in self.section("templates").get("paths", [])]
        dirs.append(texify_home() / "templates")
        return dirs


def load_config(start: Path | None = None) -> Config:
    """Load config for a document/folder (searches texify.toml upwards)."""
    data = copy.deepcopy(BUILTIN)
    cfg = Config(data=data)
    user = texify_home() / "config.toml"
    if user.is_file():
        cfg.user_file = user
        data = deep_merge(data, _load_toml(user))
    project = find_project_config(start or Path.cwd())
    if project:
        cfg.project_file = project
        data = deep_merge(data, _load_toml(project))
    cfg.data = data
    return cfg


INIT_TEMPLATE = """\
# TeXify Projekt-Konfiguration
# Alles unter [defaults] gilt als Vorbelegung für JEDES Dokument in diesem Ordner
# (und Unterordnern). Das Frontmatter eines Dokuments überschreibt diese Werte.

[defaults]
lang = "de"            # de | en
accent = "1F4E79"      # Akzentfarbe (Hex)
# font = "sans"        # sans | serif – sonst entscheidet das Template (z. B. Vertrag: serif)
# currency = "EUR"

[defaults.sender]      # Absender für Briefe, Rechnungen, Bestätigungen
name = "Max Mustermann"
street = "Musterstraße 1"
zip = "12345"
city = "Musterstadt"
phone = "+49 170 1234567"
email = "max@example.com"
# web = "example.com"
# tax_id = "123/456/78901"        # Steuernummer (Rechnung)
# vat_id = "DE123456789"          # USt-IdNr. (Rechnung)

# [defaults.sender.bank]
# name = "Musterbank"
# iban = "DE00 0000 0000 0000 0000 00"
# bic = "XXXXDEXXXXX"

[build]
engine = "auto"        # auto | tectonic | latexmk | pdflatex | xelatex | lualatex | docker
out_dir = ""           # leer = PDF neben der Quelldatei

[templates]
paths = ["templates"]  # eigene Templates (überschreiben eingebaute mit gleichem Namen)

[ai]
backend = "auto"       # auto | anthropic | claude-cli
model = "claude-opus-5-5"
"""

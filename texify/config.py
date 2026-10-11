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
    # lang/font/currency fall back to en/sans/EUR at render time, so that
    # template defaults (e.g. a serif contract) are not overridden here
    "defaults": {},
    "build": {"engine": "auto", "out_dir": "", "docker_image": "texify:latest"},
    "templates": {"paths": ["templates"]},
    # AI is provider-agnostic: any command that reads a prompt on stdin and
    # prints the answer on stdout. Empty = AI only through the MCP server.
    "ai": {"command": "", "attempts": 3, "timeout": 900},
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
        raise TexifyError(f"{path}: invalid TOML: {exc}") from None


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
# TeXify project configuration
# Everything under [defaults] pre-fills EVERY document in this folder (and its
# subfolders). A document's frontmatter overrides these values.

[defaults]
lang = "en"            # en | de
accent = "1F4E79"      # accent colour (hex)
# font = "sans"        # sans | serif – otherwise the template decides (contract: serif)
# currency = "EUR"

[defaults.sender]      # sender for letters, invoices, confirmations
name = "Alex Example"
street = "1 Sample Street"
zip = "12345"
city = "Sampletown"
phone = "+49 170 1234567"
email = "alex@example.com"
# web = "example.com"
# tax_id = "123/456/78901"        # tax number (invoices)
# vat_id = "DE123456789"          # VAT ID (invoices)

# [defaults.sender.bank]
# name = "Example Bank"
# iban = "DE00 0000 0000 0000 0000 00"
# bic = "XXXXDEXXXXX"

[build]
engine = "auto"        # auto | tectonic | latexmk | pdflatex | xelatex | lualatex | docker
out_dir = ""           # empty = PDF next to the source file

[templates]
paths = ["templates"]  # your own templates (override built-ins with the same name)

[ai]
# Optional, for `texify ai ...` on the command line. Any LLM tool that reads the
# prompt on stdin and prints the answer, e.g. "llm -m gpt-4o", "ollama run
# llama3.1", "claude -p". Leave empty to use AI only through the MCP server.
command = ""
"""

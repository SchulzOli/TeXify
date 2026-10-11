"""Template discovery, metadata and the Jinja environment.

A template is a folder::

    <name>/
      template.tex    Jinja2 template with LaTeX-friendly delimiters (see guide.md)
      template.yaml   description, fields, defaults, heading map, i18n strings
      example.md      a complete example document (used by `texify new`)
      ...             any other file (logo, .sty, images) is copied into the build

Search order (later wins): built-in -> ~/.texify/templates -> project templates/.
"""

from __future__ import annotations

import datetime as dt
import re
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path
from typing import Any, Callable

import jinja2
import jinja2.ext
import yaml

from . import TexifyError
from .config import Config
from .latex import (
    Tex,
    color_hex,
    escape,
    format_date,
    format_money,
    format_number,
    load_yaml,
    to_date,
    to_decimal,
)
from .markdown import LatexRenderer

BUILTIN_DIR = Path(__file__).resolve().parent / "templates"
SHARED_DIR = BUILTIN_DIR / "_shared"
TEMPLATE_FILE = "template.tex"
META_FILE = "template.yaml"
EXAMPLE_FILE = "example.md"
TEMPLATE_FILES = {TEMPLATE_FILE, META_FILE, EXAMPLE_FILE}
META_KEYS = {"description", "fields", "defaults", "headings", "strings", "title_from_heading"}


@dataclass
class Template:
    name: str
    path: Path
    meta: dict
    origin: str  # builtin | user | project

    @property
    def description(self) -> str:
        return str(self.meta.get("description", "")).strip()

    @property
    def fields(self) -> dict[str, dict]:
        out = {}
        for key, spec in (self.meta.get("fields") or {}).items():
            out[key] = {"description": spec} if isinstance(spec, str) else dict(spec or {})
        return out

    @property
    def defaults(self) -> dict:
        return self.meta.get("defaults") or {}

    @property
    def headings(self) -> dict | None:
        return self.meta.get("headings")

    @property
    def strings(self) -> dict:
        return self.meta.get("strings") or {}

    @property
    def title_from_heading(self) -> bool:
        return bool(self.meta.get("title_from_heading", False))

    def example(self) -> str:
        p = self.path / EXAMPLE_FILE
        return p.read_text(encoding="utf-8") if p.is_file() else f"---\ntemplate: {self.name}\n---\n"

    def source(self) -> str:
        return (self.path / TEMPLATE_FILE).read_text(encoding="utf-8")

    def asset_files(self) -> list[Path]:
        return sorted(p for p in self.path.iterdir() if p.is_file() and p.name not in TEMPLATE_FILES)

    def missing_fields(self, ctx: dict) -> list[str]:
        missing = []
        for key, spec in self.fields.items():
            if spec.get("required") and _lookup(ctx, key) in (None, "", [], {}):
                missing.append(key)
        return missing


def _lookup(ctx: dict, dotted: str) -> Any:
    cur: Any = ctx
    for part in dotted.split("."):
        if not isinstance(cur, dict) or part not in cur:
            return None
        cur = cur[part]
    return cur


def load_template_meta(path: Path) -> dict:
    meta_file = path / META_FILE
    if not meta_file.is_file():
        return {}
    try:
        meta = load_yaml(meta_file.read_text(encoding="utf-8")) or {}
    except yaml.YAMLError as exc:
        raise TexifyError(f"{meta_file}: ungültiges YAML: {exc}") from None
    if not isinstance(meta, dict):
        raise TexifyError(f"{meta_file}: muss ein YAML-Mapping sein")
    unknown = set(meta) - META_KEYS
    if unknown:
        raise TexifyError(f"{meta_file}: unbekannte Schlüssel: {', '.join(sorted(unknown))}")
    return meta


def discover(cfg: Config) -> dict[str, Template]:
    found: dict[str, Template] = {}
    *project_dirs, user_dir = cfg.template_dirs()
    roots = [(BUILTIN_DIR, "builtin"), (user_dir, "user")]
    roots += [(d, "project") for d in project_dirs]
    for root, origin in roots:
        if not root.is_dir():
            continue
        for d in sorted(root.iterdir()):
            if d.is_dir() and not d.name.startswith(("_", ".")) and (d / TEMPLATE_FILE).is_file():
                found[d.name] = Template(d.name, d, load_template_meta(d), origin)
    return found


def get_template(name: str, cfg: Config) -> Template:
    templates = discover(cfg)
    if name not in templates:
        avail = ", ".join(sorted(templates)) or "-"
        raise TexifyError(
            f"Template '{name}' nicht gefunden. Verfügbar: {avail}\n"
            f"Neues Template per KI: texify ai template {name} \"<Beschreibung>\""
        )
    return templates[name]


# --------------------------------------------------------------------------- #
# Jinja environment
# --------------------------------------------------------------------------- #


def invoice_totals(items: list, vat: Any = 19, small_business: bool = False) -> dict:
    """Compute line totals, net, VAT and gross with exact decimal arithmetic."""
    if not isinstance(items, list) or not items:
        raise TexifyError("Rechnung braucht mindestens eine Position unter 'items'")
    rows = []
    net = Decimal(0)
    for i, it in enumerate(items, 1):
        if not isinstance(it, dict) or "price" not in it:
            raise TexifyError(f"Position {i}: braucht mindestens 'desc' und 'price'")
        qty = to_decimal(it.get("qty", 1), f"Menge in Position {i}")
        price = to_decimal(it["price"], f"Preis in Position {i}")
        total = (qty * price).quantize(Decimal("0.01"))
        net += total
        rows.append({**it, "qty": qty, "price": price, "total": total})
    rate = Decimal(0) if small_business else to_decimal(vat, "USt-Satz")
    vat_amount = (net * rate / 100).quantize(Decimal("0.01"))
    return {"items": rows, "net": net, "vat_rate": rate, "vat": vat_amount, "gross": net + vat_amount}


def sum_amounts(items: list, key: str = "amount") -> Decimal:
    return sum((to_decimal(it.get(key, 0)) for it in items or []), Decimal(0))


def add_days(value: Any, days: Any) -> dt.date:
    d = to_date(value)
    if d is None:
        raise TexifyError(f"Kein gültiges Datum: {value!r}")
    return d + dt.timedelta(days=int(days))


class _SeparateCommands(jinja2.ext.Extension):
    r"""Insert ``{}`` between a LaTeX command and a following ``\VAR{``/``\BLOCK{``.

    Without it ``\selectfont\VAR{name}`` would render as ``\selectfontErika``
    (an undefined control sequence).
    """

    _PATTERN = re.compile(r"(\\[A-Za-z@]+)(?=\\(?:VAR|BLOCK)\{)")

    def preprocess(self, source: str, name: str | None, filename: str | None = None) -> str:
        return self._PATTERN.sub(r"\1{}", source)


def make_environment(
    template: Template,
    lang: str,
    currency: str,
    renderer: LatexRenderer,
    asset: Callable[[str], str],
) -> jinja2.Environment:
    def finalize(value: Any) -> str:
        if isinstance(value, Tex):
            return value
        if value is None or isinstance(value, jinja2.Undefined):
            return ""
        if isinstance(value, bool):
            return "true" if value else "false"
        if isinstance(value, (int, float, Decimal)):
            return str(value)
        if isinstance(value, (dt.date, dt.datetime)):
            return escape(format_date(value, lang))
        if isinstance(value, (list, tuple)):
            return escape(", ".join(str(v) for v in value))
        return escape(value)

    def lines(value: Any, sep: str = r"\\") -> Tex:
        if value is None or isinstance(value, jinja2.Undefined):
            return Tex("")
        items = value if isinstance(value, (list, tuple)) else str(value).strip().splitlines()
        return Tex(sep.join(escape(str(v).strip()) for v in items if str(v).strip()))

    def texjoin(values: Any, sep: str = ", ") -> Tex:
        return Tex(escape(sep).join(escape(v) for v in (values or [])))

    env = jinja2.Environment(
        loader=jinja2.FileSystemLoader([str(template.path), str(SHARED_DIR)]),
        block_start_string=r"\BLOCK{",
        block_end_string="}",
        variable_start_string=r"\VAR{",
        variable_end_string="}",
        comment_start_string=r"\#{",
        comment_end_string="}",
        line_statement_prefix="%:",
        line_comment_prefix="%#",
        trim_blocks=True,
        lstrip_blocks=True,
        keep_trailing_newline=True,
        autoescape=False,
        undefined=jinja2.ChainableUndefined,
        finalize=finalize,
        extensions=[_SeparateCommands],
    )
    env.filters.update({
        "md": lambda v: renderer.render_inline("" if v is None else str(v)),
        "mdblock": lambda v: renderer.render("" if v is None else str(v)),
        "tex": lambda v: Tex("" if v is None else str(v)),
        "lines": lines,
        "texjoin": texjoin,
        "date": lambda v, style="long": escape(format_date(v, lang, style)),
        "num": lambda v, decimals=None: escape(format_number(v, lang, decimals)),
        "money": lambda v, cur=None: escape(format_money(v, lang, cur or currency)),
        "color": color_hex,
        "asset": lambda v: Tex(asset(str(v))),
    })
    env.globals.update({
        "invoice_totals": invoice_totals,
        "sum_amounts": sum_amounts,
        "add_days": add_days,
        "today": dt.date.today(),
    })
    return env


def render_template(env: jinja2.Environment, template: Template, ctx: dict) -> str:
    try:
        return env.get_template(TEMPLATE_FILE).render(**ctx)
    except TexifyError:
        raise
    except jinja2.TemplateSyntaxError as exc:
        raise TexifyError(f"Template '{template.name}' Zeile {exc.lineno}: {exc.message}") from None
    except jinja2.TemplateError as exc:
        raise TexifyError(f"Template '{template.name}': {exc}") from None

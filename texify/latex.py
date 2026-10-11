"""LaTeX escaping and value formatting helpers (locale-aware)."""

from __future__ import annotations

import datetime as dt
import re
from decimal import ROUND_HALF_UP, Decimal, InvalidOperation
from typing import Any

import yaml

from . import TexifyError


class Tex(str):
    """A string that already is valid LaTeX and must not be escaped again."""


class YamlLoader(yaml.SafeLoader):
    """SafeLoader without YAML 1.1 surprises in documents.

    Only plain decimals become integers: ``10:00`` stays a time string
    (not 600) and ``01234`` stays a postcode (not octal 668).
    """


YamlLoader.yaml_implicit_resolvers = {
    key: [r for r in resolvers if r[0] != "tag:yaml.org,2002:int"]
    for key, resolvers in yaml.SafeLoader.yaml_implicit_resolvers.items()
}
YamlLoader.add_implicit_resolver(
    "tag:yaml.org,2002:int", re.compile(r"^[-+]?(?:0|[1-9][0-9_]*)$"), list("-+0123456789"))


def load_yaml(text: str) -> Any:
    return yaml.load(text, Loader=YamlLoader)  # noqa: S506 - SafeLoader subclass


_ESCAPES = {
    "\\": r"\textbackslash{}",
    "&": r"\&",
    "%": r"\%",
    "$": r"\$",
    "#": r"\#",
    "_": r"\_",
    "{": r"\{",
    "}": r"\}",
    "~": r"\textasciitilde{}",
    "^": r"\textasciicircum{}",
    # babel-ngerman makes " an active shorthand character
    '"': r"\textquotedbl{}",
    # characters that pdflatex + T1 cannot typeset directly
    " ": "~",
    " ": r"\,",
    " ": r"\,",
    "−": r"\textminus{}",
    "→": r"\ensuremath{\rightarrow}",
    "←": r"\ensuremath{\leftarrow}",
    "≤": r"\ensuremath{\leq}",
    "≥": r"\ensuremath{\geq}",
    "×": r"\ensuremath{\times}",
    "✓": r"\ensuremath{\checkmark}",
}


def escape(value: Any) -> Tex:
    """Escape plain text for LaTeX. Values that are already ``Tex`` pass through."""
    if isinstance(value, Tex):
        return value
    if value is None:
        return Tex("")
    return Tex("".join(_ESCAPES.get(ch, ch) for ch in str(value)))


def escape_url(url: str) -> str:
    """Escape a URL for use inside \\href{...}{} / \\url{}."""
    return url.replace("\\", "/").replace("%", r"\%").replace("#", r"\#")


# --------------------------------------------------------------------------- #
# Language
# --------------------------------------------------------------------------- #

LANGS = {"de", "en"}


def normalize_lang(value: Any) -> str:
    v = str(value or "en").strip().lower()
    if v.startswith(("de", "german", "ngerman")):
        return "de"
    if v.startswith(("en", "english")):
        return "en"
    raise TexifyError(f"Language {value!r} is not supported (allowed: en, de)")


SMART_QUOTES = {"de": "„“‚‘", "en": "“”‘’"}

_MONTHS = {
    "de": ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli",
           "August", "September", "Oktober", "November", "Dezember"],
    "en": ["January", "February", "March", "April", "May", "June", "July",
           "August", "September", "October", "November", "December"],
}


# --------------------------------------------------------------------------- #
# Formatting
# --------------------------------------------------------------------------- #


def to_date(value: Any) -> dt.date | None:
    """Parse YAML dates, ISO strings, German dd.mm.yyyy, or 'today'."""
    if value is None or value == "":
        return None
    if isinstance(value, dt.datetime):
        return value.date()
    if isinstance(value, dt.date):
        return value
    s = str(value).strip()
    if s.lower() in {"today", "heute"}:
        return dt.date.today()
    for fmt in ("%Y-%m-%d", "%d.%m.%Y", "%d.%m.%y"):
        try:
            return dt.datetime.strptime(s, fmt).date()
        except ValueError:
            pass
    return None


def format_date(value: Any, lang: str, style: str = "long") -> str:
    """Format a date; unparseable values are returned unchanged (e.g. '2020 – heute')."""
    d = to_date(value)
    if d is None:
        return "" if value is None else str(value)
    if style == "short":
        return d.strftime("%d.%m.%Y") if lang == "de" else d.isoformat()
    month = _MONTHS[lang][d.month - 1]
    return f"{d.day}. {month} {d.year}" if lang == "de" else f"{month} {d.day}, {d.year}"


def to_decimal(value: Any, what: str = "number") -> Decimal:
    if isinstance(value, Decimal):
        return value
    if isinstance(value, (int, float)):
        return Decimal(str(value))
    s = str(value).strip().replace("€", "").replace(" ", "")
    # accept German notation 1.234,50
    if "," in s:
        s = s.replace(".", "").replace(",", ".")
    try:
        return Decimal(s)
    except InvalidOperation:
        raise TexifyError(f"Invalid {what}: {value!r}") from None


def format_number(value: Any, lang: str, decimals: int | None = None) -> str:
    d = to_decimal(value)
    if decimals is None:
        # drop trailing zeros: 1.50 -> 1,5 ; 8 -> 8
        s = format(d.normalize(), "f")
        if "." in s:
            s = s.rstrip("0").rstrip(".")
        int_part, _, frac = s.partition(".")
    else:
        q = d.quantize(Decimal(1).scaleb(-decimals), rounding=ROUND_HALF_UP)
        int_part, _, frac = format(q, "f").partition(".")
    neg = int_part.startswith("-")
    int_part = int_part.lstrip("-")
    groups = []
    while len(int_part) > 3:
        groups.insert(0, int_part[-3:])
        int_part = int_part[:-3]
    groups.insert(0, int_part)
    thousands, dec = (".", ",") if lang == "de" else (",", ".")
    out = thousands.join(groups)
    if frac:
        out += dec + frac
    return ("-" if neg else "") + out


CURRENCY_SYMBOLS = {"EUR": "€", "USD": "$", "GBP": "£", "CHF": "CHF"}


def format_money(value: Any, lang: str, currency: str = "EUR") -> str:
    num = format_number(value, lang, decimals=2)
    sym = CURRENCY_SYMBOLS.get(str(currency).upper(), str(currency))
    if lang == "de" or len(sym) > 1:
        return f"{num} {sym}"
    return f"{sym}{num}"


_HEX = re.compile(r"^#?([0-9A-Fa-f]{6})$")


def color_hex(value: Any) -> str:
    m = _HEX.match(str(value or "").strip())
    if not m:
        raise TexifyError(f"Colour must be a hex value (e.g. 1F4E79), not {value!r}")
    return m.group(1).upper()

"""Markdown -> LaTeX renderer (markdown-it-py syntax tree, deterministic output).

The output only relies on packages/macros provided by ``texify.sty``.
Templates control how headings are rendered via a heading map, e.g. a CV maps
``## 2020 – 2022 | Master of Science`` to ``\\cventry{2020 – 2022}{Master of Science}``.
"""

from __future__ import annotations

import math
import re
from dataclasses import dataclass
from typing import Callable

from markdown_it import MarkdownIt
from markdown_it.tree import SyntaxTreeNode

from . import TexifyError
from .latex import SMART_QUOTES, Tex, escape, escape_url


@dataclass
class HeadingStyle:
    """How one (normalized) heading level is rendered.

    cmd:   LaTeX command name, emitted as ``\\cmd{arg1}{arg2}...``
    env:   LaTeX environment name; wraps everything up to the next heading of
           the same or a higher level: ``\\begin{env}{args}...\\end{env}``
    args:  number of arguments; heading text is split by ``split`` into args
    pad:   where missing args are padded with empty strings ("start" | "end")
    """

    cmd: str | None = None
    env: str | None = None
    args: int = 1
    split: str | None = None
    pad: str = "end"

    @classmethod
    def from_spec(cls, spec: dict | str) -> "HeadingStyle":
        if isinstance(spec, str):
            return cls(cmd=spec)
        unknown = set(spec) - {"cmd", "env", "args", "split", "pad"}
        if unknown:
            raise TexifyError(f"Unknown heading options: {', '.join(sorted(unknown))}")
        style = cls(**spec)
        if bool(style.cmd) == bool(style.env):
            raise TexifyError("A heading style needs exactly one of 'cmd' or 'env'")
        return style


DEFAULT_HEADINGS = {
    1: HeadingStyle(cmd="section"),
    2: HeadingStyle(cmd="subsection"),
    3: HeadingStyle(cmd="subsubsection"),
    4: HeadingStyle(cmd="paragraph"),
}

RAW_LATEX_INFO = {"{=latex}", "=latex"}
SHORT_COLUMN = 18  # table columns up to this many characters keep natural width

_SLUG = re.compile(r"[^a-z0-9äöüß]+")


def slug(text: str) -> str:
    return _SLUG.sub("-", text.lower()).strip("-")


class LatexRenderer:
    def __init__(
        self,
        lang: str = "en",
        headings: dict | None = None,
        asset: Callable[[str], str] | None = None,
    ):
        self.md = MarkdownIt("commonmark", {"html": False, "typographer": True})
        self.md.enable(["table", "strikethrough", "replacements", "smartquotes"])
        self.md.options["quotes"] = SMART_QUOTES[lang]
        if headings:
            self.headings = {int(k): HeadingStyle.from_spec(v) for k, v in headings.items()}
        else:
            self.headings = dict(DEFAULT_HEADINGS)
        self.asset = asset

    # ------------------------------------------------------------------ API

    def render(self, text: str) -> Tex:
        """Render a full Markdown document body."""
        return self._render_tokens(self.md.parse(text or ""), base=1)

    def render_inline(self, text: str) -> Tex:
        """Render a single line of inline Markdown (**bold**, *em*, `code`, links)."""
        if text is None:
            return Tex("")
        lines = [ln.strip() for ln in str(text).strip().splitlines()]
        tokens = self.md.parseInline(" ".join(lines))
        if not tokens:
            return Tex("")
        inline = SyntaxTreeNode(tokens).children[0]
        return Tex(self._inline_children(inline.children))

    def sections(self, text: str) -> list[dict]:
        """Split a document at its top-level headings and render each part.

        Returns ``[{"title", "raw_title", "slug", "body"}]``; content before the
        first heading becomes a section with an empty title.
        """
        tokens = self.md.parse(text or "")
        offset = _heading_offset(tokens)
        lines = (text or "").splitlines()
        cuts = [
            (t.map[0], t.map[1], tokens[i + 1].content)
            for i, t in enumerate(tokens)
            if t.type == "heading_open" and int(t.tag[1]) - offset == 1 and t.map
        ]
        result = []
        start = 0
        title = ""
        for h_start, h_end, h_text in cuts + [(len(lines), len(lines), None)]:
            chunk = "\n".join(lines[start:h_start])
            if chunk.strip() or title:
                result.append({
                    "title": self.render_inline(title),
                    "raw_title": title,
                    "slug": slug(title),
                    "body": self._render_tokens(self.md.parse(chunk), base=2),
                })
            start, title = h_end, h_text or ""
        return result

    def extract_title(self, text: str) -> tuple[str | None, str]:
        """If the document starts with a level-1 heading, split it off as title."""
        tokens = self.md.parse(text or "")
        if tokens and tokens[0].type == "heading_open" and tokens[0].tag == "h1" and tokens[0].map:
            lines = (text or "").splitlines()
            a, b = tokens[0].map
            return tokens[1].content, "\n".join(lines[:a] + lines[b:])
        return None, text

    # ------------------------------------------------------------- internals

    def _render_tokens(self, tokens, base: int) -> Tex:
        """Render block tokens; the shallowest heading is normalized to ``base``."""
        offset = _heading_offset(tokens) - (base - 1)
        out: list[str] = []
        open_envs: list[tuple[int, str]] = []
        for node in SyntaxTreeNode(tokens).children:
            if node.type == "heading":
                level = int(node.tag[1]) - offset
                while open_envs and open_envs[-1][0] >= level:
                    out.append(f"\\end{{{open_envs.pop()[1]}}}\n")
                out.append(self._heading(node, level, open_envs))
            else:
                out.append(self._block(node))
        while open_envs:
            out.append(f"\\end{{{open_envs.pop()[1]}}}\n")
        return Tex("".join(out).strip() + "\n")

    def _style(self, level: int) -> HeadingStyle:
        if level in self.headings:
            return self.headings[level]
        deeper = [lv for lv in self.headings if lv <= level]
        return self.headings[max(deeper)] if deeper else self.headings[min(self.headings)]

    def _heading(self, node: SyntaxTreeNode, level: int, open_envs: list) -> str:
        style = self._style(level)
        raw = node.children[0].content if node.children else ""
        parts = [p.strip() for p in raw.split(style.split)] if style.split else [raw.strip()]
        if len(parts) > style.args:  # too many separators: merge the tail
            parts = parts[: style.args - 1] + [f" {style.split} ".join(parts[style.args - 1:])]
        missing = [""] * (style.args - len(parts))
        parts = missing + parts if style.pad == "start" else parts + missing
        args = "".join("{" + self.render_inline(p) + "}" for p in parts)
        if style.env:
            open_envs.append((level, style.env))
            return f"\\begin{{{style.env}}}{args}\n"
        return f"\\{style.cmd}{args}\n\n"

    def _block(self, node: SyntaxTreeNode) -> str:
        t = node.type
        if t == "paragraph":
            kids = node.children[0].children if node.children else []
            if len(kids) == 1 and kids[0].type == "image":
                return "\\begin{center}\n" + self._inline(kids[0]) + "\n\\end{center}\n\n"
            content = self._inline_children(kids)
            tight = node.nester_tokens.opening.hidden
            return content + ("\n" if tight else "\n\n")
        if t in ("bullet_list", "ordered_list"):
            env = "itemize" if t == "bullet_list" else "enumerate"
            opts = ""
            start = node.attrs.get("start") if t == "ordered_list" else None
            if start and int(start) != 1:
                opts = f"[start={int(start)}]"
            items = "".join(self._block(child) for child in node.children)
            return f"\\begin{{{env}}}{opts}\n{items}\\end{{{env}}}\n\n"
        if t == "list_item":
            body = "".join(self._block(child) for child in node.children).strip()
            return f"  \\item {body}\n"
        if t == "blockquote":
            body = "".join(self._block(child) for child in node.children).strip()
            return f"\\begin{{quote}}\n{body}\n\\end{{quote}}\n\n"
        if t in ("fence", "code_block"):
            info = (node.info or "").strip() if t == "fence" else ""
            if info in RAW_LATEX_INFO:
                return node.content.rstrip() + "\n\n"
            return "\\begin{Verbatim}[fontsize=\\small]\n" + node.content.rstrip("\n") + "\n\\end{Verbatim}\n\n"
        if t == "hr":
            return "\\texifyrule\n\n"
        if t == "table":
            return self._table(node)
        if t == "html_block":
            return escape(node.content) + "\n\n"
        if t == "heading":  # nested heading (e.g. inside blockquote): plain bold line
            return "\\textbf{" + self._inline_children(node.children[0].children) + "}\n\n"
        return "".join(self._block(child) for child in node.children)

    def _table(self, node: SyntaxTreeNode) -> str:
        rows: list[tuple[bool, list[SyntaxTreeNode]]] = []
        for section in node.children:  # thead / tbody
            for tr in section.children:
                rows.append((section.type == "thead", tr.children))
        if not rows:
            return ""
        ncols = len(rows[0][1])
        widths = [0] * ncols
        for _, cells in rows:
            for i, cell in enumerate(cells[:ncols]):
                raw = cell.children[0].content if cell.children else ""
                widths[i] = max(widths[i], len(raw))
        kinds = []
        for i, cell in enumerate(rows[0][1]):
            style = cell.attrs.get("style", "")
            if "right" in style:
                kinds.append("r")
            elif "center" in style:
                kinds.append("c")
            else:  # short text columns keep their natural width
                kinds.append("l" if widths[i] <= SHORT_COLUMN else "X")
        if "X" not in kinds:  # let the widest text column fill the line
            left = [i for i, k in enumerate(kinds) if k == "l"]
            if left:
                kinds[max(left, key=lambda i: widths[i])] = "X"
        flex = [i for i, k in enumerate(kinds) if k == "X"]
        if len(flex) == 1:
            kinds[flex[0]] = "L"
        elif flex:  # share the remaining width by (damped) content length
            weights = {i: math.sqrt(max(widths[i], 1)) for i in flex}
            total = sum(weights.values())
            for i in flex:
                kinds[i] = f"Y{{{len(flex) * weights[i] / total:.2f}}}"
        has_x = bool(flex)
        spec = "@{}" + " ".join(kinds) + "@{}"
        lines = []
        for is_head, cells in rows:
            rendered = [self._inline_children(c.children[0].children) if c.children else "" for c in cells]
            if is_head:
                rendered = [f"\\textbf{{{c}}}" if c else "" for c in rendered]
            lines.append(" & ".join(rendered) + r" \\")
            if is_head:
                lines.append(r"\midrule")
        begin = f"\\begin{{tabularx}}{{\\linewidth}}{{{spec}}}" if has_x else f"\\begin{{tabular}}{{{spec}}}"
        end = "\\end{tabularx}" if has_x else "\\end{tabular}"
        body = "\n".join(lines)
        return f"\\par\\medskip\\noindent\n{begin}\n\\toprule\n{body}\n\\bottomrule\n{end}\n\\par\\medskip\n\n"

    def _inline_children(self, children) -> str:
        return "".join(self._inline(c) for c in children)

    def _inline(self, node) -> str:
        t = node.type
        if t == "text":
            return escape(node.content)
        if t == "softbreak":
            return "\n"
        if t == "hardbreak":
            return "\\\\\n"
        if t == "strong":
            return "\\textbf{" + self._inline_children(node.children) + "}"
        if t == "em":
            return "\\emph{" + self._inline_children(node.children) + "}"
        if t == "s":
            return "\\sout{" + self._inline_children(node.children) + "}"
        if t == "code_inline":
            return "\\texttt{" + escape(node.content) + "}"
        if t == "link":
            href = node.attrs.get("href", "")
            text = self._inline_children(node.children)
            return f"\\href{{{escape_url(href)}}}{{{text}}}"
        if t == "image":
            src = node.attrs.get("src", "")
            path = self.asset(src) if self.asset else src
            return f"\\texifyimage{{{path}}}"
        if t == "html_inline":
            return escape(node.content)
        return self._inline_children(node.children or [])


def _heading_offset(tokens) -> int:
    levels = [int(t.tag[1]) for t in tokens if t.type == "heading_open"]
    return (min(levels) - 1) if levels else 0

# TeXify – template authoring guide

A template turns a Markdown document with YAML frontmatter into a designed
LaTeX document. **Content** lives in the Markdown (readable, no LaTeX
knowledge needed), **design** lives exclusively in the template.

## Folder layout

```
templates/<name>/
  template.tex    Jinja2 template (LaTeX), required
  template.yaml   metadata (description, fields, defaults, headings, strings)
  example.md      complete example document (copied by `texify new`)
  *.png, *.sty    any other file is copied into the build directory
```

Search order (later wins): built-in → `~/.texify/templates` → project folder
`templates/` (see `texify.toml`).

## Document (input)

```markdown
---
template: letter            # which template
subject: Application for …  # any fields, available as \VAR{subject}
recipient: |
  Example Ltd.
  1 High Street
  12345 Sampletown
---

Running text in **Markdown**: lists, tables, links, images, quotes, code.
```

Values from `texify.toml` under `[defaults]` (e.g. `[defaults.sender]`) are
merged with the frontmatter; the frontmatter wins. YAML is read without the
YAML 1.1 number surprises: `10:00` and `01234` stay text.

## Template syntax (Jinja2 with LaTeX-friendly delimiters)

| Purpose            | Syntax                                   |
|--------------------|------------------------------------------|
| Output a value     | `\VAR{subject}`                          |
| Inline block       | `\BLOCK{if photo}...\BLOCK{endif}`       |
| Line statement     | `%: for it in items` … `%: endfor`        |
| Comment            | `%# comment` or `\#{ comment }`          |

* All output is **LaTeX-escaped automatically** (`&`, `%`, `_`, `"` …).
  Ready-made LaTeX (e.g. `body`) is not escaped twice.
* Missing fields render as an empty string; `sender.name` also works when
  `sender` is missing. Mark mandatory fields with `required: true`.
* A LaTeX command directly before a value is separated automatically
  (`\selectfont\VAR{name}` renders as `\selectfont{}Alex`).
* Access dict keys that look like methods (`items`, `keys`, `values`) as
  `x['items']` instead of `x.items`.
* Expressions must not contain `}` (it ends `\VAR{`/`\BLOCK{`).

### Context variables

| Variable   | Content                                                          |
|------------|------------------------------------------------------------------|
| `body`     | the whole Markdown text as LaTeX                                 |
| `sections` | list of `{title, raw_title, slug, body}`, one per top-level heading |
| `markdown` | the raw Markdown text                                            |
| `txt`      | texts from `strings.<lang>` in template.yaml (fallback: `en`)    |
| `lang`     | `en` or `de`;  `font`: `sans`/`serif`;  `accent`: hex colour     |
| all frontmatter/default fields under their own name                          |

### Filters

| Filter                 | Effect                                                   |
|------------------------|----------------------------------------------------------|
| `x\|md`                 | inline Markdown → LaTeX (`**bold**`, links, `code`)      |
| `x\|mdblock`            | block Markdown → LaTeX (paragraphs, lists, tables)       |
| `x\|lines`              | multi-line text or list → lines joined with `\\`         |
| `x\|texjoin(', ')`      | escape a list and join it                                |
| `x\|date` / `date('short')` | language-aware date ("October 11, 2026" / "2026-10-11"; de: "11. Oktober 2026" / "11.10.2026"); `today` allowed |
| `x\|num` / `num(2)`     | language-aware number (1,234.5 / de: 1.234,5)            |
| `x\|money`              | amount with currency (€1,234.50 / de: 1.234,50 €)        |
| `x\|color`              | validate a hex colour (for `\definecolor{..}{HTML}{..}`) |
| `x\|asset`              | copy a file (photo, logo, signature) into the build → path for `\includegraphics` |
| `x\|tex`                | output the value unchanged as LaTeX (trusted values only) |

### Global functions

* `invoice_totals(items, vat, small_business)` → `{items (with total), net, vat_rate, vat, gross}` (exact, Decimal)
* `sum_amounts(list, key='amount')` → sum
* `add_days(date, n)` → date + n days;  `today` → today's date

## Mandatory preamble

Every template loads the shared design package **right after `\documentclass`**:

```latex
\documentclass[fontsize=11pt,paper=a4,parskip=half]{scrartcl}
\usepackage[\VAR{lang},\VAR{font}]{texify}
\texifyaccent{\VAR{accent}}
```

`texify.sty` provides: fonts (TeX Gyre Heros/Pagella, pdfLaTeX and XeLaTeX),
babel (en/de), xcolor with `accent`, `texifytext`, `texifymuted`,
`texifyrule`, `texifybg`; graphicx, tabularx (+ column types `L` = ragged X and
`Y{w}` = weighted ragged X), booktabs, enumitem, fancyvrb, ulem, amssymb,
hyperref. Macros: `\texifyrule`, `\texifyimage{file}`, `\texifyheading{Text}`,
`\texifyspaced{Text}` (letter-spacing). Letter-like templates may additionally
load `texify-letter` (KOMA scrlttr2 layout).

Load further packages after `\usepackage{texify}`. Only packages from TeX Live
*latex-recommended/latex-extra/fonts-recommended/pictures* are available (e.g.
KOMA-Script, tcolorbox, paracol, tikz, pgfornament, geometry,
scrlayer-scrpage). Not available: fontawesome, system fonts, `--shell-escape`.

## Markdown output and headings

The renderer emits `itemize`/`enumerate`, `tabularx` with booktabs, `quote`,
`Verbatim`, `\href`, `\texifyimage`, `\textbf`, `\emph`, `\sout`. A code block
with the info string `{=latex}` is passed through raw.

Headings are normalised: the shallowest heading in a document is level 1.
`template.yaml` maps levels under `headings` (default: 1 → `\section`,
2 → `\subsection`, 3 → `\subsubsection`):

```yaml
headings:
  1: cvsection                                   # \cvsection{Title}
  2: {cmd: cventry, args: 2, split: "|", pad: start}
  #   "## 2020 – 2022 | MSc, TU Dresden" → \cventry{2020 – 2022}{MSc, TU Dresden}
  3: {env: block, args: 1}                       # \begin{block}{Title} … \end{block}
```

`env` headings wrap everything up to the next heading of the same or a higher
level. The template must define every command/environment it maps to. Levels
inside `sections[i].body` start at 2.

## template.yaml

```yaml
description: One sentence on what the template is for.
title_from_heading: false     # true: the first "# Title" becomes title
fields:                       # documentation + validation
  name: {required: true, description: Full name}
  photo: Photo (optional)     # short form = description
defaults:                     # pre-set values (lowest priority)
  date: today
headings: {}                  # see above
strings:                      # language-dependent texts → \VAR{txt.x}
  en: {title: Confirmation}
  de: {title: Bestätigung}
```

## Design rules

* Professional and calm: one accent colour (`accent`), plenty of white space,
  clear hierarchy, no effects. A4, margins ≥ 1.5 cm.
* Every visible default text goes through `strings` (en **and** de), never
  hard-coded.
* Guard optional fields with `%: if field`.
* Use the Markdown structure (heading map, `sections`) instead of forcing users
  to write running text into YAML. YAML only for real data (addresses,
  amounts, line items, key facts).
* The template must compile with pdfLaTeX (XeLaTeX desirable).

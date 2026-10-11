# TeXify – Template-Leitfaden

Ein Template macht aus einem Markdown-Dokument mit YAML-Frontmatter ein
gestaltetes LaTeX-Dokument. **Inhalt** steht im Markdown (lesbar, ohne
LaTeX-Kenntnisse), **Design** steht ausschließlich im Template.

## Ordnerstruktur

```
templates/<name>/
  template.tex    Jinja2-Template (LaTeX), Pflicht
  template.yaml   Metadaten (Beschreibung, Felder, Defaults, Headings, Texte)
  example.md      vollständiges Beispieldokument (wird von `texify new` kopiert)
  *.png, *.sty    weitere Dateien werden ins Build-Verzeichnis kopiert
```

Suchreihenfolge (später gewinnt): eingebaut → `~/.texify/templates` →
Projektordner `templates/` (siehe `texify.toml`).

## Dokument (Eingabe)

```markdown
---
template: letter            # welches Template
subject: Bewerbung als …    # beliebige Felder, im Template als \VAR{subject}
recipient: |
  Firma GmbH
  Straße 1
  12345 Stadt
---

Fließtext in **Markdown**: Listen, Tabellen, Links, Bilder, Zitate, Code.
```

Werte aus `texify.toml` unter `[defaults]` (z. B. `[defaults.sender]`) werden
mit dem Frontmatter zusammengeführt; das Frontmatter gewinnt.
Postleitzahlen/IDs mit führender Null in Anführungszeichen setzen (`zip: "04109"`).

## Template-Syntax (Jinja2 mit LaTeX-freundlichen Begrenzern)

| Zweck              | Syntax                                   |
|--------------------|------------------------------------------|
| Wert ausgeben      | `\VAR{subject}`                          |
| Block inline       | `\BLOCK{if photo}...\BLOCK{endif}`       |
| Zeilen-Anweisung   | `%: for it in items` … `%: endfor`        |
| Kommentar          | `%# Kommentar` oder `\#{ Kommentar }`    |

* Alle Ausgaben werden **automatisch LaTeX-escaped** (`&`, `%`, `_`, `"` …).
  Bereits fertiges LaTeX (z. B. `body`) wird nicht doppelt escaped.
* Fehlende Felder ergeben einen leeren String; `sender.name` funktioniert auch,
  wenn `sender` fehlt. Pflichtfelder in `template.yaml` als `required: true`.
* Dict-Schlüssel, die wie Methoden heißen (`items`, `keys`, `values`), mit
  `x['items']` statt `x.items` ansprechen.
* Ausdrücke dürfen kein `}` enthalten (es beendet `\VAR{`/`\BLOCK{`).

### Variablen im Kontext

| Variable   | Inhalt                                                          |
|------------|-----------------------------------------------------------------|
| `body`     | gesamter Markdown-Text als LaTeX                                |
| `sections` | Liste `{title, raw_title, slug, body}` je Überschrift 1. Ebene  |
| `markdown` | Markdown-Rohtext                                                |
| `txt`      | Texte aus `strings.<lang>` der template.yaml                    |
| `lang`     | `de` oder `en`;  `font`: `sans`/`serif`;  `accent`: Hex-Farbe   |
| alle Frontmatter-/Default-Felder unter ihrem Namen                          |

### Filter

| Filter                 | Wirkung                                                  |
|------------------------|----------------------------------------------------------|
| `x\|md`                 | Inline-Markdown → LaTeX (`**fett**`, Links, `code`)      |
| `x\|mdblock`            | Block-Markdown → LaTeX (Absätze, Listen, Tabellen)       |
| `x\|lines`              | mehrzeiliger Text oder Liste → Zeilen mit `\\`           |
| `x\|texjoin(', ')`      | Liste escapen und verbinden                              |
| `x\|date` / `date('short')` | Datum sprachabhängig („11. Oktober 2026“ / 11.10.2026); `today` erlaubt |
| `x\|num` / `num(2)`     | Zahl sprachabhängig (1.234,5)                            |
| `x\|money`              | Betrag mit Währung (1.234,50 €)                          |
| `x\|color`              | Hex-Farbe prüfen (für `\definecolor{..}{HTML}{..}`)     |
| `x\|asset`              | Datei (Foto, Logo, Unterschrift) ins Build kopieren → Pfad für `\includegraphics` |
| `x\|tex`                | Wert unverändert als LaTeX ausgeben (nur für vertrauenswürdige Werte) |


### Globale Funktionen

* `invoice_totals(items, vat, small_business)` → `{items (mit total), net, vat_rate, vat, gross}` (exakt, Decimal)
* `sum_amounts(list, key='amount')` → Summe
* `add_days(date, n)` → Datum + n Tage;  `today` → heutiges Datum

## Pflicht-Präambel

Jedes Template lädt das gemeinsame Designpaket **direkt nach `\documentclass`**:

```latex
\documentclass[fontsize=11pt,paper=a4,parskip=half]{scrartcl}
\usepackage[\VAR{lang},\VAR{font}]{texify}
\texifyaccent{\VAR{accent}}
```

`texify.sty` stellt bereit: Schriften (TeX Gyre Heros/Pagella, pdfLaTeX und
XeLaTeX), babel (de/en), xcolor mit `accent`, `texifytext`, `texifymuted`,
`texifyrule`, `texifybg`; graphicx, tabularx (+ Spaltentyp `L` = linksbündiges X),
booktabs, enumitem, fancyvrb, ulem, amssymb, hyperref. Makros: `\texifyrule`,
`\texifyimage{datei}`, `\texifyheading{Text}`, `\texifyspaced{Text}` (Sperrsatz).

Weitere Pakete nach `\usepackage{texify}` laden. Erlaubt sind nur Pakete aus
TeX Live *latex-recommended/latex-extra/fonts-recommended* (z. B. KOMA-Script,
tcolorbox, paracol, tikz, geometry, scrlayer-scrpage). Nicht verfügbar:
fontawesome, eigene Systemschriften, `--shell-escape`.

## Markdown-Ausgabe und Überschriften

Der Renderer erzeugt: `itemize`/`enumerate`, `tabularx` mit booktabs,
`quote`, `Verbatim`, `\href`, `\texifyimage`, `\textbf`, `\emph`, `\sout`.
Ein Codeblock mit der Info-Zeichenkette `{=latex}` wird roh übernommen.

Überschriften werden normalisiert: die flachste Überschrift im Dokument ist
Ebene 1. Die Abbildung steht in `template.yaml` unter `headings`
(Standard: 1 → `\section`, 2 → `\subsection`, 3 → `\subsubsection`):

```yaml
headings:
  1: cvsection                                   # \cvsection{Titel}
  2: {cmd: cventry, args: 2, split: "|", pad: start}
  #   "## 2020 – 2022 | Master, TU Dresden" → \cventry{2020 – 2022}{Master, TU Dresden}
  3: {env: block, args: 1}                       # \begin{block}{Titel} … \end{block}
```

`env`-Überschriften umschließen alles bis zur nächsten Überschrift gleicher
oder höherer Ebene. Alle verwendeten Befehle/Umgebungen muss das Template
definieren. Die Ebenen in `sections[i].body` beginnen bei 2.

## template.yaml

```yaml
description: Ein Satz, wofür das Template gedacht ist.
title_from_heading: false     # true: erstes "# Titel" wird zu title
fields:                       # Doku + Validierung
  name: {required: true, description: Vollständiger Name}
  photo: Bewerbungsfoto (optional)   # Kurzform = Beschreibung
defaults:                     # Vorbelegungen (niedrigste Priorität)
  date: today
headings: {}                  # siehe oben
strings:                      # sprachabhängige Texte → \VAR{txt.x}
  de: {title: Bestätigung}
  en: {title: Confirmation}
```

## Gestaltungsregeln

* Professionell und ruhig: eine Akzentfarbe (`accent`), viel Weißraum,
  klare Hierarchie, keine Effekte. A4, Ränder ≥ 1,5 cm.
* Alle sichtbaren Standardtexte über `strings` (de **und** en) statt hart codiert.
* Optionale Felder immer mit `%: if feld` absichern.
* Struktur aus Markdown nutzen (Überschriften-Mapping, `sections`) statt den
  Nutzer zu zwingen, Fließtext in YAML zu schreiben. YAML nur für echte Daten
  (Adressen, Beträge, Positionen, Eckdaten).
* Template-Code muss mit pdfLaTeX kompilieren (XeLaTeX wünschenswert).

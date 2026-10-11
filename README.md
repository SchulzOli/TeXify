# TeXify

**Markdown/Text → LaTeX → PDF.** Du schreibst Inhalte in gut lesbarem
Markdown, TeXify setzt sie mit LaTeX-Templates zu professionellen Dokumenten:
tabellarischer Lebenslauf, Bewerbungsanschreiben, Rechnung, Projektantrag,
Bestätigung, Bericht und mehr. Außerdem organisiert es PDFs (zusammenführen,
Seiten auswählen, Bewerbungsmappen bauen).

<p align="center">
  <a href="examples/"><img src="examples/previews/11-lebenslauf.png" width="190"></a>
  <a href="examples/"><img src="examples/previews/08-rechnung.png" width="190"></a>
  <a href="examples/"><img src="examples/previews/19-einladung.png" width="190"></a>
  <a href="examples/"><img src="examples/previews/12-cv-english.png" width="190"></a>
</p>

**→ [21 Beispiele mit Markdown-Quelle und PDF](examples/)**

| Ebene | Wer ist zuständig? |
|---|---|
| **Inhalt** | du, in Markdown + YAML-Frontmatter (ohne LaTeX-Kenntnisse) |
| **Design & Format** | LaTeX-Templates + gemeinsames Designpaket `texify.sty` |
| **Rechnen, Validieren, Bauen, Zusammenführen** | deterministischer Code |
| **KI** | nur, wo Code nicht weiterhilft: neue Templates entwerfen, unstrukturierten Text einem Template zuordnen. Über die CLI oder MCP. |

## Installation

```bash
pip install -e ".[all]"
```

TeXify braucht eine LaTeX-Engine. `texify doctor` zeigt, was gefunden wird.
Ohne lokale TeX-Installation baut TeXify automatisch über das Docker-Image:

```bash
docker compose build
```

Unterstützte Engines in Prioritätsreihenfolge: `tectonic`, `latexmk`, `pdflatex`,
`xelatex`, `lualatex`, `docker`. Eine Engine lässt sich mit `-e`, mit der
Umgebungsvariablen `TEXIFY_ENGINE` oder in `texify.toml` erzwingen.

Ist `texify` nach der Installation nicht im `PATH`, funktioniert jeder Befehl
auch als `python -m texify …`.

## Schnellstart

```bash
texify init                       # texify.toml: Absender, Bank, Farbe … einmal eintragen
texify new letter anschreiben.md  # Beispiel des Templates als Startpunkt
texify build anschreiben.md       # -> anschreiben.pdf
texify build anschreiben.md --watch --open   # live neu bauen beim Speichern
```

## Dokumente schreiben

```markdown
---
template: invoice
recipient: |
  Beispiel GmbH
  Hauptstraße 5
  10115 Berlin
number: 2026-017
items:
  - {desc: Beratung, qty: 16, unit: Std., price: 110}
  - {desc: "Implementierung **CI/CD**", qty: 1, unit: Pausch., price: 1850}
---

vielen Dank für Ihren Auftrag. Für die erbrachten Leistungen berechne ich:
```

- **Frontmatter** (zwischen `---`) für strukturierte Daten, **Markdown** für
  Fließtext, Listen, Tabellen, Bilder, Zitate und Links.
- Werte aus `texify.toml` unter `[defaults]` gelten für alle Dokumente im
  Ordner, zum Beispiel `[defaults.sender]` für Absender und Bank. Das
  Frontmatter überschreibt sie.
- `lang: de|en`, `font: sans|serif` und `accent: 1F4E79` steuern das Design
  jedes Templates.
- Rohes LaTeX geht als Codeblock mit der Kennung `{=latex}`.
- Uhrzeiten (`10:00`) und Postleitzahlen (`01234`) bleiben Text. TeXify liest
  YAML ohne die Zahlenumwandlungen von YAML 1.1 (Sexagesimal- und Oktalzahlen).
- Auch `.txt` (wird als Markdown gelesen) und `.yaml` (nur Daten) sind möglich.

## Templates

| Name | Wofür | Markdown-Konvention |
|---|---|---|
| `article` | Bericht, Anleitung, Lastenheft, Pressemitteilung | `# Titel`, `## Abschnitte`; `toc`, `numbered` |
| `letter` | DIN-5008-Brief: Anschreiben, Kündigung, Mahnung | Fließtext; `recipient`, `subject` |
| `invoice` | Rechnung (§14 UStG, optional §19) und Angebot (`kind: quote`) | `items` im YAML, Summen werden berechnet |
| `cv` | klassischer tabellarischer Lebenslauf | `# Rubrik`, `## Zeitraum \| Position, Firma` |
| `cv-cards` | moderner Karten-Lebenslauf, 2 Spalten | gleiches Markdown wie `cv` |
| `coverpage` | Deckblatt für Bewerbungsmappen | `position`, `company`, `contents` |
| `proposal` | Projekt-/Förderantrag | `facts`, `budget` (Summe automatisch) |
| `confirmation` | Bestätigung, Arbeitszeugnis, Vollmacht | `person`, `intro`, `signer` |
| `minutes` | Protokoll, Tagesordnung | `# TOP`-Überschriften, `actions` |
| `invitation` | Einladung zu Feier, Veranstaltung | `date`, `place`, `program`, `rsvp` |
| `certificate` | Urkunde, Zertifikat (quer) | `person`, `signers` |
| `contract` | Vertrag mit § -Nummerierung | `parties`; `# Klausel`, Absätze als `1.`-Liste |

```bash
texify templates                  # Liste
texify templates show invoice     # Felder + Konventionen
texify templates eject letter     # Kopie nach ./templates/letter zum Anpassen
texify templates guide            # Leitfaden für eigene Templates
texify tex brief.md               # nur LaTeX erzeugen (kompletter Projektordner)
```

**LaTeX ist das Designwerkzeug.** Eigene oder angepasste Templates liegen in
`./templates/<name>/` (Projekt) oder `~/.texify/templates/<name>/` (global) und
haben Vorrang vor den eingebauten. Das gemeinsame Design (Schriften, Farben,
Tabellen, Listen) steckt in
[texify/templates/_shared/texify.sty](texify/templates/_shared/texify.sty).

## PDFs organisieren

```bash
# Bewerbungsmappe: Markdown bauen und vorhandene PDFs anhängen, mit Lesezeichen
texify build anschreiben.md lebenslauf.md zeugnisse/*.pdf -o bewerbung.pdf --bookmarks

texify merge scans/ -o alles.pdf --sort name --bookmarks
texify pages vertrag.pdf 1-3,7 -o auszug.pdf
texify info *.pdf
texify import alter_lebenslauf.pdf          # PDF/DOCX/TXT -> Markdown
```

## KI-Unterstützung

KI kommt nur zum Einsatz, wenn kein passendes Template existiert oder Inhalte
unstrukturiert vorliegen. Jedes KI-Ergebnis wird deterministisch geprüft
(rendern und kompilieren). Fehler gehen automatisch zur Korrektur an die KI
zurück, standardmäßig bis zu 3 Versuche.

```bash
texify ai template kuendigung "Kündigung eines Vertrags mit Einschreiben-Vermerk und Bitte um Bestätigung"
texify ai fill cv alter_lebenslauf.pdf      # PDF/Text -> lebenslauf im cv-Format
```

Backend (`[ai]` in `texify.toml`):

- `anthropic`: Anthropic-API (`ANTHROPIC_API_KEY` oder `ant auth login`).
  Standardmodell ist `claude-opus-5-5`. Die serverseitige Fallback-Funktion ist
  aktiv: Lehnt das Modell eine Anfrage aus Richtlinien-Gründen ab, wird sie
  automatisch mit einem anderen Modell wiederholt.
- `claude-cli`: Claude Code CLI (`claude -p`).
- über **MCP** braucht TeXify kein eigenes Backend, die KI des MCP-Hosts übernimmt.

### MCP-Server

Tools: `list_templates`, `describe_template`, `template_guide`,
`create_template` (speichert erst nach erfolgreichem Test-Build),
`write_document`, `build_document`, `render_latex`, `merge_pdfs`,
`import_document`, `read_file`, `list_files`, `pdf_info`. Alle Pfade sind auf
den Arbeitsordner beschränkt.

Claude Code:

```bash
claude mcp add texify -- python -m texify mcp --workspace "C:/Pfad/zu/Dokumente"
```

Claude Desktop (`claude_desktop_config.json`), alternativ Hermes (`~/.hermes/config.yaml`, gleiche Felder unter `mcp_servers`):

```json
{
  "mcpServers": {
    "texify": {
      "command": "python",
      "args": ["-m", "texify", "mcp", "--workspace", "C:/Pfad/zu/Dokumente"]
    }
  }
}
```

Komplett in Docker (TeX Live im Container):

```json
{ "command": "docker", "args": ["run", "-i", "--rm", "-v", "C:/Pfad/zu/Dokumente:/work", "texify:latest", "mcp"] }
```

## Projektstruktur

```
texify/
  cli.py          Kommandozeile
  build.py        Pipeline Dokument -> LaTeX -> PDF
  markdown.py     Markdown -> LaTeX (markdown-it-py, Heading-Mapping, Tabellen)
  templates.py    Template-Suche, Jinja-Umgebung, Filter
  engine.py       Engine-Erkennung, Kompilieren, Fehlerauszug
  config.py       texify.toml
  pdftools.py     Zusammenführen, Seiten, Info
  importers.py    PDF/DOCX/TXT -> Markdown
  ai.py           KI: Templates entwerfen, Text strukturieren (mit Test-Build-Schleife)
  mcp_server.py   MCP-Server
  guide.md        Template-Leitfaden (für Menschen und KI)
  templates/      eingebaute Templates + _shared/texify.sty
tests/            pytest (Kompilier-Tests laufen, sobald eine Engine verfügbar ist)
scripts/third_party.py   erzeugt THIRD_PARTY.md aus pyproject, Templates und Dockerfile
```

## Entwicklung

```bash
pip install -e ".[dev]"
pytest
python scripts/third_party.py          # Liste der Open-Source-Komponenten aktualisieren
python scripts/third_party.py --check  # in CI: schlägt fehl, wenn veraltet
```

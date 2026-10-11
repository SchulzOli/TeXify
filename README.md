# TeXify

**Markdown/Text → LaTeX → PDF.** You write content in plain, readable
Markdown; TeXify typesets it with LaTeX templates into professional documents –
CVs, cover letters, invoices, quotes, contracts, meeting minutes, invitations,
certificates and more. It also organises PDFs (merge, select pages, build
application bundles) and ships **one MCP server that works with every AI
client**.

<p align="center">
  <a href="examples/"><img src="examples/previews/11-cv-classic.png" width="190"></a>
  <a href="examples/"><img src="examples/previews/08-invoice.png" width="190"></a>
  <a href="examples/"><img src="examples/previews/19-invitation.png" width="190"></a>
  <a href="examples/"><img src="examples/previews/12-cv-modern.png" width="190"></a>
</p>

**→ [21 examples with Markdown source and PDF](examples/)**

| Layer | Who is responsible? |
|---|---|
| **Content** | you, in Markdown + YAML frontmatter (no LaTeX knowledge needed) |
| **Design & format** | LaTeX templates + the shared design package `texify.sty` |
| **Calculating, validating, building, merging** | deterministic code |
| **AI** | only where code cannot help: designing new templates and turning unstructured text into a document – through the MCP server (any AI client) or any LLM command line tool |

## Installation

```bash
pip install -e ".[all]"
```

TeXify needs a LaTeX engine. `texify doctor` shows what it finds. Without a
local TeX installation it builds through the Docker image automatically:

```bash
docker compose build
```

Supported engines, in order of preference: `tectonic`, `latexmk`, `pdflatex`,
`xelatex`, `lualatex`, `docker`. Force one with `-e`, the environment variable
`TEXIFY_ENGINE` or in `texify.toml`.

If `texify` is not on your `PATH` after installation, every command also works
as `python -m texify …`.

## Quick start

```bash
texify init                       # texify.toml: sender, bank, colour … enter once
texify new letter letter.md       # a template's example as a starting point
texify build letter.md            # -> letter.pdf
texify build letter.md --watch --open   # rebuild live on every save
```

## Writing documents

```markdown
---
template: invoice
recipient: |
  Example Ltd.
  5 High Street
  10115 Berlin
number: 2026-017
items:
  - {desc: Consulting, qty: 16, unit: h, price: 110}
  - {desc: "Implementation of **CI/CD**", qty: 1, unit: flat, price: 1850}
---

Thank you for your order. For the services provided I am charging:
```

- **Frontmatter** (between `---`) for structured data, **Markdown** for running
  text, lists, tables, images, quotes and links.
- Values from `texify.toml` under `[defaults]` apply to every document in the
  folder, e.g. `[defaults.sender]` for sender and bank. The frontmatter
  overrides them.
- `lang: en|de`, `font: sans|serif` and `accent: 1F4E79` control the design of
  every template. English is the default; German is fully supported
  (dates, numbers, currency, labels, DIN 5008 letters).
- Raw LaTeX goes into a code block with the info string `{=latex}`.
- Times (`10:00`) and postcodes (`01234`) stay text – YAML is read without the
  YAML 1.1 number conversions.
- `.txt` (read as Markdown) and `.yaml` (data only) work as well.

## Templates

| Name | For | Markdown convention |
|---|---|---|
| `article` | report, how-to, specification, press release | `# Title`, `## Sections`; `toc`, `numbered` |
| `letter` | DIN 5008 letter: cover letter, cancellation, reminder | running text; `recipient`, `subject` |
| `invoice` | invoice (German §14 UStG, optional §19) and quote (`kind: quote`) | `items` in YAML, totals are calculated |
| `cv` | classic tabular CV | `# Section`, `## Period \| Position, Company` |
| `cv-cards` | modern two-column card CV | same Markdown as `cv` |
| `coverpage` | cover page for job applications | `position`, `company`, `contents` |
| `proposal` | project / grant proposal | `facts`, `budget` (total calculated) |
| `confirmation` | confirmation, reference, power of attorney | `person`, `intro`, `signer` |
| `minutes` | meeting minutes, agenda | `# item` headings, `actions` |
| `invitation` | invitation to a party or event | `date`, `place`, `program`, `rsvp` |
| `certificate` | certificate, award (landscape) | `person`, `signers` |
| `contract` | contract with numbered clauses | `parties`; `# Clause`, paragraphs as `1.` list |

```bash
texify templates                  # list
texify templates show invoice     # fields + conventions
texify templates eject letter     # copy to ./templates/letter to adapt it
texify templates guide            # guide for writing your own templates
texify tex letter.md              # LaTeX only (complete project folder)
```

**LaTeX is the design tool.** Your own or adapted templates live in
`./templates/<name>/` (project) or `~/.texify/templates/<name>/` (global) and
take precedence over the built-in ones. The shared design (fonts, colours,
tables, lists) lives in
[texify/templates/_shared/texify.sty](texify/templates/_shared/texify.sty);
the authoring rules are in [texify/guide.md](texify/guide.md).

## Organising PDFs

```bash
# application bundle: build Markdown files and append existing PDFs, with bookmarks
texify build cover.md letter.md cv.md references/*.pdf -o application.pdf --bookmarks

texify merge scans/ -o everything.pdf --sort name --bookmarks
texify pages contract.pdf 1-3,7 -o excerpt.pdf
texify info *.pdf
texify import old_cv.pdf               # PDF/DOCX/TXT -> Markdown
```

## MCP server – one server for every AI client

TeXify ships a single [Model Context Protocol](https://modelcontextprotocol.io)
server. It is not tied to any AI vendor: Claude, ChatGPT, GitHub Copilot in VS
Code, Cursor, Windsurf, Zed, Cline, Codex, Hermes and every other MCP client
use the same server. The server never calls a model itself – the client's model
does the thinking, TeXify does the deterministic work.

| | |
|---|---|
| **Tools** | `list_templates`, `describe_template`, `template_guide`, `create_template` (saves only after a successful test build), `write_document`, `read_file`, `list_files`, `build_document`, `render_latex`, `merge_pdfs`, `import_document`, `pdf_info` |
| **Prompts** | `create_document` (notes → finished PDF), `design_template` (new document type), `application_bundle` (cover page + letter + CV) |
| **Resources** | `texify://guide`, `texify://templates/{name}` |

All file access is restricted to the workspace folder.

### Setup

**1. Pick how the server runs**

| Mode | When | Start |
|---|---|---|
| **Shared HTTP server** (recommended) | one running server for all your clients, TeX Live included | `docker compose up -d mcp` → `http://localhost:8747/mcp` |
| HTTP without Docker | local TeX/Tectonic installed | `texify mcp --http --workspace ~/Documents/texify` |
| stdio | the client starts its own server process | nothing to start – the client runs `texify mcp` |

The Docker service stores documents in `./workspace` (set `TEXIFY_WORKSPACE`
to use another folder):

```bash
TEXIFY_WORKSPACE=~/Documents/texify docker compose up -d mcp
```

**2. Generate the client configuration**

```bash
texify mcp config --http                  # connect to the shared HTTP server
texify mcp config --workspace ~/Documents/texify   # stdio, local Python
texify mcp config --docker --workspace ~/Documents/texify   # stdio inside Docker
```

Add `--format vscode` (VS Code `.vscode/mcp.json`), `--format toml` (e.g.
Codex `config.toml`) or `--format yaml` (e.g. Hermes `config.yaml`). The
default is the widely used `mcpServers` JSON:

```json
{
  "mcpServers": {
    "texify": { "type": "http", "url": "http://localhost:8747/mcp" }
  }
}
```

**3. Paste it into your client's MCP settings** – e.g. the `mcpServers` file
of your desktop app or IDE, `.vscode/mcp.json`, or the client's "add MCP
server" dialog (URL `http://localhost:8747/mcp`). Restart the client; it now
lists the TeXify tools and prompts.

**4. Try it:** *“Write my CV with the cv template from these notes and build
the PDF”* or use the prompt `create_document`.

> **Network access.** The HTTP server has no authentication and only accepts
> local connections (`127.0.0.1`). To share it in a trusted network, publish the
> port accordingly and add the host name with `--allow-host myserver:8747`, or put
> it behind a reverse proxy with authentication. Clients that only accept public
> HTTPS URLs need such a proxy or a tunnel.

## AI on the command line

Without an AI client, the same two AI tasks are available on the command line
with **any** LLM tool that reads a prompt on stdin – set it once in
`texify.toml`:

```toml
[ai]
command = "llm -m gpt-4o"        # or "ollama run llama3.1", "claude -p", …
```

```bash
texify ai template donation-receipt "Donation receipt for a registered non-profit"
texify ai fill cv old_cv.pdf      # PDF/text -> document in the cv format
```

Every AI result is checked deterministically (render and compile); errors go
back to the model automatically for correction (up to 3 attempts by default).

## Project layout

```
texify/
  cli.py          command line
  build.py        pipeline document -> LaTeX -> PDF
  markdown.py     Markdown -> LaTeX (markdown-it-py, heading map, tables)
  templates.py    template discovery, Jinja environment, filters
  engine.py       engine detection, compiling, error excerpt
  config.py       texify.toml
  pdftools.py     merge, pages, info
  importers.py    PDF/DOCX/TXT -> Markdown
  ai.py           AI tasks with test-build loop (any LLM command)
  mcp_server.py   MCP server (stdio + Streamable HTTP)
  guide.md        template authoring guide (for people and AI)
  templates/      built-in templates + _shared/texify.sty
examples/         21 example documents with PDFs and previews
tests/            pytest (compile tests run when an engine is available)
scripts/          build_examples.py, third_party.py (-> THIRD_PARTY.md)
```

## Development

```bash
pip install -e ".[dev]"
pytest
python scripts/build_examples.py       # rebuild examples, previews and gallery
python scripts/third_party.py          # update the list of open-source components
python scripts/third_party.py --check  # in CI: fails when outdated
```

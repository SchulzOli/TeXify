# TeXify — Setup Guide

A small Dockerized **LaTeX MCP server** that lets an LLM/agent:

- convert **PDF / DOCX / TXT / MD** documents into **LaTeX** (`.tex`)
- **create / read / edit** LaTeX documents (always stored as `.tex`)
- **compile** LaTeX back into **PDF** (pdflatex, multi-pass)

Everything happens inside one container; all documents live in a shared
`./data` folder that is bind-mounted to the host, so you can drop source
files in and pull generated PDFs out directly.

---

## 1. Layout

```
TeXify/
├── Dockerfile              # TeX Live + Python MCP server image
├── docker-compose.yml      # one service: texify
├── server.py               # the MCP server (all tools)
├── data/                   # <== workspace: put PDFs/DOCX here, get .tex/.pdf out
└── SETUP.md                # this file
```

## 2. Prerequisites

- Docker Desktop (Windows) — already present here
- `mcp` Python package installed for Hermes' MCP client (for the LLM side):
  `pip install mcp`

## 3. Build & start

From the project directory (`C:\Users\SOliv\Desktop\TeXify`):

```bash
docker compose build
docker compose up -d
```

This starts the `texify` container, which runs **two things**:

- **Web UI** on `http://localhost:8000` — upload, convert, edit, compile, preview
- **MCP server** (stdio, on demand) — the LLM interface (see §4)

## 3a. Web UI

Open **http://localhost:8000**:

- **Sidebar** — all documents in `data/` grouped by type. Drop a PDF/DOCX onto
  the dropzone (or click it) to upload; it auto-converts to a `.tex`.
- **LaTeX Source (left)** — CodeMirror editor with syntax highlighting. Select
  any `.tex` to load it; edit freely.
- **Preview (right)** — renders the compiled **PDF** (pdf.js) or, for a source
  file, the extracted text / a **DOCX** visual preview (mammoth). Toggle with
  the **PDF / Source** segmented control.
- **Save** — write the editor content back to the `.tex`.
- **Compile & Preview** — save + run pdflatex (2 passes) and show the result.
  Compile errors surface the pdflatex log tail in the preview pane.

Files persist on the host in `data/` — inputs and generated `.tex`/`.pdf` alike.

## 4. Wire it into Hermes (LLM interface)

Add the server to `~/.hermes/config.yaml` under `mcp_servers`:

```yaml
mcp_servers:
  texify:
    command: "docker"
    args: ["exec", "-i", "texify", "/app/entrypoint.sh", "mcp"]
    timeout: 300
    connect_timeout: 120
```

> The running `texify` container hosts the web UI; `docker exec ... mcp`
> spawns the MCP stdio server process inside it on demand (shares `/data`).
> Requires the container to be up (`docker compose up -d`).

Restart Hermes. On startup it connects, discovers the tools, and registers
them as:

```
mcp_texify_convert_to_latex
mcp_texify_create_latex
mcp_texify_edit_latex
mcp_texify_compile_pdf
mcp_texify_list_documents
mcp_texify_read_document
mcp_texify_pdf_info
mcp_texify_delete_document
mcp_texify_workspace_path
```

You can now just ask the agent in natural language, e.g.:

- *"Convert my cv.pdf to LaTeX"*
- *"Create a cover letter in LaTeX and compile it"*
- *"Change the accent color of my CV to teal and rebuild the PDF"*

## 5. Typical workflow (any document)

1. **Drop** the source file into `data/` (e.g. `data/report.docx`).
2. **Convert** → `convert_to_latex("report.docx")` → `report.tex`
   (text extracted, headings/bullets preserved, wrapped in a clean article skeleton).
3. **Refine** → `read_document` / `edit_latex` until the LaTeX looks right.
   For rich designs, replace the skeleton with a custom template (see `data/cv_oliver_schulz.tex`).
4. **Compile** → `compile_pdf("report.tex")` → `report.pdf`.
5. The finished PDF is immediately on the host at `data/report.pdf`.

## 6. Notes & pitfalls

- **Scanned PDFs** (no text layer) can't be text-extracted — run OCR first
  (the container has no OCR by design, to keep the image lean).
- Compile errors are returned with the tail of the pdflatex log so the LLM
  can fix the source with `edit_latex` and recompile.
- `edit_latex` uses unique find-and-replace — always include enough context.
- All `.tex`, `.pdf`, `.log` files persist on the host via the `./data` mount.

## 7. Example: the CV

`data/cv_oliver_schulz.tex` is a full custom template (two-column tcolorbox
layout) generated from `cv_komprimiert.pdf`, compiled to
`data/cv_oliver_schulz.pdf`. Use it as a reference for building other styled
documents.

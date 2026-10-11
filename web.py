"""
TeXify — Web UI backend (FastAPI).

Serves a single-page app for:
  * uploading PDF/DOCX/TXT/MD documents
  * converting them to LaTeX
  * editing the LaTeX source (code view)
  * compiling to PDF and previewing the result

Runs on :8000 inside the texify container, sharing /data with the MCP server.
"""

from __future__ import annotations

import json
import mimetypes
from pathlib import Path

from fastapi import FastAPI, File, UploadFile
from fastapi.responses import FileResponse, JSONResponse
from fastapi.staticfiles import StaticFiles

import core

app = FastAPI(title="TeXify UI")

STATIC_DIR = Path(__file__).resolve().parent / "static"


# --------------------------------------------------------------------------- #
# API
# --------------------------------------------------------------------------- #


@app.get("/api/health")
def health() -> dict:
    return {"status": "ok", "workspace": str(core.WORKSPACE)}


@app.get("/api/documents")
def documents() -> dict:
    return core.list_documents()


@app.post("/api/upload")
async def upload(file: UploadFile = File(...)):
    data = await file.read()
    res = core.save_upload(file.filename or "upload", data)
    code = 200 if "error" not in res else 400
    return JSONResponse(res, status_code=code)


@app.post("/api/convert")
def convert(payload: dict):
    source = payload.get("source", "")
    res = core.convert_to_latex(source)
    code = 200 if "error" not in res else 400
    return JSONResponse(res, status_code=code)


@app.get("/api/read")
def read(name: str):
    p = core.safe_name(name)
    if not p.exists():
        return JSONResponse({"error": f"not found: {name}"}, status_code=404)
    if p.suffix.lower() in {".pdf", ".docx"}:
        return JSONResponse({"error": "binary file; use /api/file to fetch it"}, status_code=400)
    return {"name": p.name, "content": p.read_text(encoding="utf-8", errors="replace")}


@app.post("/api/save")
def save(payload: dict):
    name = payload.get("name", "")
    content = payload.get("content", "")
    res = core.create_latex(name, content, overwrite=True)
    code = 200 if "error" not in res else 400
    return JSONResponse(res, status_code=code)


@app.post("/api/compile")
def compile_doc(payload: dict):
    name = payload.get("name", "")
    passes = int(payload.get("passes", 2))
    res = core.compile_pdf(name, passes)
    code = 200 if res.get("success") else 400
    return JSONResponse(res, status_code=code)


@app.get("/api/file")
def get_file(name: str):
    """Serve a workspace file (PDF preview, DOCX download, etc.)."""
    p = core.safe_name(name)
    if not p.exists():
        return JSONResponse({"error": f"not found: {name}"}, status_code=404)
    media, _ = mimetypes.guess_type(p.name)
    if p.suffix.lower() == ".pdf":
        media = "application/pdf"
    elif p.suffix.lower() == ".docx":
        media = "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
    return FileResponse(str(p), media_type=media or "application/octet-stream", filename=p.name)


@app.get("/api/preview_source")
def preview_source(name: str):
    """Extract text from a PDF/DOCX/TXT/MD for the source preview pane."""
    p = core.safe_name(name)
    if not p.exists():
        return JSONResponse({"error": f"not found: {name}"}, status_code=404)
    ext = p.suffix.lower()
    try:
        if ext == ".pdf":
            text = core.extract_pdf_text(p)
        elif ext == ".docx":
            text = core.extract_docx_text(p)
        elif ext in {".txt", ".md"}:
            text = p.read_text(encoding="utf-8", errors="replace")
        else:
            return JSONResponse({"error": f"no text preview for {ext}"}, status_code=400)
    except RuntimeError as exc:
        return JSONResponse({"error": str(exc)}, status_code=400)
    return {"name": p.name, "text": text}


# --------------------------------------------------------------------------- #
# Static SPA
# --------------------------------------------------------------------------- #

app.mount("/", StaticFiles(directory=str(STATIC_DIR), html=True), name="static")

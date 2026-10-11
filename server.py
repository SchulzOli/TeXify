"""
TeXify — MCP server (stdio).

Thin MCP wrapper over core.py so an LLM/agent can:
  * convert PDF / DOCX / TXT / MD documents to LaTeX source
  * create, read, edit LaTeX documents (always stored as .tex)
  * compile LaTeX to PDF (pdflatex, multi-pass)
  * list / manage documents

All files live under WORKSPACE_DIR (default /data), bind-mounted to the host.
"""

from __future__ import annotations

import json

from mcp.server.fastmcp import FastMCP

import core

mcp = FastMCP("texify")


@mcp.tool()
def workspace_path() -> str:
    """Return the workspace path info (for orientation)."""
    return json.dumps(
        {
            "workspace_dir_in_container": str(core.WORKSPACE),
            "note": "Bind-mounted to the host project ./data folder.",
        },
        indent=2,
    )


@mcp.tool()
def list_documents() -> str:
    """List all documents in the workspace grouped by type."""
    return json.dumps(core.list_documents(), indent=2)


@mcp.tool()
def read_document(name: str) -> str:
    """Read a text document from the workspace (.tex, .txt, .md, .log).

    For .pdf/.docx use convert_to_latex instead.
    """
    p = core.safe_name(name)
    if not p.exists():
        return json.dumps({"error": f"not found: {name}"})
    if p.suffix.lower() in {".pdf", ".docx"}:
        return json.dumps(
            {"error": f"{name} is binary; use convert_to_latex to get LaTeX source."}
        )
    return p.read_text(encoding="utf-8", errors="replace")


@mcp.tool()
def convert_to_latex(source: str, output: str | None = None) -> str:
    """Convert a PDF, DOCX, TXT or MD file in the workspace into a .tex document."""
    return json.dumps(core.convert_to_latex(source, output), indent=2)


@mcp.tool()
def create_latex(name: str, content: str, overwrite: bool = False) -> str:
    """Create a new LaTeX document in the workspace."""
    return json.dumps(core.create_latex(name, content, overwrite), indent=2)


@mcp.tool()
def edit_latex(name: str, old_string: str, new_string: str, replace_all: bool = False) -> str:
    """Targeted find-and-replace edit of a .tex file in the workspace."""
    return json.dumps(core.edit_latex(name, old_string, new_string, replace_all), indent=2)


@mcp.tool()
def compile_pdf(name: str, passes: int = 2) -> str:
    """Compile a .tex document to PDF with pdflatex (multi-pass)."""
    return json.dumps(core.compile_pdf(name, passes), indent=2)


@mcp.tool()
def pdf_info(name: str) -> str:
    """Get page count and metadata for a PDF in the workspace (pdfinfo)."""
    p = core.safe_name(name)
    if not p.exists():
        return json.dumps({"error": f"not found: {name}"})
    r = core.run(["pdfinfo", str(p)], cwd=core.WORKSPACE, timeout=30)
    return r.stdout if r.returncode == 0 else json.dumps({"error": r.stderr})


@mcp.tool()
def delete_document(name: str) -> str:
    """Delete a document from the workspace (and its build artifacts)."""
    return json.dumps(core.delete_document(name), indent=2)


if __name__ == "__main__":
    mcp.run()

"""Smoke test: exercise every MCP tool against the running latex-mcp container.

Runs INSIDE the container via `docker exec` so we talk to the real server code
with the real TeX Live toolchain, without needing an MCP client.
"""
import json
import subprocess
import sys

C = "texify"


def call(tool_src: str) -> dict:
    """Run a python snippet inside the container that imports server and calls a tool."""
    cmd = ["docker", "exec", C, "python", "-c", tool_src]
    r = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
    out = (r.stdout or "").strip()
    try:
        return json.loads(out)
    except Exception:
        return {"_raw": out, "_stderr": r.stderr.strip()[-800:], "_rc": r.returncode}


def run(label, tool_src):
    print(f"\n=== {label} ===")
    res = call(tool_src)
    s = json.dumps(res, indent=2)[:700]
    print(s)
    return res


pre = "import sys, json; sys.path.insert(0,'/app'); import server; "

run("list_documents", pre + "print(server.list_documents())")
run("convert_to_latex (scanned pdf -> expect OCR error)",
    pre + "print(server.convert_to_latex('cv_komprimiert.pdf'))")
run("compile_pdf cv_oliver_schulz",
    pre + "print(server.compile_pdf('cv_oliver_schulz.tex', passes=2))")
run("pdf_info",
    pre + "print(server.pdf_info('cv_oliver_schulz.pdf'))")
run("edit_latex (accent color tweak)",
    pre + "print(server.edit_latex('cv_oliver_schulz.tex', 'definecolor{headgray}{RGB}{120,120,120}', 'definecolor{headgray}{RGB}{120,120,120}'))")
run("create_latex (test doc)",
    pre + "print(server.create_latex('smoke_test', '\\\\documentclass{article}\\\\begin{document}Hello MCP.\\\\end{document}', overwrite=True))")
run("compile smoke_test",
    pre + "print(server.compile_pdf('smoke_test.tex', passes=1))")
run("delete_document smoke_test",
    pre + "print(server.delete_document('smoke_test.tex'))")

print("\nSMOKE TEST DONE")

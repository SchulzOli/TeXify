import asyncio
import json
import sys
from pathlib import Path

import pytest

from texify import TexifyError
from texify.ai import ai_command, complete, extract_blocks
from texify.cli import mcp_client_config
from texify.config import load_config


def test_mcp_config_formats(tmp_path):
    ws = tmp_path
    http = json.loads(mcp_client_config("http", "json", ws, "http://localhost:8747/mcp", "texify:latest"))
    assert http == {"mcpServers": {"texify": {"type": "http", "url": "http://localhost:8747/mcp"}}}

    stdio = json.loads(mcp_client_config("stdio", "vscode", ws, "", "texify:latest"))
    entry = stdio["servers"]["texify"]
    assert entry["type"] == "stdio" and entry["args"][:3] == ["-m", "texify", "mcp"]

    docker = mcp_client_config("docker", "toml", ws, "", "texify:latest")
    assert docker.startswith("[mcp_servers.texify]") and '"docker"' in docker


def test_ai_command_is_optional_and_provider_agnostic(tmp_path, monkeypatch):
    monkeypatch.delenv("TEXIFY_AI_COMMAND", raising=False)
    monkeypatch.setenv("TEXIFY_HOME", str(tmp_path / "home"))
    monkeypatch.chdir(tmp_path)
    with pytest.raises(TexifyError, match="MCP"):
        ai_command(load_config(tmp_path))

    # any stdin -> stdout program works as a "model"
    script = tmp_path / "echo_model.py"
    script.write_text("import sys; data = sys.stdin.read(); print('<document>' + str(len(data)) + '</document>')")
    monkeypatch.setenv("TEXIFY_AI_COMMAND", f'"{sys.executable}" "{script}"')
    answer = complete("system", [{"role": "user", "content": "hello"}], load_config(tmp_path))
    assert int(extract_blocks(answer, ("document",))["document"]) > 0


def test_mcp_server_lists_tools_prompts_resources():
    from texify import mcp_server

    async def inspect():
        tools = [t.name for t in await mcp_server.mcp.list_tools()]
        prompts = [p.name for p in await mcp_server.mcp.list_prompts()]
        templates = [t.uriTemplate for t in await mcp_server.mcp.list_resource_templates()]
        return tools, prompts, templates

    tools, prompts, templates = asyncio.run(inspect())
    assert {"build_document", "create_template", "merge_pdfs"} <= set(tools)
    assert {"create_document", "design_template", "application_bundle"} <= set(prompts)
    assert "texify://templates/{name}" in templates


def test_mcp_paths_stay_in_workspace(tmp_path):
    from texify import mcp_server

    mcp_server._ROOT = Path(tmp_path).resolve()
    assert mcp_server.read_file("../outside.md")["ok"] is False

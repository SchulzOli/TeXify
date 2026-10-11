#!/bin/sh
# TeXify container entrypoint:
#   - web UI (FastAPI/uvicorn) on :8000  -> docker compose service
#   - MCP server (stdio) on demand       -> `docker exec texify mcp`
set -e

if [ "$1" = "mcp" ]; then
  exec python /app/server.py
fi

# default: run the web UI
exec uvicorn web:app --host 0.0.0.0 --port 8000 --app-dir /app

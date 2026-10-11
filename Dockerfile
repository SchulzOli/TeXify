FROM python:3.12-slim

ENV DEBIAN_FRONTEND=noninteractive \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1

# TeX Live (recommended+extra cover tcolorbox, tikz, geometry, titlesec,
# enumitem, parskip, xcolor, hyperref, helvet, ...) + poppler for pdftotext.
# texlive-fonts-extra intentionally omitted (624MB, not needed).
# The deb.debian.org mirror is flaky -> run apt-get in a retry loop (apt resumes).
RUN set -e; \
    for i in 1 2 3 4 5 6 7 8; do \
      apt-get update && \
      apt-get install -y --no-install-recommends --fix-missing \
        -o Acquire::Retries=10 -o Acquire::http::Timeout=120 \
        texlive-latex-base \
        texlive-latex-recommended \
        texlive-latex-extra \
        texlive-fonts-recommended \
        latexmk \
        poppler-utils \
      && break; \
      echo "apt attempt $i failed, retrying..."; sleep 5; \
    done; \
    apt-get clean && rm -rf /var/lib/apt/lists/*

RUN pip install --no-cache-dir "mcp[cli]>=1.10,<2" python-docx \
      "fastapi>=0.110" "uvicorn[standard]>=0.29" "python-multipart>=0.0.9" uv || \
    pip install --no-cache-dir --retries 10 "mcp[cli]>=1.10,<2" python-docx \
      "fastapi>=0.110" "uvicorn[standard]>=0.29" "python-multipart>=0.0.9"

WORKDIR /app

COPY core.py /app/core.py
COPY server.py /app/server.py
COPY web.py /app/web.py
COPY static/ /app/static/
COPY entrypoint.sh /app/entrypoint.sh
RUN chmod +x /app/entrypoint.sh

# Documents live here (bind-mounted from the host)
RUN mkdir -p /data
VOLUME ["/data"]

EXPOSE 8000

ENTRYPOINT ["/app/entrypoint.sh"]

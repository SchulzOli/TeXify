# TeXify: TeX Live + texify CLI/MCP in one image.
#
#   docker compose build
#   docker compose run --rm texify build dokumente/brief.md
#   docker run -i --rm -v "$PWD:/work" texify:latest mcp      # MCP over stdio
#
# The local CLI also uses this image as LaTeX engine when no TeX is installed
# (engine "docker": runs latexmk inside the container).
FROM python:3.12-slim

ENV DEBIAN_FRONTEND=noninteractive \
    PYTHONUNBUFFERED=1 \
    PIP_NO_CACHE_DIR=1

# TeX Live: latex-extra covers tcolorbox, paracol, titlesec, ...;
# fonts-recommended brings TeX Gyre; lang-german for babel ngerman.
# The deb.debian.org mirror is flaky -> apt-get in a retry loop.
RUN set -e; \
    for i in 1 2 3 4 5 6 7 8; do \
      apt-get update && \
      apt-get install -y --no-install-recommends --fix-missing \
        -o Acquire::Retries=10 -o Acquire::http::Timeout=120 \
        texlive-latex-base \
        texlive-latex-recommended \
        texlive-latex-extra \
        texlive-fonts-recommended \
        tex-gyre \
        texlive-plain-generic \
        texlive-lang-german \
        texlive-lang-english \
        lmodern \
        latexmk \
      && break; \
      echo "apt attempt $i failed, retrying..."; sleep 5; \
    done; \
    apt-get clean && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY pyproject.toml README.md /app/
COPY texify /app/texify
RUN pip install --no-cache-dir --retries 10 "/app[mcp,docx]"

ENV TEXIFY_ENGINE=latexmk
WORKDIR /work
ENTRYPOINT ["texify"]
CMD ["--help"]

# TeXify: TeX Live + the texify binary in one image.
#
#   docker compose build
#   docker compose up -d mcp                                   # shared MCP server
#   docker compose run --rm cli build examples/08-invoice.md   # command line
#
# The local CLI also uses this image as LaTeX engine when no TeX is installed
# (engine "docker": runs latexmk inside the container).

# ---------------------------------------------------------------- build
FROM rust:1-trixie AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY xtask/Cargo.toml xtask/Cargo.toml
COPY src src
COPY assets assets
COPY xtask/src xtask/src
RUN cargo build --release --locked --bin texify

# ---------------------------------------------------------------- runtime
FROM debian:trixie-slim

ENV DEBIAN_FRONTEND=noninteractive

# TeX Live: latex-extra covers tcolorbox, paracol, pgfornament, ...;
# tex-gyre = template fonts; lang-german for babel ngerman;
# poppler-utils renders preview images (cargo xtask examples).
# The Debian mirror can be flaky -> apt-get in a retry loop.
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
        poppler-utils \
        ca-certificates \
      && break; \
      echo "apt attempt $i failed, retrying..."; sleep 5; \
    done; \
    apt-get clean && rm -rf /var/lib/apt/lists/*

COPY --from=build /src/target/release/texify /usr/local/bin/texify

ENV TEXIFY_ENGINE=latexmk
WORKDIR /work
ENTRYPOINT ["texify"]
CMD ["--help"]

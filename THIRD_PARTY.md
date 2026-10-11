# Third-Party-Komponenten

Automatisch erzeugt von `scripts/third_party.py` – nicht von Hand bearbeiten.
Quellen: `pyproject.toml`, alle Templates (`\usepackage`), `Dockerfile`.

## Python

| Paket | Gruppe | Lizenz | Projekt |
|---|---|---|---|
| markdown-it-py | core | MIT License | https://pypi.org/project/markdown-it-py/ |
| Jinja2 | core | BSD License | https://pypi.org/project/Jinja2/ |
| PyYAML | core | MIT | https://pypi.org/project/PyYAML/ |
| pypdf | core | BSD-3-Clause | https://pypi.org/project/pypdf/ |
| mcp | mcp | MIT | https://pypi.org/project/mcp/ |
| anthropic | ai | MIT | https://pypi.org/project/anthropic/ |
| python-docx | docx | MIT | https://pypi.org/project/python-docx/ |
| pytest | dev | MIT | https://pypi.org/project/pytest/ |
| pypdfium2 | dev | BSD-3-Clause, Apache-2.0, dependency lic | https://pypi.org/project/pypdfium2/ |

## LaTeX-Pakete (TeX Live / CTAN)

| Paket | verwendet in |
|---|---|
| [amssymb](https://ctan.org/pkg/amssymb) | _shared |
| [array](https://ctan.org/pkg/array) | _shared |
| [babel](https://ctan.org/pkg/babel) | _shared |
| [bookmark](https://ctan.org/pkg/bookmark) | _shared |
| [booktabs](https://ctan.org/pkg/booktabs) | _shared |
| [enumitem](https://ctan.org/pkg/enumitem) | _shared |
| [fancyvrb](https://ctan.org/pkg/fancyvrb) | _shared |
| [fontenc](https://ctan.org/pkg/fontenc) | _shared |
| [fontspec](https://ctan.org/pkg/fontspec) | _shared |
| [geometry](https://ctan.org/pkg/geometry) | _shared, article, certificate, confirmation, contract, coverpage, cv, cv-cards, invitation, minutes, proposal |
| [graphicx](https://ctan.org/pkg/graphicx) | _shared |
| [hyperref](https://ctan.org/pkg/hyperref) | _shared |
| [iftex](https://ctan.org/pkg/iftex) | _shared |
| [inputenc](https://ctan.org/pkg/inputenc) | _shared |
| [lmodern](https://ctan.org/pkg/lmodern) | _shared |
| [microtype](https://ctan.org/pkg/microtype) | _shared |
| [paracol](https://ctan.org/pkg/paracol) | cv-cards |
| [pgfornament](https://ctan.org/pkg/pgfornament) | certificate |
| [scrartcl](https://ctan.org/pkg/scrartcl) | article, certificate, confirmation, contract, coverpage, cv, cv-cards, invitation, minutes, proposal |
| [scrlayer-scrpage](https://ctan.org/pkg/scrlayer-scrpage) | article, contract, minutes, proposal |
| [scrlttr2](https://ctan.org/pkg/scrlttr2) | invoice, letter |
| [tabularx](https://ctan.org/pkg/tabularx) | _shared |
| [tcolorbox](https://ctan.org/pkg/tcolorbox) | cv-cards, invitation, minutes, proposal |
| [textcomp](https://ctan.org/pkg/textcomp) | _shared |
| [tgheros](https://ctan.org/pkg/tgheros) | _shared |
| [tgpagella](https://ctan.org/pkg/tgpagella) | _shared |
| [tikz](https://ctan.org/pkg/tikz) | certificate, coverpage, invitation |
| [ulem](https://ctan.org/pkg/ulem) | _shared |
| [xcolor](https://ctan.org/pkg/xcolor) | _shared |

## System

- Docker-Basis-Image `python:3.12-slim`
- Debian-Paket `texlive-latex-base`
- Debian-Paket `texlive-latex-recommended`
- Debian-Paket `texlive-latex-extra`
- Debian-Paket `texlive-fonts-recommended`
- Debian-Paket `tex-gyre`
- Debian-Paket `texlive-plain-generic`
- Debian-Paket `texlive-lang-german`
- Debian-Paket `texlive-lang-english`
- Debian-Paket `lmodern`
- Debian-Paket `latexmk`
- LaTeX-Engines (optional, lokal): Tectonic, TeX Live, MiKTeX

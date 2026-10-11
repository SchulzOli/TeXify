"""TeXify — Markdown/Text -> LaTeX -> PDF.

LaTeX is the design layer: templates turn plain, readable Markdown (plus a
YAML frontmatter for structured data) into professional documents such as
CVs, cover letters, invoices, proposals and confirmations.
"""

__version__ = "0.2.0"


class TexifyError(Exception):
    """User-facing error (bad input, missing template, compile failure...)."""

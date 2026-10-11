from texify.latex import Tex, escape, format_money, format_number
from texify.markdown import LatexRenderer


def test_escape_specials():
    assert escape("50% & $5_#{x}~^") == r"50\% \& \$5\_\#\{x\}\textasciitilde{}\textasciicircum{}"
    assert escape('"a"') == r"\textquotedbl{}a\textquotedbl{}"
    assert escape(Tex(r"\textbf{x}")) == r"\textbf{x}"


def test_smart_quotes_by_language():
    assert LatexRenderer("de").render_inline('"Hallo"') == "„Hallo“"
    assert LatexRenderer("en").render_inline('"Hi"') == "“Hi”"


def test_inline_markup():
    out = LatexRenderer().render_inline("**fett** *kursiv* `a_b` [x](https://e.de/a#b) ~~weg~~")
    assert out == r"\textbf{fett} \emph{kursiv} \texttt{a\_b} \href{https://e.de/a\#b}{x} \sout{weg}"


def test_heading_levels_are_normalized():
    out = LatexRenderer().render("## A\n\n### B\n")
    assert "\\section{A}" in out and "\\subsection{B}" in out


def test_heading_map_with_split_and_pad():
    r = LatexRenderer(headings={1: "cvsection", 2: {"cmd": "cventry", "args": 2, "split": "|", "pad": "start"}})
    out = r.render("# Erfahrung\n\n## 2020 – 2022 | Master\n\n## Ohne Datum\n\n## a | b | c\n")
    assert "\\cvsection{Erfahrung}" in out
    assert "\\cventry{2020 – 2022}{Master}" in out
    assert "\\cventry{}{Ohne Datum}" in out
    assert "\\cventry{a}{b | c}" in out


def test_env_headings_wrap_content():
    r = LatexRenderer(headings={1: {"env": "box", "args": 1}})
    out = r.render("# One\ntext\n# Two\nmore\n")
    assert out.index("\\end{box}") < out.index("\\begin{box}{Two}")
    assert out.rstrip().endswith("\\end{box}")


def test_lists_tables_and_raw_latex():
    out = LatexRenderer().render(
        "- a\n- b\n\n3. x\n4. y\n\n| Name | Betrag |\n|---|--:|\n| A | 1 |\n\n```{=latex}\n\\newpage\n```\n")
    assert "\\begin{itemize}" in out and "\\begin{enumerate}[start=3]" in out
    assert "\\begin{tabularx}{\\linewidth}{@{}L r@{}}" in out
    assert "\\newpage" in out


def test_long_table_columns_are_weighted():
    md = "| Nr | Beschreibung | Notiz |\n|---|---|---|\n| 1 | " + "x" * 80 + " | " + "y" * 30 + " |\n"
    out = LatexRenderer().render(md)
    assert "{@{}l Y{" in out


def test_sections_and_title():
    r = LatexRenderer()
    title, rest = r.extract_title("# Titel\n\nText\n\n## Teil\nInhalt\n")
    assert title == "Titel"
    secs = r.sections(rest)
    assert [s["raw_title"] for s in secs] == ["", "Teil"]
    assert "Inhalt" in secs[1]["body"]


def test_yaml_keeps_times_and_postcodes_as_text():
    from texify.latex import load_yaml

    data = load_yaml("time: 10:00\nzip: 01234\nn: 42\nf: 1.5")
    assert data == {"time": "10:00", "zip": "01234", "n": 42, "f": 1.5}


def test_number_formatting():
    assert format_number("1234.5", "de") == "1.234,5"
    assert format_number(1234.5, "en", 2) == "1,234.50"
    assert format_money(4551.27, "de") == "4.551,27 €"

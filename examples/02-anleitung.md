---
template: article
subtitle: Von der Installation bis zum ersten PDF
author: TeXify-Team
date: today
numbered: true
toc: true
accent: 2E7D32
---

# Kurzanleitung TeXify

## Installation

TeXify ist ein Python-Paket. Die Installation dauert weniger als eine Minute:

```
pip install -e ".[all]"
texify doctor
```

Fehlt eine LaTeX-Engine, baut TeXify automatisch über Docker.

## Das erste Dokument

### Vorlage wählen

Alle Vorlagen zeigt `texify templates`. Ein Beispiel als Startpunkt legt
`texify new letter brief.md` an.

### Inhalt schreiben

Der Inhalt ist normales **Markdown**. Strukturierte Angaben wie Adressen oder
Rechnungspositionen stehen im Kopfbereich (*Frontmatter*) zwischen zwei
`---`-Zeilen.

### PDF erzeugen

```
texify build brief.md --open
```

## Tipps

| Wunsch                    | Lösung                                   |
|:--------------------------|:-----------------------------------------|
| Andere Farbe              | `accent: 2E7D32` im Frontmatter          |
| Serifenschrift            | `font: serif`                            |
| Englisch                  | `lang: en`                               |
| Absender nur einmal pflegen | `texify init` und `[defaults.sender]`  |
| Eigenes Design            | `texify templates eject letter`          |

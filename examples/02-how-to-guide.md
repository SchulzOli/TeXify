---
template: article
subtitle: From installation to your first PDF
author: The TeXify team
date: today
numbered: true
toc: true
accent: 2E7D32
---

# TeXify Quick Guide

## Installation

TeXify is a single binary. Download it from the releases page or install it
with Cargo – it takes less than a minute:

```
cargo install --git https://github.com/SchulzOli/TeXify texify
texify doctor
```

Without a local LaTeX engine TeXify builds through Docker automatically.

## Your first document

### Pick a template

`texify templates` lists every template. `texify new letter letter.md`
creates a starting point from its example.

### Write the content

The content is plain **Markdown**. Structured data such as addresses or
invoice items goes into the header (*frontmatter*) between two `---` lines.

### Create the PDF

```
texify build letter.md --open
```

## Tips

| You want …                     | Do this                                  |
|:-------------------------------|:-----------------------------------------|
| A different colour             | `accent: 2E7D32` in the frontmatter      |
| A serif font                   | `font: serif`                            |
| German output                  | `lang: de`                               |
| Your sender data only once     | `texify init` and `[defaults.sender]`    |
| Your own design                | `texify templates eject letter`          |

# dprint-plugin-markdown

[![](https://img.shields.io/crates/v/dprint-plugin-markdown.svg)](https://crates.io/crates/dprint-plugin-markdown) [![CI](https://github.com/dprint/dprint-plugin-markdown/workflows/CI/badge.svg)](https://github.com/dprint/dprint-plugin-markdown/actions?query=workflow%3ACI)

Markdown formatting plugin for dprint.

This fork tracks upstream [`dprint/dprint-plugin-markdown`](https://github.com/dprint/dprint-plugin-markdown) with Cole-specific customizations on the [`cole-customizations`](https://github.com/colelawrence/dprint-plugin-markdown/tree/cole-customizations) branch.

## Fork customizations

Compared to upstream, this fork currently adds:

- `tableFormat` configuration with `aligned` compatibility mode and opt-in `compact` table output for canonical, lower-noise Markdown tables.
- Heuristic YAML/TOML frontmatter routing so `---` and `+++` metadata blocks can be formatted by the matching host formatter when the language is detectable.
- `.mdx` file recognition through an MDX-aware preserve-first mode rather than treating MDX as plain Markdown.
- Safe format-or-preserve handling for top-level MDX ESM and React-like JSX islands via the host TSX formatter.
- Conservative raw preservation for unsupported or ambiguous MDX syntax, including MDX JSX with Markdown/prose children and inline MDX expressions.
- MDX ignore directive support for both HTML comments and MDX comments.
- Prettier-derived MDX fixture category coverage for ESM, invalid import-like prose, ignore comments, inline HTML/JSX, tables, and fenced JSX code.

MDX support is intentionally preserve-first: the formatter may format safe embedded islands, but ambiguous or unsupported MDX syntax is preserved instead of being treated as plain Markdown.

This uses the [pulldown-cmark](https://github.com/raphlinus/pulldown-cmark) parser for markdown.

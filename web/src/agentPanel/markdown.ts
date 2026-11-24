import { Lexer, type Token, type Tokens } from "marked"

export type MarkdownInline =
  | { readonly kind: "text", readonly text: string }
  | { readonly kind: "strong", readonly children: readonly MarkdownInline[] }
  | { readonly kind: "emphasis", readonly children: readonly MarkdownInline[] }
  | { readonly kind: "strikethrough", readonly children: readonly MarkdownInline[] }
  | { readonly kind: "code", readonly text: string }
  | { readonly kind: "link", readonly href: string, readonly children: readonly MarkdownInline[] }
  | { readonly kind: "line_break" }

export type TableAlignment = "left" | "center" | "right" | null

export type MarkdownBlock =
  | { readonly kind: "heading", readonly level: number, readonly children: readonly MarkdownInline[] }
  | { readonly kind: "paragraph", readonly children: readonly MarkdownInline[] }
  | { readonly kind: "code", readonly language: string, readonly text: string }
  | { readonly kind: "list", readonly ordered: boolean, readonly start: number, readonly items: readonly (readonly MarkdownBlock[])[] }
  | { readonly kind: "quote", readonly children: readonly MarkdownBlock[] }
  | {
    readonly kind: "table"
    readonly alignments: readonly TableAlignment[]
    readonly header: readonly (readonly MarkdownInline[])[]
    readonly rows: readonly (readonly (readonly MarkdownInline[])[])[]
  }
  | { readonly kind: "rule" }

const LINK_PROTOCOLS = new Set(["http:", "https:", "mailto:"])

// Only absolute addresses with a listed protocol become links, so a relative or script address stays text
export function safeLinkHref(href: string): string | undefined {
  if (!URL.canParse(href)) return undefined
  const url = new URL(href)
  return LINK_PROTOCOLS.has(url.protocol) ? url.href : undefined
}

function inlines(tokens: readonly Token[] | undefined): MarkdownInline[] {
  return (tokens ?? []).flatMap(inline)
}

function inline(token: Token): MarkdownInline[] {
  switch (token.type) {
    case "strong":
      return [{ kind: "strong", children: inlines(token.tokens) }]
    case "em":
      return [{ kind: "emphasis", children: inlines(token.tokens) }]
    case "del":
      return [{ kind: "strikethrough", children: inlines(token.tokens) }]
    case "codespan":
      return [{ kind: "code", text: token.text }]
    case "br":
      return [{ kind: "line_break" }]
    case "link": {
      const href = safeLinkHref(token.href)
      if (href === undefined) return [{ kind: "text", text: token.raw }]
      return [{ kind: "link", href, children: inlines(token.tokens) }]
    }
    case "text":
      return token.tokens ? inlines(token.tokens) : [{ kind: "text", text: token.text }]
    case "escape":
      return [{ kind: "text", text: token.text }]
    default:
      // Raw HTML, images, and each other token show as their source text
      return [{ kind: "text", text: token.raw }]
  }
}

const tableCells = (cells: readonly Tokens.TableCell[]) => cells.map((cell) => inlines(cell.tokens))

function blocks(tokens: readonly Token[]): MarkdownBlock[] {
  return tokens.flatMap(block)
}

function block(token: Token): MarkdownBlock[] {
  switch (token.type) {
    case "space":
    case "def":
      return []
    case "heading":
      return [{ kind: "heading", level: token.depth, children: inlines(token.tokens) }]
    case "paragraph":
    case "text":
      return [{ kind: "paragraph", children: token.tokens ? inlines(token.tokens) : [{ kind: "text", text: token.text }] }]
    case "code":
      return [{ kind: "code", language: token.lang ?? "", text: token.text }]
    case "list": {
      const list = token as Tokens.List
      return [{
        kind: "list",
        ordered: list.ordered,
        start: typeof list.start === "number" ? list.start : 1,
        items: list.items.map((item) => blocks(item.tokens)),
      }]
    }
    case "blockquote":
      return [{ kind: "quote", children: blocks(token.tokens ?? []) }]
    case "table": {
      const table = token as Tokens.Table
      return [{ kind: "table", alignments: table.align, header: tableCells(table.header), rows: table.rows.map(tableCells) }]
    }
    case "hr":
      return [{ kind: "rule" }]
    default:
      return [{ kind: "paragraph", children: [{ kind: "text", text: token.raw.trimEnd() }] }]
  }
}

export function parseMarkdown(text: string): MarkdownBlock[] {
  return blocks(Lexer.lex(text, { gfm: true }))
}

import type { Extension } from "@codemirror/state"

type EditorLanguage = { readonly name: string, readonly load: () => Promise<Extension> }

function javascriptDialect(name: string, dialect: { jsx?: boolean, typescript?: boolean }): EditorLanguage {
  return { name, load: async () => (await import("@codemirror/lang-javascript")).javascript(dialect) }
}

const typescript = javascriptDialect("TypeScript", { typescript: true })
const javascript = javascriptDialect("JavaScript", {})
const markdown: EditorLanguage = {
  name: "Markdown",
  load: async () => (await import("@codemirror/lang-markdown")).markdown(),
}
const html: EditorLanguage = { name: "HTML", load: async () => (await import("@codemirror/lang-html")).html() }

const languageByExtension: ReadonlyMap<string, EditorLanguage> = new Map(
  Object.entries({
    ts: typescript,
    mts: typescript,
    cts: typescript,
    tsx: javascriptDialect("TypeScript JSX", { typescript: true, jsx: true }),
    js: javascript,
    mjs: javascript,
    cjs: javascript,
    jsx: javascriptDialect("JavaScript JSX", { jsx: true }),
    rs: { name: "Rust", load: async () => (await import("@codemirror/lang-rust")).rust() },
    py: { name: "Python", load: async () => (await import("@codemirror/lang-python")).python() },
    json: { name: "JSON", load: async () => (await import("@codemirror/lang-json")).json() },
    md: markdown,
    markdown,
    html,
    htm: html,
    css: { name: "CSS", load: async () => (await import("@codemirror/lang-css")).css() },
  }),
)

export function languageOf(path: string): EditorLanguage | undefined {
  return languageByExtension.get(path.slice(path.lastIndexOf(".") + 1).toLowerCase())
}

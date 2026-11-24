import { HighlightStyle, syntaxHighlighting } from "@codemirror/language"
import type { Extension } from "@codemirror/state"
import { EditorView } from "@codemirror/view"
import { tags } from "@lezer/highlight"

// Each colour is a design token, so the editor follows the colour scheme without a new configuration
const theme = EditorView.theme({
  "&": {
    height: "100%",
    backgroundColor: "var(--surface-base)",
    color: "var(--text-primary)",
    fontSize: "var(--font-size-code)",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": {
    fontFamily: "var(--font-code)",
    lineHeight: "var(--line-height-code)",
    fontVariantLigatures: "none",
    scrollbarColor: "var(--border-strong) transparent",
  },
  ".cm-content": { padding: "6px 0", caretColor: "var(--text-primary)" },
  ".cm-line": { padding: "0 16px 0 4px" },
  ".cm-cursor, .cm-dropCursor": { borderLeft: "1.5px solid var(--text-primary)" },
  ".cm-gutters": {
    border: "none",
    backgroundColor: "var(--surface-base)",
    color: "var(--text-tertiary)",
  },
  ".cm-lineNumbers .cm-gutterElement": {
    minWidth: "44px",
    padding: "0 12px 0 12px",
    fontVariantNumeric: "tabular-nums",
  },
  ".cm-activeLine": { backgroundColor: "var(--editor-active-line)" },
  ".cm-activeLineGutter": { backgroundColor: "transparent", color: "var(--text-primary)" },
  "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground": {
    backgroundColor: "var(--editor-selection)",
  },
  ".cm-selectionMatch": { backgroundColor: "var(--editor-match)" },
  ".cm-searchMatch": { backgroundColor: "var(--editor-match)", outline: "none" },
  ".cm-searchMatch.cm-searchMatch-selected": { backgroundColor: "var(--editor-match-selected)" },
  "&.cm-focused .cm-matchingBracket": {
    backgroundColor: "transparent",
    outline: "1px solid var(--border-strong)",
  },
  "&.cm-focused .cm-nonmatchingBracket": { backgroundColor: "transparent", color: "var(--error)" },
  ".cm-panels": {
    backgroundColor: "var(--surface-raised)",
    color: "var(--text-primary)",
    fontFamily: "var(--font-ui)",
    fontSize: "var(--font-size-small)",
  },
  ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--border)" },
  ".cm-panel.cm-search": { padding: "6px 32px 6px 12px" },
  ".cm-panel.cm-search label": {
    display: "inline-flex",
    alignItems: "center",
    gap: "4px",
    marginRight: "8px",
    color: "var(--text-secondary)",
    fontSize: "var(--font-size-small)",
  },
  ".cm-panel.cm-search [name=close]": { color: "var(--text-secondary)", fontSize: "16px", cursor: "pointer" },
  ".cm-textfield": {
    padding: "3px 6px",
    border: "1px solid var(--border-strong)",
    borderRadius: "var(--radius-small)",
    backgroundColor: "var(--surface-base)",
    fontSize: "var(--font-size-small)",
  },
  ".cm-button": {
    padding: "3px 8px",
    border: "1px solid var(--border-strong)",
    borderRadius: "var(--radius-small)",
    backgroundColor: "transparent",
    backgroundImage: "none",
    fontSize: "var(--font-size-small)",
    cursor: "pointer",
  },
  ".cm-button:hover, .cm-button:active": { backgroundColor: "var(--surface-hover)", backgroundImage: "none" },
})

const highlightStyle = HighlightStyle.define([
  { tag: tags.keyword, color: "var(--syntax-keyword)" },
  { tag: [tags.string, tags.special(tags.string), tags.regexp, tags.monospace], color: "var(--syntax-string)" },
  { tag: [tags.number, tags.bool, tags.null, tags.atom, tags.escape, tags.color], color: "var(--syntax-constant)" },
  {
    tag: [
      tags.function(tags.variableName),
      tags.function(tags.propertyName),
      tags.macroName,
      tags.heading,
      tags.link,
      tags.url,
    ],
    color: "var(--syntax-function)",
  },
  { tag: [tags.typeName, tags.className, tags.namespace, tags.attributeName], color: "var(--syntax-type)" },
  { tag: [tags.tagName, tags.labelName], color: "var(--syntax-tag)" },
  { tag: [tags.comment, tags.meta, tags.processingInstruction], color: "var(--text-tertiary)" },
  { tag: [tags.punctuation, tags.operator, tags.quote], color: "var(--text-secondary)" },
  { tag: [tags.heading, tags.strong], fontWeight: "600" },
  { tag: tags.emphasis, fontStyle: "italic" },
  { tag: tags.strikethrough, textDecoration: "line-through" },
  { tag: tags.invalid, color: "var(--error)" },
])

export const editorTheme: Extension = [theme, syntaxHighlighting(highlightStyle)]

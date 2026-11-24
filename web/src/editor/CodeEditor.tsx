import { defaultKeymap, history, historyKeymap, indentWithTab } from "@codemirror/commands"
import { bracketMatching, indentOnInput } from "@codemirror/language"
import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from "@codemirror/search"
import { Compartment, EditorState, type Extension, type StateEffect } from "@codemirror/state"
import {
  drawSelection,
  EditorView,
  highlightActiveLine,
  highlightActiveLineGutter,
  keymap,
  lineNumbers,
} from "@codemirror/view"
import { createEffect, on, onCleanup } from "solid-js"
import { reportFailure } from "../layout/failureNotice"
import { registerCommandShortcuts } from "../layout/keyboardShortcuts"
import type { OpenWorkspace } from "../workspace/workspaceStore"
import { languageOf } from "./editorLanguages"
import { editorTheme } from "./editorTheme"

// Sentence case for the labels of the search panel, which are lower case by default
const searchPhrases = {
  next: "Next",
  previous: "Previous",
  all: "All",
  "match case": "Match case",
  regexp: "Regular expression",
  "by word": "Whole word",
  replace: "Replace",
  "replace all": "Replace all",
  close: "Close",
}

export type CursorPosition = { readonly line: number, readonly column: number }

// The editor state of one open file, with the buffer text that the workspace store last exchanged with it
type DocumentBuffer = { state: EditorState, text: string, scroll: StateEffect<unknown> | undefined }

export default function CodeEditor(props: {
  workspace: OpenWorkspace
  onCursorChange: (cursor: CursorPosition | undefined) => void
}) {
  const buffers = new Map<string, DocumentBuffer>()
  const language = new Compartment()
  const view = new EditorView()
  let shownPath: string | undefined

  function reportCursor(state: EditorState): void {
    const head = state.selection.main.head
    const line = state.doc.lineAt(head)
    props.onCursorChange({ line: line.number, column: head - line.from + 1 })
  }

  const extensions: Extension = [
    lineNumbers(),
    highlightActiveLineGutter(),
    highlightActiveLine(),
    drawSelection(),
    history(),
    indentOnInput(),
    bracketMatching(),
    highlightSelectionMatches(),
    search({ top: true }),
    EditorState.phrases.of(searchPhrases),
    keymap.of([...defaultKeymap, ...historyKeymap, ...searchKeymap, indentWithTab]),
    editorTheme,
    EditorView.updateListener.of((update) => {
      const buffer = shownPath === undefined ? undefined : buffers.get(shownPath)
      if (shownPath === undefined || !buffer) return
      buffer.state = update.state
      if (update.docChanged) {
        buffer.text = update.state.sliceDoc()
        props.workspace.setBufferText(shownPath, buffer.text)
      }
      if (update.docChanged || update.selectionSet) reportCursor(update.state)
    }),
  ]

  async function loadLanguage(path: string): Promise<void> {
    const extension = await languageOf(path)?.load()
    const buffer = buffers.get(path)
    if (!extension || !buffer) return
    const transaction = buffer.state.update({ effects: language.reconfigure(extension) })
    if (shownPath === path) view.dispatch(transaction)
    else buffer.state = transaction.state
  }

  // Makes a new editor state for text that did not come from this editor, and keeps the cursor of the earlier state
  function createBuffer(path: string, text: string, earlier: DocumentBuffer | undefined): DocumentBuffer {
    const state = EditorState.create({
      doc: text,
      extensions: [
        extensions,
        language.of((earlier && language.get(earlier.state)) ?? []),
        // A file with Windows line ends keeps them, the default splits and joins lines with a line feed
        text.includes("\r\n") ? EditorState.lineSeparator.of("\r\n") : [],
      ],
    })
    if (!earlier) {
      loadLanguage(path).catch(reportFailure)
      return { state, text, scroll: undefined }
    }
    const cursor = Math.min(earlier.state.selection.main.head, state.doc.length)
    return { state: state.update({ selection: { anchor: cursor } }).state, text, scroll: earlier.scroll }
  }

  function show(path: string, bufferText: string): void {
    const known = buffers.get(path)
    const current = known?.text === bufferText
    if (current && shownPath === path) return

    const shown = shownPath === undefined ? undefined : buffers.get(shownPath)
    if (shown) shown.scroll = view.scrollSnapshot()

    const buffer = current ? known : createBuffer(path, bufferText, known)
    buffers.set(path, buffer)
    shownPath = path
    view.setState(buffer.state)
    if (buffer.scroll) view.dispatch({ effects: buffer.scroll })
    reportCursor(buffer.state)
  }

  createEffect(() => {
    const document = props.workspace.activeDocument()
    if (document) show(document.path, document.bufferText)
  })

  createEffect(() => {
    const openPaths = new Set(props.workspace.documents.map((document) => document.path))
    for (const path of buffers.keys()) {
      if (!openPaths.has(path)) buffers.delete(path)
    }
  })

  // This effect is after the effect that shows the active document, so the revealed file is in the view
  createEffect(
    on(props.workspace.reveal, (reveal) => {
      if (!reveal || reveal.path !== shownPath) return
      if (reveal.line !== undefined) {
        const lineNumber = Math.min(Math.max(Math.trunc(reveal.line) || 1, 1), view.state.doc.lines)
        const position = view.state.doc.line(lineNumber).from
        view.dispatch({
          selection: { anchor: position },
          effects: EditorView.scrollIntoView(position, { y: "center" }),
        })
      }
      view.focus()
    }),
  )

  // The editor key binding opens the search only when the editor has the focus
  registerCommandShortcuts({ f: () => openSearchPanel(view) })

  onCleanup(() => {
    view.destroy()
    props.onCursorChange(undefined)
  })

  return <div class="code-editor" ref={(element) => element.append(view.dom)} />
}

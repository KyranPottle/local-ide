import { For, lazy, Show, Suspense } from "solid-js"
import { Icon } from "../design/Icon"
import { reportFailure } from "../layout/failureNotice"
import { registerCommandShortcuts } from "../layout/keyboardShortcuts"
import { moveTabFocus } from "../layout/tabListFocus"
import { fileName } from "../workspace/WorkspaceFileSystem"
import { documentState, type EditorDocument, type OpenWorkspace } from "../workspace/workspaceStore"
import type { CursorPosition } from "./CodeEditor"
import "./EditorArea.css"

const CodeEditor = lazy(() => import("./CodeEditor"))

const shortcutHints = [
  ["Find file", "⌘P"],
  ["Open folder", "⌘O"],
  ["Save file", "⌘S"],
  ["Toggle sidebar", "⌘B"],
] as const

function EditorHints() {
  return (
    <dl class="editor-hints">
      <For each={shortcutHints}>
        {([action, keys]) => (
          <div class="editor-hint">
            <dt>{action}</dt>
            <dd>{keys}</dd>
          </div>
        )}
      </For>
    </dl>
  )
}

export function EditorArea(props: {
  workspace: OpenWorkspace
  onCursorChange: (cursor: CursorPosition | undefined) => void
}) {
  function selectTab(document: EditorDocument): void {
    props.workspace.openFile(document.path).catch(reportFailure)
  }

  function closeTab(document: EditorDocument): void {
    const discard =
      documentState(document) === "saved" || window.confirm(`Discard the changes to ${fileName(document.path)}?`)
    if (discard) props.workspace.closeFile(document.path)
  }

  function withActiveDocument(action: (document: EditorDocument) => void): () => void {
    return () => {
      const document = props.workspace.activeDocument()
      if (document) action(document)
    }
  }

  registerCommandShortcuts({
    s: withActiveDocument((document) => props.workspace.saveFile(document.path).catch(reportFailure)),
    w: withActiveDocument(closeTab),
  })

  const tabKeyActions: Readonly<Record<string, (document: EditorDocument) => void>> = {
    Enter: selectTab,
    " ": selectTab,
    Delete: closeTab,
  }

  return (
    <section class="editor-area" aria-label="Editor">
      <div class="editor-tabs" role="tablist" aria-label="Open files" onKeyDown={moveTabFocus}>
        <For each={props.workspace.documents}>
          {(document) => {
            const name = () => fileName(document.path)
            const active = () => props.workspace.activeFilePath() === document.path
            return (
              <div
                class="editor-tab"
                role="tab"
                aria-selected={active()}
                aria-label={documentState(document) === "saved" ? name() : `${name()}, modified`}
                title={document.path}
                tabindex={active() ? 0 : -1}
                data-state={documentState(document)}
                onClick={() => selectTab(document)}
                onAuxClick={(event) => event.button === 1 && closeTab(document)}
                onKeyDown={(event) => {
                  if (event.target !== event.currentTarget || !Object.hasOwn(tabKeyActions, event.key)) return
                  event.preventDefault()
                  tabKeyActions[event.key]?.(document)
                }}
              >
                <span class="editor-tab-name">{name()}</span>
                <button
                  class="icon-button icon-button-small editor-tab-close"
                  type="button"
                  aria-label={`Close ${name()}`}
                  tabindex="-1"
                  onClick={(event) => {
                    event.stopPropagation()
                    closeTab(document)
                  }}
                >
                  <Icon name="close" />
                </button>
              </div>
            )
          }}
        </For>
      </div>
      <Show when={props.workspace.activeDocument()} fallback={<EditorHints />}>
        {(document) => (
          <>
            <Show when={documentState(document()) === "conflict"}>
              <div class="editor-conflict" role="alert">
                <span>This file changed on disk.</span>
                <button
                  class="button button-small"
                  type="button"
                  onClick={() => props.workspace.discardBuffer(document().path)}
                >
                  Reload
                </button>
                <button
                  class="button button-small"
                  type="button"
                  onClick={() => props.workspace.keepBuffer(document().path)}
                >
                  Keep mine
                </button>
              </div>
            </Show>
            <Suspense>
              <CodeEditor workspace={props.workspace} onCursorChange={props.onCursorChange} />
            </Suspense>
          </>
        )}
      </Show>
    </section>
  )
}

import { createMemo, createSignal, For, onCleanup, onMount, Show } from "solid-js"
import { reportFailure } from "../layout/failureNotice"
import { fileName, parentDirectory, type WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"
import type { OpenWorkspace } from "../workspace/workspaceStore"
import { rankPaths } from "./fuzzyMatch"
import "./QuickFileFinder.css"

const FILE_PATH_LIMIT = 5000
const RESULT_LIMIT = 50

async function listFilePaths(fileSystem: WorkspaceFileSystem): Promise<string[]> {
  const paths: string[] = []
  for await (const path of fileSystem.walkFiles()) {
    paths.push(path)
    if (paths.length === FILE_PATH_LIMIT) break
  }
  return paths
}

export function QuickFileFinder(props: { workspace: OpenWorkspace, onClose: () => void, onChoose?: (path: string) => void }) {
  const [paths, setPaths] = createSignal<string[]>()
  const [query, setQuery] = createSignal("")
  const [selectedIndex, setSelectedIndex] = createSignal(0)
  const results = createMemo(() => rankPaths(query(), paths() ?? [], RESULT_LIMIT))
  const focusedBefore = document.activeElement
  let input!: HTMLInputElement
  let list!: HTMLUListElement

  function select(index: number): void {
    const clamped = Math.max(Math.min(index, results().length - 1), 0)
    setSelectedIndex(clamped)
    list.children[clamped]?.scrollIntoView({ block: "nearest" })
  }

  function openResult(path: string | undefined): void {
    if (path === undefined) return
    props.onClose()
    if (props.onChoose) {
      props.onChoose(path)
      return
    }
    props.workspace.openFile(path).catch(reportFailure)
  }

  const keyActions: Readonly<Record<string, () => void>> = {
    ArrowDown: () => select(selectedIndex() + 1),
    ArrowUp: () => select(selectedIndex() - 1),
    Enter: () => openResult(results()[selectedIndex()]),
    Escape: props.onClose,
    // The dialog holds one control, so the Tab key keeps the focus in it
    Tab: () => undefined,
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (!Object.hasOwn(keyActions, event.key)) return
    event.preventDefault()
    keyActions[event.key]?.()
  }

  onMount(() => {
    input.focus()
    listFilePaths(props.workspace.fileSystem).then(setPaths, reportFailure)
  })
  onCleanup(() => {
    if (focusedBefore instanceof HTMLElement) focusedBefore.focus()
  })

  return (
    <div class="file-finder-backdrop" onMouseDown={(event) => event.target === event.currentTarget && props.onClose()}>
      <div class="file-finder" role="dialog" aria-modal="true" aria-label="Find file">
        <input
          ref={input}
          class="file-finder-input"
          type="text"
          role="combobox"
          aria-label="File name"
          aria-expanded="true"
          aria-controls="file-finder-results"
          aria-activedescendant={`file-finder-result-${selectedIndex()}`}
          autocomplete="off"
          spellcheck={false}
          placeholder="Find file by name"
          value={query()}
          onInput={(event) => {
            setQuery(event.currentTarget.value)
            select(0)
          }}
          onKeyDown={onKeyDown}
        />
        <ul ref={list} id="file-finder-results" class="file-finder-results" role="listbox" aria-label="Files">
          <For each={results()}>
            {(path, index) => (
              <li
                id={`file-finder-result-${index()}`}
                class="file-finder-result"
                role="option"
                aria-selected={index() === selectedIndex()}
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => openResult(path)}
              >
                <span class="file-finder-name">{fileName(path)}</span>
                <span class="file-finder-directory">{parentDirectory(path)}</span>
              </li>
            )}
          </For>
        </ul>
        <Show when={results().length === 0}>
          <p class="file-finder-status">{paths() === undefined ? "Listing files" : "No matching files"}</p>
        </Show>
      </div>
    </div>
  )
}

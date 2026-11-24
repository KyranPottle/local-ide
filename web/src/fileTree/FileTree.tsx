import { createEffect, createSignal, For, type JSX, on, Show } from "solid-js"
import { Icon } from "../design/Icon"
import { reportFailure } from "../layout/failureNotice"
import {
  type DirectoryEntry,
  IGNORED_DIRECTORY_NAMES,
  joinPath,
  parentDirectory,
  ROOT_DIRECTORY,
  WorkspaceFileError,
} from "../workspace/WorkspaceFileSystem"
import type { OpenWorkspace } from "../workspace/workspaceStore"
import "./FileTree.css"

const INDENT_WIDTH = 12

type TreeItem = { readonly path: string, readonly kind: FileSystemHandleKind }
type VisibleItem = TreeItem & { readonly element: HTMLElement }

export function FileTree(props: { workspace: OpenWorkspace }) {
  const [listings, setListings] = createSignal<ReadonlyMap<string, DirectoryEntry[]>>(new Map())
  const [expandedPaths, setExpandedPaths] = createSignal<ReadonlySet<string>>(new Set())
  const [cursorPath, setCursorPath] = createSignal<string>()
  const itemIds = new Map<string, string>()
  let tree!: HTMLUListElement

  function itemId(path: string): string {
    const id = itemIds.get(path) ?? `file-tree-item-${itemIds.size}`
    itemIds.set(path, id)
    return id
  }

  function forgetDirectory(path: string): void {
    setListings((current) => new Map([...current].filter(([directory]) => directory !== path)))
    setExpandedPaths((current) => new Set([...current].filter((directory) => directory !== path)))
  }

  async function loadDirectory(path: string): Promise<void> {
    try {
      const entries = await props.workspace.fileSystem.listDirectory(path)
      setListings((current) => new Map(current).set(path, entries))
    } catch (error) {
      // A directory that another program removed leaves the tree at the next reload
      if (!(error instanceof WorkspaceFileError)) throw error
      forgetDirectory(path)
    }
  }

  function setExpanded(path: string, expanded: boolean): void {
    setExpandedPaths((current) => {
      const next = new Set(current)
      if (expanded) next.add(path)
      else next.delete(path)
      return next
    })
    if (expanded) loadDirectory(path).catch(reportFailure)
  }

  function activate(item: TreeItem): void {
    setCursorPath(item.path)
    if (item.kind === "directory") setExpanded(item.path, !expandedPaths().has(item.path))
    else props.workspace.openFile(item.path).catch(reportFailure)
  }

  function visibleItems(): VisibleItem[] {
    return [...tree.querySelectorAll<HTMLElement>('[role="treeitem"]')].map((element) => ({
      path: element.dataset.path ?? "",
      kind: element.dataset.kind === "directory" ? "directory" : "file",
      element,
    }))
  }

  function moveCursor(target: VisibleItem | undefined): void {
    if (!target) return
    setCursorPath(target.path)
    target.element.firstElementChild?.scrollIntoView({ block: "nearest" })
  }

  function expandOrEnter(items: VisibleItem[], index: number): void {
    const cursor = items[index]
    if (cursor?.kind !== "directory") return
    if (expandedPaths().has(cursor.path)) moveCursor(items[index + 1])
    else setExpanded(cursor.path, true)
  }

  function collapseOrLeave(items: VisibleItem[], index: number): void {
    const cursor = items[index]
    if (!cursor) return
    if (cursor.kind === "directory" && expandedPaths().has(cursor.path)) setExpanded(cursor.path, false)
    else moveCursor(items.find((item) => item.path === parentDirectory(cursor.path)))
  }

  function activateCursor(items: VisibleItem[], index: number): void {
    const cursor = items[index]
    if (cursor) activate(cursor)
  }

  const keyActions: Readonly<Record<string, (items: VisibleItem[], index: number) => void>> = {
    ArrowDown: (items, index) => moveCursor(items[index + 1]),
    ArrowUp: (items, index) => moveCursor(items[Math.max(index - 1, 0)]),
    Home: (items) => moveCursor(items[0]),
    End: (items) => moveCursor(items.at(-1)),
    ArrowRight: expandOrEnter,
    ArrowLeft: collapseOrLeave,
    Enter: activateCursor,
    " ": activateCursor,
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (!Object.hasOwn(keyActions, event.key)) return
    event.preventDefault()
    const items = visibleItems()
    keyActions[event.key]?.(
      items,
      items.findIndex((item) => item.path === cursorPath()),
    )
  }

  function cursorItemId(): string | undefined {
    const path = cursorPath()
    return path === undefined ? undefined : itemId(path)
  }

  const entriesOf = (directory: string, level: number): JSX.Element => (
    <For each={listings().get(directory)}>
      {(entry) => {
        const path = joinPath(directory, entry.name)
        const expanded = () => entry.kind === "directory" && expandedPaths().has(path)
        return (
          <li
            id={itemId(path)}
            class="file-tree-item"
            classList={{
              "file-tree-item-cursor": cursorPath() === path,
              "file-tree-item-dimmed": entry.kind === "directory" && IGNORED_DIRECTORY_NAMES.has(entry.name),
            }}
            role="treeitem"
            aria-level={level}
            aria-expanded={entry.kind === "directory" ? expanded() : undefined}
            aria-selected={props.workspace.activeFilePath() === path}
            data-path={path}
            data-kind={entry.kind}
          >
            <div
              class="file-tree-row"
              style={{ "padding-left": `${8 + (level - 1) * INDENT_WIDTH}px` }}
              onClick={() => activate({ path, kind: entry.kind })}
            >
              <span class="file-tree-chevron" classList={{ "file-tree-chevron-expanded": expanded() }}>
                <Show when={entry.kind === "directory"}>
                  <Icon name="chevron" />
                </Show>
              </span>
              <Icon name={entry.kind === "directory" ? "folder" : "file"} />
              <span class="file-tree-name">{entry.name}</span>
            </div>
            <Show when={expanded()}>
              <ul role="group">{entriesOf(path, level + 1)}</ul>
            </Show>
          </li>
        )
      }}
    </For>
  )

  createEffect(
    on(props.workspace.fileTreeRevision, () => {
      Promise.all([ROOT_DIRECTORY, ...expandedPaths()].map(loadDirectory)).catch(reportFailure)
    }),
  )

  return (
    <div class="file-tree">
      <div class="file-tree-header">
        <span class="file-tree-title">{props.workspace.name}</span>
        <button
          class="icon-button icon-button-small"
          type="button"
          aria-label="Refresh files"
          title="Refresh files"
          onClick={props.workspace.refreshFileTree}
        >
          <Icon name="refresh" />
        </button>
      </div>
      <ul
        ref={tree}
        class="file-tree-items"
        role="tree"
        aria-label="Files"
        aria-activedescendant={cursorItemId()}
        tabindex="0"
        onKeyDown={onKeyDown}
      >
        {entriesOf(ROOT_DIRECTORY, 1)}
      </ul>
    </div>
  )
}

import { type Component, createSignal, For, Show } from "solid-js"
import { ColorSchemeToggle } from "../design/ColorSchemeToggle"
import { Icon } from "../design/Icon"
import type { CursorPosition } from "../editor/CodeEditor"
import { EditorArea } from "../editor/EditorArea"
import { languageOf } from "../editor/editorLanguages"
import { FileTree } from "../fileTree/FileTree"
import { folderAccessSupport } from "../workspace/directoryAccess"
import { type DocumentState, documentState, type OpenWorkspace } from "../workspace/workspaceStore"
import { registerCommandShortcuts } from "./keyboardShortcuts"
import { createPanelLayout, type PanelName, panelWidthRanges } from "./panelLayout"
import { ResizeSeparator } from "./ResizeSeparator"
import { moveTabFocus } from "./tabListFocus"
import "./IdeLayout.css"

export type SidebarTab = { readonly id: string, readonly label: string, readonly content: Component }

// A slot without content is not in the page
export type IdeLayoutSlots = {
  // Second tab of the sidebar, after Files
  sidebarTab?: SidebarTab
  rightPanel?: Component
  // Right-aligned content of the top bar, before the colour scheme toggle
  toolbar?: Component
  // Right-aligned content of the status bar
  statusBar?: Component
  // Cmd+L shows the right panel, then this moves the focus into it
  focusRightPanel?: () => void
}

const FILES_TAB_ID = "files"

const documentStateLabels: Readonly<Record<DocumentState, string>> = {
  saved: "Saved",
  modified: "Modified",
  conflict: "Changed on disk",
}

export function IdeLayout(props: IdeLayoutSlots & { workspace: OpenWorkspace, onOpenFolder: () => void }) {
  const panels = createPanelLayout()
  const [sidebarTabId, setSidebarTabId] = createSignal(FILES_TAB_ID)
  const [cursor, setCursor] = createSignal<CursorPosition>()

  const filesTab: SidebarTab = {
    id: FILES_TAB_ID,
    label: "Files",
    content: () => <FileTree workspace={props.workspace} />,
  }
  const sidebarTabs = () => (props.sidebarTab ? [filesTab, props.sidebarTab] : [filesTab])

  registerCommandShortcuts({
    b: () => panels.toggle("sidebar"),
    j: () => props.rightPanel && panels.toggle("rightPanel"),
    l: () => {
      if (!props.rightPanel) return
      panels.expand("rightPanel")
      props.focusRightPanel?.()
    },
  })

  const separator = (panel: PanelName, label: string, growth: 1 | -1) => (
    <ResizeSeparator
      label={label}
      width={panels.layout[panel].width}
      minimum={panelWidthRanges[panel].minimum}
      maximum={panelWidthRanges[panel].maximum}
      growth={growth}
      onResize={(width) => panels.resize(panel, width)}
    />
  )

  return (
    <div class="ide">
      <header class="top-bar">
        <button
          class="icon-button"
          type="button"
          aria-label="Toggle sidebar"
          aria-pressed={!panels.layout.sidebar.collapsed}
          title="Toggle sidebar (⌘B)"
          onClick={() => panels.toggle("sidebar")}
        >
          <Icon name="sidebar" />
        </button>
        <span class="top-bar-workspace">{props.workspace.name}</span>
        <button
          class="icon-button"
          type="button"
          aria-label="Open folder"
          title="Open folder (⌘O)"
          disabled={folderAccessSupport().kind !== "available"}
          onClick={props.onOpenFolder}
        >
          <Icon name="folder" />
        </button>
        <div class="top-bar-toolbar" role="toolbar" aria-label="Workspace tools">
          <Show when={props.toolbar} keyed>
            {(Toolbar) => <Toolbar />}
          </Show>
          <ColorSchemeToggle />
          <Show when={props.rightPanel}>
            <button
              class="icon-button"
              type="button"
              aria-label="Toggle right panel"
              aria-pressed={!panels.layout.rightPanel.collapsed}
              title="Toggle right panel (⌘J)"
              onClick={() => panels.toggle("rightPanel")}
            >
              <Icon name="rightPanel" />
            </button>
          </Show>
        </div>
      </header>

      <div class="ide-body">
        {/* A collapsed panel stays in the page, so that its content keeps its state */}
        <aside
          class="sidebar"
          aria-label="Sidebar"
          hidden={panels.layout.sidebar.collapsed}
          style={{ width: `${panels.layout.sidebar.width}px` }}
        >
          <div class="sidebar-tabs" role="tablist" aria-label="Sidebar" onKeyDown={moveTabFocus}>
            <For each={sidebarTabs()}>
              {(tab) => (
                <button
                  class="sidebar-tab"
                  type="button"
                  role="tab"
                  id={`sidebar-tab-${tab.id}`}
                  aria-selected={sidebarTabId() === tab.id}
                  aria-controls={`sidebar-panel-${tab.id}`}
                  tabindex={sidebarTabId() === tab.id ? 0 : -1}
                  // A click and an arrow key both move the focus to the tab
                  onFocus={() => setSidebarTabId(tab.id)}
                >
                  {tab.label}
                </button>
              )}
            </For>
          </div>
          <For each={sidebarTabs()}>
            {(tab) => (
              <div
                class="sidebar-panel"
                role="tabpanel"
                id={`sidebar-panel-${tab.id}`}
                aria-labelledby={`sidebar-tab-${tab.id}`}
                hidden={sidebarTabId() !== tab.id}
              >
                <tab.content />
              </div>
            )}
          </For>
        </aside>
        <Show when={!panels.layout.sidebar.collapsed}>{separator("sidebar", "Resize sidebar", 1)}</Show>

        <EditorArea workspace={props.workspace} onCursorChange={setCursor} />

        <Show when={props.rightPanel} keyed>
          {(RightPanel) => (
            <>
              <Show when={!panels.layout.rightPanel.collapsed}>
                {separator("rightPanel", "Resize right panel", -1)}
              </Show>
              <aside
                class="right-panel"
                aria-label="Right panel"
                hidden={panels.layout.rightPanel.collapsed}
                style={{ width: `${panels.layout.rightPanel.width}px` }}
              >
                <RightPanel />
              </aside>
            </>
          )}
        </Show>
      </div>

      <footer class="status-bar">
        <Show when={props.workspace.activeDocument()}>
          {(document) => (
            <>
              <Show when={cursor()}>
                {(position) => (
                  <span class="status-item status-item-numeric">
                    Ln {position().line}, Col {position().column}
                  </span>
                )}
              </Show>
              <span class="status-item">{languageOf(document().path)?.name ?? "Plain text"}</span>
              <span class="status-item">{documentStateLabels[documentState(document())]}</span>
            </>
          )}
        </Show>
        <div class="status-bar-end">
          <Show when={props.statusBar} keyed>
            {(StatusBar) => <StatusBar />}
          </Show>
        </div>
      </footer>
    </div>
  )
}

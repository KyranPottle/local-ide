import { createEffect } from "solid-js"
import { createStore } from "solid-js/store"
import { readPreference, writePreference } from "../preferences/storedPreference"

export type PanelName = "sidebar" | "rightPanel"
type PanelState = { width: number, collapsed: boolean }
type PanelLayout = Record<PanelName, PanelState>
type WidthRange = { readonly minimum: number, readonly initial: number, readonly maximum: number }

const PREFERENCE_KEY = "local-ide.panel-layout"

export const panelWidthRanges: Readonly<Record<PanelName, WidthRange>> = {
  sidebar: { minimum: 180, initial: 260, maximum: 560 },
  rightPanel: { minimum: 300, initial: 420, maximum: 800 },
}

function clampWidth(panel: PanelName, width: number): number {
  const { minimum, maximum } = panelWidthRanges[panel]
  return Math.round(Math.min(Math.max(width, minimum), maximum))
}

function property(value: unknown, key: string): unknown {
  return value instanceof Object ? (value as Record<string, unknown>)[key] : undefined
}

function storedPanelState(stored: unknown, panel: PanelName): PanelState {
  const state = property(stored, panel)
  const width = property(state, "width")
  return {
    width: typeof width === "number" ? clampWidth(panel, width) : panelWidthRanges[panel].initial,
    collapsed: property(state, "collapsed") === true,
  }
}

function storedPanelLayout(): PanelLayout {
  let stored: unknown
  try {
    stored = JSON.parse(readPreference(PREFERENCE_KEY) ?? "null")
  } catch {
    stored = null
  }
  return { sidebar: storedPanelState(stored, "sidebar"), rightPanel: storedPanelState(stored, "rightPanel") }
}

// Call inside a component: the layout saves each change for the lifetime of that component
export function createPanelLayout() {
  const [layout, setLayout] = createStore(storedPanelLayout())
  createEffect(() => writePreference(PREFERENCE_KEY, JSON.stringify(layout)))

  return {
    layout,
    resize(panel: PanelName, width: number): void {
      setLayout(panel, "width", clampWidth(panel, width))
    },
    expand(panel: PanelName): void {
      setLayout(panel, "collapsed", false)
    },
    toggle(panel: PanelName): void {
      setLayout(panel, "collapsed", (collapsed) => !collapsed)
    },
  }
}

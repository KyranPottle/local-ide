import { createSignal, For, onCleanup, Show } from "solid-js"
import { isRunActive, modelLabel } from "../agent/sessionState"
import type { SessionStore } from "../agent/sessionStore"
import { Icon } from "../design/Icon"
import { reportFailure } from "../layout/failureNotice"
import type { Session } from "../protocol"
import { formatRelativeTime } from "./relativeTime"
import "./SessionList.css"

const SEARCH_DEBOUNCE_MILLISECONDS = 200
const CLOCK_TICK_MILLISECONDS = 30_000

type RowMode = { readonly kind: "idle" } | { readonly kind: "renaming" } | { readonly kind: "confirming_delete" }

const rowSelector = ".session-row-select"
const rowFocusSteps: Readonly<Record<string, number>> = { ArrowDown: 1, ArrowUp: -1 }

function moveRowFocus(event: KeyboardEvent & { currentTarget: HTMLElement }): void {
  const step = rowFocusSteps[event.key]
  if (step === undefined || !(event.target instanceof HTMLElement) || !event.target.matches(rowSelector)) return
  event.preventDefault()
  const rows = [...event.currentTarget.querySelectorAll<HTMLElement>(rowSelector)]
  rows[rows.indexOf(event.target) + step]?.focus()
}

function SessionRow(props: {
  store: SessionStore
  session: Session
  showWorkspace: boolean
  now: number
}) {
  const [mode, setMode] = createSignal<RowMode>({ kind: "idle" })
  const store = props.store
  const selected = () => store.state.selectedId === props.session.id
  const active = () => {
    const { run } = store.state
    return store.state.engineActiveRun === props.session.id || (isRunActive(run) && run.sessionId === props.session.id)
  }

  async function fork(): Promise<void> {
    await store.loadSession(props.session.id)
    const lastSeq = store.state.loaded[props.session.id]?.events.at(-1)?.seq
    if (lastSeq === undefined) throw new Error("the session has no events to fork")
    await store.fork(props.session.id, lastSeq)
  }

  function commitRename(input: HTMLInputElement): void {
    if (mode().kind !== "renaming") return
    setMode({ kind: "idle" })
    const title = input.value.trim()
    if (title !== "" && title !== props.session.title) store.rename(props.session.id, title).catch(reportFailure)
  }

  return (
    <li class="session-row" classList={{ "session-row-selected": selected() }}>
      <Show when={mode().kind === "renaming"}>
        <input
          ref={(input) => queueMicrotask(() => input.select())}
          class="session-row-rename"
          aria-label="Session title"
          value={props.session.title}
          onBlur={(event) => commitRename(event.currentTarget)}
          onKeyDown={(event) => {
            if (event.key === "Enter") commitRename(event.currentTarget)
            if (event.key === "Escape") setMode({ kind: "idle" })
          }}
        />
      </Show>
      <Show when={mode().kind === "confirming_delete"}>
        <div class="session-row-confirm" role="group" aria-label="Delete session">
          <span>Delete this session?</span>
          <button
            class="button button-small"
            type="button"
            ref={(button) => queueMicrotask(() => button.focus())}
            onClick={() => setMode({ kind: "idle" })}
          >
            Cancel
          </button>
          <button
            class="button button-small session-row-delete"
            type="button"
            onClick={() => store.deleteSession(props.session.id).catch(reportFailure)}
          >
            Delete
          </button>
        </div>
      </Show>
      <Show when={mode().kind === "idle"}>
        <button
          class="session-row-select"
          type="button"
          aria-current={selected() ? "true" : undefined}
          onClick={() => store.selectSession(props.session.id).catch(reportFailure)}
        >
          <span class="session-row-title">
            <Show when={props.session.origin.kind === "fork"}>
              <span class="session-row-fork" title="Fork"><Icon name="fork" /></span>
            </Show>
            <span class="session-row-title-text">{props.session.title || "Untitled"}</span>
            <Show when={active()}>
              <span class="session-row-activity" role="status" aria-label="Running" title="Running" />
            </Show>
          </span>
          <span class="session-row-meta">
            <span class="session-row-model">{modelLabel(store.state.models, props.session.model)}</span>
            <Show when={props.showWorkspace}>
              <span class="session-row-workspace">{props.session.workspace}</span>
            </Show>
            <span class="session-row-time">{formatRelativeTime(props.session.updated_at, props.now)}</span>
          </span>
        </button>
        <div class="session-row-actions">
          <button class="icon-button icon-button-small" type="button" aria-label="Rename" title="Rename" onClick={() => setMode({ kind: "renaming" })}>
            <Icon name="pencil" />
          </button>
          <button class="icon-button icon-button-small" type="button" aria-label="Fork" title="Fork" onClick={() => fork().catch(reportFailure)}>
            <Icon name="fork" />
          </button>
          <button
            class="icon-button icon-button-small"
            type="button"
            aria-label="Delete"
            title="Delete"
            onClick={() => setMode({ kind: "confirming_delete" })}
          >
            <Icon name="trash" />
          </button>
        </div>
      </Show>
    </li>
  )
}

export function SessionList(props: { store: SessionStore, workspaceName: string }) {
  const store = props.store
  const [now, setNow] = createSignal(Date.now())
  const clock = setInterval(() => setNow(Date.now()), CLOCK_TICK_MILLISECONDS)
  let searchTimer: ReturnType<typeof setTimeout> | undefined
  onCleanup(() => {
    clearInterval(clock)
    clearTimeout(searchTimer)
  })

  const topLevel = () => store.state.sessions.filter((session) => session.origin.kind !== "delegate")
  const inWorkspace = () => topLevel().filter((session) => session.workspace === props.workspaceName)
  const elsewhere = () => topLevel().filter((session) => session.workspace !== props.workspaceName)

  function onSearch(text: string): void {
    clearTimeout(searchTimer)
    searchTimer = setTimeout(() => store.search(text).catch(reportFailure), SEARCH_DEBOUNCE_MILLISECONDS)
  }

  return (
    <div class="session-list">
      <div class="session-list-header">
        <input
          class="session-list-search"
          type="search"
          aria-label="Search sessions"
          placeholder="Search sessions"
          value={store.state.searchText}
          onInput={(event) => onSearch(event.currentTarget.value)}
        />
        <button class="icon-button" type="button" aria-label="New session" title="New session" onClick={store.clearSelection}>
          <Icon name="plus" />
        </button>
      </div>
      <div class="session-list-scroll" onKeyDown={moveRowFocus}>
        <ul class="session-list-rows" aria-label="Sessions">
          <For each={inWorkspace()}>{(session) => <SessionRow store={store} session={session} showWorkspace={false} now={now()} />}</For>
        </ul>
        <Show when={elsewhere().length > 0}>
          <h3 class="session-list-heading">Other workspaces</h3>
          <ul class="session-list-rows" aria-label="Sessions of other workspaces">
            <For each={elsewhere()}>{(session) => <SessionRow store={store} session={session} showWorkspace now={now()} />}</For>
          </ul>
        </Show>
        <Show when={topLevel().length === 0}>
          <p class="session-list-empty">{store.state.searchText === "" ? "No sessions yet" : "No matching sessions"}</p>
        </Show>
      </div>
    </div>
  )
}

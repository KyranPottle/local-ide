import { createEffect, createMemo, createSignal, For, on, onCleanup, onMount, Show } from "solid-js"
import { isRunActive, modelLabel } from "../agent/sessionState"
import type { SessionStore } from "../agent/sessionStore"
import { Icon } from "../design/Icon"
import { reportFailure } from "../layout/failureNotice"
import type { Mode } from "../protocol"
import type { OpenWorkspace } from "../workspace/workspaceStore"
import { Composer, type ComposerBlock } from "./Composer"
import { buildTranscript } from "./transcript"
import { type TranscriptActions, TranscriptActionsContext } from "./transcriptActions"
import { TranscriptList } from "./TranscriptView"
import "./AgentPanel.css"

const BUSY_STATUS_POLL_MILLISECONDS = 5000
// Distance from the end that still counts as the bottom, for fractional scroll positions
const BOTTOM_TOLERANCE_PIXELS = 8

const modeLabels: Readonly<Record<Mode, string>> = { agent: "Agent", ask: "Ask" }
const modeOrder: readonly Mode[] = ["agent", "ask"]

function SessionTitle(props: { title: string, onRename: (title: string) => void }) {
  const [editing, setEditing] = createSignal(false)

  function commit(input: HTMLInputElement): void {
    if (!editing()) return
    setEditing(false)
    const title = input.value.trim()
    if (title !== "" && title !== props.title) props.onRename(title)
  }

  return (
    <Show
      when={editing()}
      fallback={
        <button class="agent-title" type="button" title="Rename" onClick={() => setEditing(true)}>
          {props.title}
        </button>
      }
    >
      <input
        ref={(input) => queueMicrotask(() => input.select())}
        class="agent-title-input"
        aria-label="Session title"
        value={props.title}
        onBlur={(event) => commit(event.currentTarget)}
        onKeyDown={(event) => {
          if (event.key === "Enter") commit(event.currentTarget)
          if (event.key === "Escape") setEditing(false)
        }}
      />
    </Show>
  )
}

export function AgentPanel(props: { store: SessionStore, workspace: OpenWorkspace }) {
  const { store } = props
  const [draft, setDraft] = createSignal("")
  const [following, setFollowing] = createSignal(true)
  let scroller!: HTMLDivElement
  let content!: HTMLDivElement

  const selected = () => {
    const id = store.state.selectedId
    return id === null ? undefined : store.state.loaded[id]
  }
  const running = () => isRunActive(store.state.run)
  const label = (id: string) => modelLabel(store.state.models, id)
  const mode = () => selected()?.session.mode ?? store.preferredMode()

  const items = createMemo(() => {
    const id = store.state.selectedId
    if (id === null) return []
    return buildTranscript({
      sessionId: id,
      loaded: store.state.loaded,
      journal: store.journal(),
      pending: store.pendingChanges(),
      run: store.state.run,
    })
  })

  const block = createMemo((): ComposerBlock => {
    if (store.connectionState().kind !== "open") return { kind: "disconnected" }
    const activeId = store.state.engineActiveRun
    if (activeId === null || running()) return { kind: "ready" }
    return { kind: "busy_elsewhere", sessionId: activeId, session: store.state.sessions.find((session) => session.id === activeId) }
  })

  createEffect(() => {
    if (block().kind !== "busy_elsewhere") return
    const timer = setInterval(() => store.refreshStatus().catch(reportFailure), BUSY_STATUS_POLL_MILLISECONDS)
    onCleanup(() => clearInterval(timer))
  })

  const actions: TranscriptActions = {
    modelLabel: label,
    openFile: (path, line) => props.workspace.openFile(path, line).catch(reportFailure),
    editAndResend(seq, text) {
      const session = selected()?.session
      if (!session) return
      const branch = seq > 1 ? store.fork(session.id, seq - 1) : store.createSession(session.model, session.mode)
      branch.then(() => setDraft(text), reportFailure)
    },
    forkThrough(seq) {
      const id = store.state.selectedId
      if (id !== null) store.fork(id, seq).catch(reportFailure)
    },
    revert: store.revertChange,
    approve: store.approveChange,
    decline: store.declineChange,
    continueRun: () => store.sendMessage("Continue.").catch(reportFailure),
  }

  function scrollToLatest(): void {
    scroller.scrollTop = scroller.scrollHeight
    setFollowing(true)
  }

  onMount(() => {
    const observer = new ResizeObserver(() => {
      if (following()) scroller.scrollTop = scroller.scrollHeight
    })
    observer.observe(content)
    onCleanup(() => observer.disconnect())
  })

  createEffect(on(() => store.state.selectedId, scrollToLatest))

  function changeMode(next: Mode): void {
    if (!selected()) {
      store.setPreferredMode(next)
      return
    }
    store.changeMode(next).catch(reportFailure)
  }

  return (
    <section class="agent-panel" aria-label="Agent">
      <header class="agent-header">
        <Show when={selected()?.session} keyed fallback={<span class="agent-title agent-title-new">New session</span>}>
          {(session) => (
            <SessionTitle title={session.title || "Untitled"} onRename={(title) => store.rename(session.id, title).catch(reportFailure)} />
          )}
        </Show>
        <div class="mode-switch" role="radiogroup" aria-label="Mode">
          <For each={modeOrder}>
            {(option) => (
              <button
                class="mode-switch-option"
                type="button"
                role="radio"
                aria-checked={mode() === option}
                disabled={running()}
                title={running() ? "Wait for the run to finish" : undefined}
                onClick={() => changeMode(option)}
              >
                {modeLabels[option]}
              </button>
            )}
          </For>
        </div>
        <button
          class="icon-button"
          type="button"
          aria-label="New session"
          title="New session"
          onClick={() => {
            store.clearSelection()
            setDraft("")
          }}
        >
          <Icon name="plus" />
        </button>
      </header>

      <div class="transcript-region">
        <div
          ref={scroller}
          class="transcript"
          role="log"
          aria-label="Transcript"
          onScroll={() => {
            const distance = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight
            setFollowing(distance <= BOTTOM_TOLERANCE_PIXELS)
          }}
        >
          <div ref={content} class="transcript-content">
            <Show
              when={items().length > 0}
              fallback={
                <div class="transcript-empty">
                  <p>The agent can read, search, and edit the files in {props.workspace.name}.</p>
                  <p>Ask a question or describe a change, and type @ to attach a file.</p>
                </div>
              }
            >
              <TranscriptActionsContext.Provider value={actions}>
                <TranscriptList items={items()} nested={false} />
              </TranscriptActionsContext.Provider>
            </Show>
          </div>
        </div>
        <Show when={!following()}>
          <button class="jump-to-latest button button-small" type="button" onClick={scrollToLatest}>
            <Icon name="arrowDown" />
            Jump to latest
          </button>
        </Show>
      </div>

      <Composer
        workspace={props.workspace}
        draft={draft()}
        onDraftChange={setDraft}
        block={block()}
        running={running()}
        onSend={store.sendMessage}
        onStop={() => {
          try {
            store.cancel()
          } catch (error) {
            reportFailure(error)
          }
        }}
        onSelectSession={(id) => store.selectSession(id).catch(reportFailure)}
      />
    </section>
  )
}

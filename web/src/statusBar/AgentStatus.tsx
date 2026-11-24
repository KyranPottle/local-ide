import { createSignal, onCleanup, Show } from "solid-js"
import { isRunActive, modelLabel } from "../agent/sessionState"
import type { SessionStore } from "../agent/sessionStore"
import type { ConnectionState } from "../engine/engineConnection"

const CLOCK_TICK_MILLISECONDS = 1000

function connectionText(state: ConnectionState, now: number): string {
  switch (state.kind) {
    case "open":
      return "Connected"
    case "connecting":
      return "Connecting"
    case "closed":
      return `Reconnecting in ${Math.max(Math.ceil((state.retryAt - now) / 1000), 0)} s`
    default: {
      const unhandled: never = state
      return unhandled
    }
  }
}

export function AgentStatus(props: { store: SessionStore }) {
  const store = props.store
  const [now, setNow] = createSignal(Date.now())
  const clock = setInterval(() => setNow(Date.now()), CLOCK_TICK_MILLISECONDS)
  onCleanup(() => clearInterval(clock))

  const runText = () => {
    const { run, loaded, models } = store.state
    if (!isRunActive(run)) return "Idle"
    const streamingId = run.kind === "running" ? run.streamingSessionId : run.sessionId
    const model = loaded[streamingId]?.session.model
    return model === undefined ? "Running" : `Running ${modelLabel(models, model)}`
  }

  const tokensPerSecond = () => {
    const id = store.state.selectedId
    const events = id === null ? [] : store.state.loaded[id]?.events ?? []
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const body = events[index]?.body
      if (body?.type === "assistant" && body.usage?.tokens_per_second != null) return body.usage.tokens_per_second
    }
    return undefined
  }

  return (
    <>
      <span class="status-item">{connectionText(store.connectionState(), now())}</span>
      <span class="status-item">{runText()}</span>
      <Show when={tokensPerSecond()}>
        {(rate) => <span class="status-item status-item-numeric">{rate().toFixed(1)} tok/s</span>}
      </Show>
    </>
  )
}

import type {
  ContextReport,
  Event,
  ModelInfo,
  RejectReason,
  RunOutcome,
  ServerFrame,
  Session,
  SessionDetail,
} from "../protocol"

export type StreamingBuffers = { readonly text: string, readonly reasoning: string }

export type LoadedSession = {
  readonly session: Session
  readonly events: readonly Event[]
  readonly context: ContextReport | null
  readonly streaming: StreamingBuffers
  // Child session id for each delegate tool call id
  readonly children: Readonly<Record<string, string>>
}

export type RunState =
  | { readonly kind: "idle" }
  | { readonly kind: "starting", readonly sessionId: string }
  | { readonly kind: "running", readonly sessionId: string, readonly streamingSessionId: string }
  | { readonly kind: "finished", readonly sessionId: string, readonly outcome: RunOutcome }
  | { readonly kind: "rejected", readonly reason: RejectReason }

export type SessionState = {
  readonly models: readonly ModelInfo[]
  readonly sessions: readonly Session[]
  readonly searchText: string
  readonly selectedId: string | null
  readonly loaded: Readonly<Record<string, LoadedSession>>
  readonly run: RunState
  // Session of the run that the engine reports, which can belong to another page
  readonly engineActiveRun: string | null
}

export const initialSessionState: SessionState = {
  models: [],
  sessions: [],
  searchText: "",
  selectedId: null,
  loaded: {},
  run: { kind: "idle" },
  engineActiveRun: null,
}

const emptyStreaming: StreamingBuffers = { text: "", reasoning: "" }

export function modelLabel(models: readonly ModelInfo[], id: string): string {
  return models.find(({ profile }) => profile.id === id)?.profile.label ?? id
}

export function isRunActive(run: RunState): run is Extract<RunState, { kind: "starting" | "running" }> {
  return run.kind === "starting" || run.kind === "running"
}

function delegateChildren(sessions: readonly Session[], parentId: string): Record<string, string> {
  const children: Record<string, string> = {}
  for (const session of sessions) {
    if (session.origin.kind === "delegate" && session.origin.parent === parentId) {
      children[session.origin.call_id] = session.id
    }
  }
  return children
}

function updateLoaded(
  state: SessionState,
  id: string,
  update: (loaded: LoadedSession) => LoadedSession,
): SessionState {
  const loaded = state.loaded[id]
  if (!loaded) return state
  return { ...state, loaded: { ...state.loaded, [id]: update(loaded) } }
}

function upsertSession(sessions: readonly Session[], session: Session, insert: boolean): readonly Session[] {
  const index = sessions.findIndex((entry) => entry.id === session.id)
  if (index === -1 && !insert) return sessions
  const others = sessions.filter((entry) => entry.id !== session.id)
  return [session, ...others].sort((a, b) => b.updated_at - a.updated_at)
}

function appendEvent(loaded: LoadedSession, event: Event): LoadedSession {
  const lastSeq = loaded.events.at(-1)?.seq ?? 0
  if (event.seq <= lastSeq) return loaded
  const streaming = event.body.type === "assistant" ? emptyStreaming : loaded.streaming
  return { ...loaded, events: [...loaded.events, event], streaming }
}

export function withSessionChanged(state: SessionState, session: Session): SessionState {
  const sessions = upsertSession(state.sessions, session, state.searchText === "")
  let next = updateLoaded({ ...state, sessions }, session.id, (loaded) => ({ ...loaded, session }))
  if (session.origin.kind !== "delegate") return next
  const { parent, call_id: callId } = session.origin
  if (!next.loaded[session.id]) {
    const child: LoadedSession = { session, events: [], context: null, streaming: emptyStreaming, children: {} }
    next = { ...next, loaded: { ...next.loaded, [session.id]: child } }
  }
  return updateLoaded(next, parent, (loaded) => ({ ...loaded, children: { ...loaded.children, [callId]: session.id } }))
}

function finishRun(state: SessionState, sessionId: string, outcome: RunOutcome): SessionState {
  const cleared = updateLoaded(state, sessionId, (loaded) => ({ ...loaded, streaming: emptyStreaming }))
  const { run } = cleared
  if (!isRunActive(run)) return cleared
  if (run.sessionId !== sessionId) {
    return { ...cleared, run: { kind: "running", sessionId: run.sessionId, streamingSessionId: run.sessionId } }
  }
  return { ...cleared, run: { kind: "finished", sessionId, outcome }, engineActiveRun: null }
}

export function reduceServerFrame(state: SessionState, frame: ServerFrame): SessionState {
  switch (frame.type) {
    case "run_started": {
      const { run } = state
      // The engine runs one agent at a time, so a start inside a running run is a delegate child
      if (run.kind === "running" && run.sessionId !== frame.session_id) {
        return { ...state, run: { ...run, streamingSessionId: frame.session_id } }
      }
      return {
        ...state,
        run: { kind: "running", sessionId: frame.session_id, streamingSessionId: frame.session_id },
        engineActiveRun: frame.session_id,
      }
    }
    case "delta": {
      const { run } = state
      const next = run.kind === "running" ? { ...state, run: { ...run, streamingSessionId: frame.session_id } } : state
      return updateLoaded(next, frame.session_id, (loaded) => ({
        ...loaded,
        streaming: { ...loaded.streaming, [frame.channel]: loaded.streaming[frame.channel] + frame.text },
      }))
    }
    case "event":
      return updateLoaded(state, frame.session_id, (loaded) => appendEvent(loaded, frame.event))
    case "context_report":
      return updateLoaded(state, frame.session_id, (loaded) => ({ ...loaded, context: frame.report }))
    case "session_changed":
      return withSessionChanged(state, frame.session)
    case "run_finished":
      return finishRun(state, frame.session_id, frame.outcome)
    case "rejected": {
      const engineActiveRun = frame.reason.kind === "busy" ? frame.reason.session_id : state.engineActiveRun
      return { ...state, run: { kind: "rejected", reason: frame.reason }, engineActiveRun }
    }
    case "tool_call":
      return state
    default: {
      const unhandled: never = frame
      return unhandled
    }
  }
}

export function withSessionDetail(state: SessionState, detail: SessionDetail): SessionState {
  const id = detail.session.id
  const children = { ...delegateChildren(state.sessions, id), ...state.loaded[id]?.children }
  const loaded: LoadedSession = {
    session: detail.session,
    events: detail.events,
    context: detail.context,
    streaming: state.loaded[id]?.streaming ?? emptyStreaming,
    children,
  }
  return { ...state, loaded: { ...state.loaded, [id]: loaded } }
}

export function withoutSession(state: SessionState, id: string): SessionState {
  const loaded = Object.fromEntries(Object.entries(state.loaded).filter(([key]) => key !== id))
  return {
    ...state,
    sessions: state.sessions.filter((session) => session.id !== id),
    loaded,
    selectedId: state.selectedId === id ? null : state.selectedId,
  }
}

export function withConnectionLost(state: SessionState): SessionState {
  const { run } = state
  if (!isRunActive(run)) return state
  return finishRun(state, run.sessionId, { kind: "failed", message: "connection lost" })
}

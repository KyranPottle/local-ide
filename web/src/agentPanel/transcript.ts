import type { Attachment, Event, JsonValue, RunOutcome, ToolCall, Usage } from "../protocol"
import { isRunActive, type LoadedSession, type RunState } from "../agent/sessionState"
import type { FileChange } from "../agent/toolHost"

export type ToolState =
  | { readonly kind: "running" }
  | { readonly kind: "done", readonly output: string }
  | { readonly kind: "error", readonly output: string }
  // The run ended before the tool answered
  | { readonly kind: "stopped" }

export type ChangeReview =
  | { readonly kind: "none" }
  | { readonly kind: "pending", readonly change: FileChange }
  | { readonly kind: "applied", readonly change: FileChange }

export type ChildTranscript = {
  readonly sessionId: string
  readonly model: string
  readonly running: boolean
  readonly items: readonly TranscriptItem[]
}

export type RunEnding = Exclude<RunOutcome, { kind: "complete" }>

export type TranscriptItem =
  | { readonly kind: "user", readonly seq: number, readonly text: string, readonly attachments: readonly string[] }
  | {
    readonly kind: "assistant"
    readonly seq: number
    // Last event of the turn, so that a fork keeps the tool results of this reply
    readonly forkSeq: number
    readonly text: string
    readonly reasoning: string
    readonly model: string
    readonly usage: Usage | null
  }
  | { readonly kind: "reply_in_progress", readonly text: string, readonly reasoning: string }
  | { readonly kind: "tool", readonly call: ToolCall, readonly argument: string, readonly path: string | null, readonly state: ToolState }
  | { readonly kind: "file_change", readonly call: ToolCall, readonly path: string, readonly state: ToolState, readonly review: ChangeReview }
  | { readonly kind: "delegate", readonly call: ToolCall, readonly task: string, readonly state: ToolState, readonly child: ChildTranscript | null }
  | { readonly kind: "model_switch", readonly from: string, readonly to: string }
  | { readonly kind: "results_elided" }
  | { readonly kind: "summary", readonly text: string }
  | { readonly kind: "run_ending", readonly ending: RunEnding }

export type TranscriptSource = {
  readonly sessionId: string
  readonly loaded: Readonly<Record<string, LoadedSession>>
  readonly journal: readonly FileChange[]
  readonly pending: readonly FileChange[]
  readonly run: RunState
}

const PATH_ARGUMENT_TOOLS = new Set(["read", "edit", "write"])

function stringArgument(call: ToolCall, key: string): string | null {
  const { arguments: value } = call
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null
  const argument: JsonValue | undefined = value[key]
  return typeof argument === "string" ? argument : null
}

export function attachmentLabel({ path, lines }: Attachment): string {
  return lines === null ? path : `${path}:${lines.first}-${lines.last}`
}

function toolState(result: Event["body"] | undefined, active: boolean): ToolState {
  if (result?.type === "tool_result") {
    return result.is_error ? { kind: "error", output: result.output } : { kind: "done", output: result.output }
  }
  return active ? { kind: "running" } : { kind: "stopped" }
}

function changeReview(source: TranscriptSource, callId: string): ChangeReview {
  const pending = source.pending.find((change) => change.callId === callId)
  if (pending) return { kind: "pending", change: pending }
  const applied = source.journal.find((change) => change.callId === callId)
  return applied ? { kind: "applied", change: applied } : { kind: "none" }
}

function callItem(
  source: TranscriptSource,
  session: LoadedSession,
  call: ToolCall,
  state: ToolState,
): TranscriptItem {
  if (call.name === "edit" || call.name === "write") {
    const path = stringArgument(call, "path") ?? ""
    return { kind: "file_change", call, path, state, review: changeReview(source, call.id) }
  }
  if (call.name === "delegate") {
    const childId = session.children[call.id]
    const childSession = childId === undefined ? undefined : source.loaded[childId]
    const running = state.kind === "running"
    const child = childSession && {
      sessionId: childSession.session.id,
      model: childSession.session.model,
      running,
      items: sessionItems(source, childSession, running),
    }
    return { kind: "delegate", call, task: stringArgument(call, "task") ?? "", state, child: child ?? null }
  }
  const path = PATH_ARGUMENT_TOOLS.has(call.name) ? stringArgument(call, "path") : null
  const argument = path ?? stringArgument(call, "pattern") ?? ""
  return { kind: "tool", call, argument, path, state }
}

function isStreamingSession(run: RunState, sessionId: string): boolean {
  if (run.kind === "starting") return run.sessionId === sessionId
  return run.kind === "running" && run.streamingSessionId === sessionId
}

function sessionItems(source: TranscriptSource, session: LoadedSession, active: boolean): TranscriptItem[] {
  const results = new Map<string, Event>()
  for (const event of session.events) {
    if (event.body.type === "tool_result") results.set(event.body.call_id, event)
  }
  const items: TranscriptItem[] = []
  for (const { seq, body } of session.events) {
    switch (body.type) {
      case "user":
        items.push({ kind: "user", seq, text: body.text, attachments: body.attachments.map(attachmentLabel) })
        break
      case "assistant": {
        const callResults = body.tool_calls.map((call) => results.get(call.id))
        const forkSeq = Math.max(seq, ...callResults.map((result) => result?.seq ?? seq))
        const { text, reasoning, model, usage } = body
        items.push({ kind: "assistant", seq, forkSeq, text, reasoning, model, usage })
        body.tool_calls.forEach((call, index) => {
          items.push(callItem(source, session, call, toolState(callResults[index]?.body, active)))
        })
        break
      }
      case "tool_result":
        break
      case "compaction":
        items.push(body.kind.kind === "summary" ? { kind: "summary", text: body.kind.text } : { kind: "results_elided" })
        break
      case "model_switch":
        items.push({ kind: "model_switch", from: body.from, to: body.to })
        break
      default: {
        const unhandled: never = body
        return unhandled
      }
    }
  }
  const toolRunning = items.some((item) => "state" in item && item.state.kind === "running")
  if (active && isStreamingSession(source.run, session.session.id) && !toolRunning) {
    items.push({ kind: "reply_in_progress", ...session.streaming })
  }
  return items
}

export function buildTranscript(source: TranscriptSource): TranscriptItem[] {
  const session = source.loaded[source.sessionId]
  if (!session) return []
  const { run } = source
  const active = isRunActive(run) && run.sessionId === source.sessionId
  const items = sessionItems(source, session, active)
  if (run.kind === "finished" && run.sessionId === source.sessionId && run.outcome.kind !== "complete") {
    items.push({ kind: "run_ending", ending: run.outcome })
  }
  return items
}

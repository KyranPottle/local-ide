import { describe, expect, test } from "vitest"
import type { Event, EventBody, Session } from "../protocol"
import type { LoadedSession, RunState } from "../agent/sessionState"
import type { FileChange } from "../agent/toolHost"
import { buildTranscript, type TranscriptSource } from "./transcript"

const root: Session = {
  id: "root",
  title: "Fix the sum",
  workspace: "scratch",
  model: "coder",
  mode: "agent",
  origin: { kind: "root" },
  created_at: 1000,
  updated_at: 1000,
}

const childSession: Session = { ...root, id: "child", model: "fast", origin: { kind: "delegate", parent: "root", call_id: "call-d" } }

const events = (...bodies: EventBody[]): Event[] => bodies.map((body, index) => ({ seq: index + 1, at: 1000 + index, body }))

const user = (text: string): EventBody => ({
  type: "user",
  text,
  attachments: [],
  workspace: null,
  system_rules: null,
  repo_rules: null,
})

function loaded(session: Session, eventList: Event[], streaming = { text: "", reasoning: "" }, children = {}): LoadedSession {
  return { session, events: eventList, context: null, streaming, children }
}

function source(sessions: LoadedSession[], run: RunState, changes: Partial<TranscriptSource> = {}): TranscriptSource {
  return {
    sessionId: "root",
    loaded: Object.fromEntries(sessions.map((entry) => [entry.session.id, entry])),
    journal: [],
    pending: [],
    run,
    ...changes,
  }
}

const editChange: FileChange = { callId: "call-e", path: "src/example.ts", earlier: { kind: "text", text: "a\n" }, content: "b\n" }

describe("buildTranscript", () => {
  test("a full run with a read, an edit, and an answer", () => {
    const run = events(
      {
        type: "user",
        text: "Fix it",
        attachments: [{ path: "README.md", lines: { first: 2, last: 4 }, content: "x" }],
        workspace: null,
        system_rules: null,
        repo_rules: null,
      },
      {
        type: "assistant",
        text: "",
        reasoning: "",
        model: "coder",
        usage: null,
        tool_calls: [
          { id: "call-r", name: "read", arguments: { path: "README.md" } },
          { id: "call-e", name: "edit", arguments: { path: "src/example.ts", old: "a", new: "b" } },
        ],
      },
      { type: "tool_result", call_id: "call-r", name: "read", output: "1|# Scratch", is_error: false },
      { type: "tool_result", call_id: "call-e", name: "edit", output: "edited src/example.ts: 1 replacement(s)", is_error: false },
      {
        type: "assistant",
        text: "Done.",
        reasoning: "",
        model: "coder",
        usage: { prompt_tokens: 900, completion_tokens: 12, tokens_per_second: 40 },
        tool_calls: [],
      },
    )
    const items = buildTranscript(source([loaded(root, run)], { kind: "finished", sessionId: "root", outcome: { kind: "complete" } }, { journal: [editChange] }))
    expect(items).toEqual([
      { kind: "user", seq: 1, text: "Fix it", attachments: ["README.md:2-4"] },
      { kind: "assistant", seq: 2, forkSeq: 4, text: "", reasoning: "", model: "coder", usage: null },
      {
        kind: "tool",
        call: { id: "call-r", name: "read", arguments: { path: "README.md" } },
        argument: "README.md",
        path: "README.md",
        state: { kind: "done", output: "1|# Scratch" },
      },
      {
        kind: "file_change",
        call: { id: "call-e", name: "edit", arguments: { path: "src/example.ts", old: "a", new: "b" } },
        path: "src/example.ts",
        state: { kind: "done", output: "edited src/example.ts: 1 replacement(s)" },
        review: { kind: "applied", change: editChange },
      },
      {
        kind: "assistant",
        seq: 5,
        forkSeq: 5,
        text: "Done.",
        reasoning: "",
        model: "coder",
        usage: { prompt_tokens: 900, completion_tokens: 12, tokens_per_second: 40 },
      },
    ])
  })

  test("a delegate child nests under its call while it runs", () => {
    const delegateCall = { id: "call-d", name: "delegate", arguments: { task: "Find the tests" } }
    const parent = loaded(
      root,
      events(user("Plan"), { type: "assistant", text: "", reasoning: "", model: "coder", usage: null, tool_calls: [delegateCall] }),
      { text: "", reasoning: "" },
      { "call-d": "child" },
    )
    const child = loaded(childSession, events(user("Find the tests")), { text: "Looking", reasoning: "" })
    const items = buildTranscript(source([parent, child], { kind: "running", sessionId: "root", streamingSessionId: "child" }))
    expect(items.at(-1)).toEqual({
      kind: "delegate",
      call: delegateCall,
      task: "Find the tests",
      state: { kind: "running" },
      child: {
        sessionId: "child",
        model: "fast",
        running: true,
        items: [
          { kind: "user", seq: 1, text: "Find the tests", attachments: [] },
          { kind: "reply_in_progress", text: "Looking", reasoning: "" },
        ],
      },
    })
    expect(items).toHaveLength(3)
  })

  test("a pending change shows its review and a running state", () => {
    const call = { id: "call-e", name: "write", arguments: { path: "notes.md", content: "b\n" } }
    const session = loaded(root, events(user("Write"), { type: "assistant", text: "", reasoning: "", model: "coder", usage: null, tool_calls: [call] }))
    const items = buildTranscript(source([session], { kind: "running", sessionId: "root", streamingSessionId: "root" }, { pending: [editChange] }))
    expect(items.at(-1)).toEqual({
      kind: "file_change",
      call,
      path: "notes.md",
      state: { kind: "running" },
      review: { kind: "pending", change: editChange },
    })
  })

  test("a cancelled run keeps the partial text and ends with the outcome", () => {
    const session = loaded(
      root,
      events(user("Explain"), { type: "assistant", text: "The sum", reasoning: "", model: "coder", usage: null, tool_calls: [] }),
    )
    const items = buildTranscript(source([session], { kind: "finished", sessionId: "root", outcome: { kind: "cancelled" } }))
    expect(items.slice(1)).toEqual([
      { kind: "assistant", seq: 2, forkSeq: 2, text: "The sum", reasoning: "", model: "coder", usage: null },
      { kind: "run_ending", ending: { kind: "cancelled" } },
    ])
  })

  test("compaction and model switch events become markers", () => {
    const session = loaded(
      root,
      events(
        user("One"),
        { type: "compaction", through: 1, kind: { kind: "elide" } },
        { type: "compaction", through: 2, kind: { kind: "summary", text: "The user asked one thing.", model: "coder" } },
        { type: "model_switch", from: "coder", to: "fast" },
      ),
    )
    const items = buildTranscript(source([session], { kind: "idle" }))
    expect(items.slice(1)).toEqual([
      { kind: "results_elided" },
      { kind: "summary", text: "The user asked one thing." },
      { kind: "model_switch", from: "coder", to: "fast" },
    ])
  })

  test("a streaming reply follows the stored events", () => {
    const session = loaded(root, events(user("Hi")), { text: "Hello", reasoning: "The user greets" })
    const items = buildTranscript(source([session], { kind: "running", sessionId: "root", streamingSessionId: "root" }))
    expect(items).toEqual([
      { kind: "user", seq: 1, text: "Hi", attachments: [] },
      { kind: "reply_in_progress", text: "Hello", reasoning: "The user greets" },
    ])
  })

  test("an unanswered call after the run ends is stopped", () => {
    const call = { id: "call-g", name: "glob", arguments: { pattern: "**/*.ts" } }
    const session = loaded(root, events(user("List"), { type: "assistant", text: "", reasoning: "", model: "coder", usage: null, tool_calls: [call] }))
    const items = buildTranscript(source([session], { kind: "idle" }))
    expect(items.at(-1)).toEqual({ kind: "tool", call, argument: "**/*.ts", path: null, state: { kind: "stopped" } })
  })
})

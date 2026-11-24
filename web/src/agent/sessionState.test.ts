import { describe, expect, test } from "vitest"
import type { ContextReport, Event, ServerFrame, Session, SessionDetail } from "../protocol"
import {
  initialSessionState,
  reduceServerFrame,
  withConnectionLost,
  withSessionDetail,
  type SessionState,
} from "./sessionState"

const report: ContextReport = { budget: 24000, reserve: 2048, system: 300, tools: 200, history: 100, total: 600 }

const root: Session = {
  id: "root",
  title: "Fix the parser",
  workspace: "project",
  model: "coder",
  mode: "agent",
  origin: { kind: "root" },
  created_at: 1000,
  updated_at: 1000,
}

const child: Session = {
  ...root,
  id: "child",
  title: "Find the tests",
  origin: { kind: "delegate", parent: "root", call_id: "call-9" },
  created_at: 2000,
  updated_at: 2000,
}

const userEvent: Event = {
  seq: 1,
  at: 1000,
  body: { type: "user", text: "fix it", attachments: [], workspace: null, system_rules: null, repo_rules: null },
}

const assistantEvent: Event = {
  seq: 2,
  at: 1100,
  body: { type: "assistant", text: "Done.", reasoning: "think", tool_calls: [], model: "coder", usage: null },
}

const detail: SessionDetail = { session: root, events: [userEvent], context: report }

const reduce = (state: SessionState, frames: ServerFrame[]) => frames.reduce(reduceServerFrame, state)

const loadedRoot = withSessionDetail({ ...initialSessionState, sessions: [root], selectedId: "root" }, detail)

describe("reduceServerFrame", () => {
  test("a full run sequence", () => {
    const afterStart = reduce(loadedRoot, [{ type: "run_started", session_id: "root" }])
    expect(afterStart.run).toEqual({ kind: "running", sessionId: "root", streamingSessionId: "root" })
    expect(afterStart.engineActiveRun).toBe("root")

    const streaming = reduce(afterStart, [
      { type: "context_report", session_id: "root", model: "coder", report: { ...report, history: 150, total: 650 } },
      { type: "delta", session_id: "root", channel: "reasoning", text: "thi" },
      { type: "delta", session_id: "root", channel: "reasoning", text: "nk" },
      { type: "delta", session_id: "root", channel: "text", text: "Do" },
      { type: "delta", session_id: "root", channel: "text", text: "ne." },
    ])
    expect(streaming.loaded.root?.streaming).toEqual({ text: "Done.", reasoning: "think" })
    expect(streaming.loaded.root?.context?.total).toBe(650)

    const finished = reduce(streaming, [
      { type: "event", session_id: "root", event: assistantEvent },
      { type: "session_changed", session: { ...root, title: "Parser fix", updated_at: 3000 } },
      { type: "run_finished", session_id: "root", outcome: { kind: "complete" } },
    ])
    expect(finished.loaded.root?.events).toEqual([userEvent, assistantEvent])
    expect(finished.loaded.root?.streaming).toEqual({ text: "", reasoning: "" })
    expect(finished.loaded.root?.session.title).toBe("Parser fix")
    expect(finished.sessions.map((session) => session.title)).toEqual(["Parser fix"])
    expect(finished.run).toEqual({ kind: "finished", sessionId: "root", outcome: { kind: "complete" } })
    expect(finished.engineActiveRun).toBe(null)
  })

  test("a delegate child nests under its call", () => {
    const childEvent: Event = { ...userEvent, body: { ...userEvent.body, text: "find tests" } as Event["body"] }
    const state = reduce(loadedRoot, [
      { type: "run_started", session_id: "root" },
      { type: "session_changed", session: child },
      { type: "delta", session_id: "child", channel: "text", text: "Look" },
    ])
    expect(state.loaded.root?.children).toEqual({ "call-9": "child" })
    expect(state.loaded.child?.streaming.text).toBe("Look")
    expect(state.run).toEqual({ kind: "running", sessionId: "root", streamingSessionId: "child" })

    const afterChild = reduce(state, [
      { type: "event", session_id: "child", event: childEvent },
      { type: "run_finished", session_id: "child", outcome: { kind: "complete" } },
    ])
    expect(afterChild.loaded.child?.events).toEqual([childEvent])
    expect(afterChild.loaded.child?.streaming.text).toBe("")
    expect(afterChild.run).toEqual({ kind: "running", sessionId: "root", streamingSessionId: "root" })
    expect(afterChild.loaded.root?.events).toEqual([userEvent])
  })

  test("the run_started frame of a delegate child keeps the parent run active", () => {
    const state = reduce(loadedRoot, [
      { type: "run_started", session_id: "root" },
      { type: "session_changed", session: child },
      { type: "run_started", session_id: "child" },
      { type: "run_finished", session_id: "child", outcome: { kind: "complete" } },
    ])
    expect(state.run).toEqual({ kind: "running", sessionId: "root", streamingSessionId: "root" })
    expect(state.engineActiveRun).toBe("root")

    const finished = reduce(state, [{ type: "run_finished", session_id: "root", outcome: { kind: "complete" } }])
    expect(finished.run).toEqual({ kind: "finished", sessionId: "root", outcome: { kind: "complete" } })
  })

  test("a cancel", () => {
    const state = reduce(loadedRoot, [
      { type: "run_started", session_id: "root" },
      { type: "delta", session_id: "root", channel: "reasoning", text: "hmm" },
      { type: "run_finished", session_id: "root", outcome: { kind: "cancelled" } },
    ])
    expect(state.run).toEqual({ kind: "finished", sessionId: "root", outcome: { kind: "cancelled" } })
    expect(state.loaded.root?.streaming).toEqual({ text: "", reasoning: "" })
  })

  test("a rejection", () => {
    const state = reduce({ ...loadedRoot, run: { kind: "starting", sessionId: "root" } }, [
      { type: "rejected", reason: { kind: "busy", session_id: "other" } },
    ])
    expect(state.run).toEqual({ kind: "rejected", reason: { kind: "busy", session_id: "other" } })
    expect(state.engineActiveRun).toBe("other")
  })

  test("duplicate and out-of-order events", () => {
    const later: Event = { ...assistantEvent, seq: 3 }
    const state = reduce(loadedRoot, [
      { type: "event", session_id: "root", event: userEvent },
      { type: "event", session_id: "root", event: later },
      { type: "event", session_id: "root", event: assistantEvent },
      { type: "event", session_id: "root", event: later },
    ])
    expect(state.loaded.root?.events.map((event) => event.seq)).toEqual([1, 3])
  })

  test("frames for a session that is not loaded", () => {
    const state = reduce(loadedRoot, [
      { type: "delta", session_id: "unknown", channel: "text", text: "x" },
      { type: "event", session_id: "unknown", event: userEvent },
      { type: "context_report", session_id: "unknown", model: "coder", report },
    ])
    expect(state).toEqual(loadedRoot)
  })

  test("a session change during a search only updates listed sessions", () => {
    const state = reduce({ ...initialSessionState, searchText: "parser" }, [{ type: "session_changed", session: root }])
    expect(state.sessions).toEqual([])
  })
})

test("a connection loss ends the run", () => {
  const state = withConnectionLost(reduce(loadedRoot, [{ type: "run_started", session_id: "root" }]))
  expect(state.run).toEqual({ kind: "finished", sessionId: "root", outcome: { kind: "failed", message: "connection lost" } })
})

test("a detail load registers listed delegate children", () => {
  const state = withSessionDetail({ ...initialSessionState, sessions: [child, root] }, detail)
  expect(state.loaded.root?.children).toEqual({ "call-9": "child" })
})

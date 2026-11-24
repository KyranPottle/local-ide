import { createRoot } from "solid-js"
import { describe, expect, test } from "vitest"
import type { ClientFrame, ContextReport, Session, SessionDetail } from "../protocol"
import type { EngineApi } from "../engine/engineApi"
import type { EngineConnection } from "../engine/engineConnection"
import { memoryDirectoryHandle } from "../workspace/memoryDirectoryHandle"
import { createWorkspaceStore } from "../workspace/workspaceStore"
import { createSessionStore } from "./sessionStore"

const report: ContextReport = { budget: 24000, reserve: 2048, system: 1, tools: 1, history: 1, total: 3 }

const session: Session = {
  id: "s1",
  title: "Notes",
  workspace: "project",
  model: "coder",
  mode: "agent",
  origin: { kind: "root" },
  created_at: 1,
  updated_at: 1,
}

const detail: SessionDetail = { session, events: [], context: report }

function api(): EngineApi {
  return {
    fetchStatus: async () => ({ version: "test", active_run: null }),
    fetchModels: async () => [],
    fetchSessions: async () => [],
    createSession: async () => session,
    fetchSessionDetail: async () => detail,
    updateSession: async () => session,
    deleteSession: async () => undefined,
    forkSession: async () => session,
  }
}

async function sentFrame(content: Record<string, string>, text: string): Promise<ClientFrame> {
  return createRoot(async (dispose) => {
    const sent: ClientFrame[] = []
    const connection: EngineConnection = {
      state: () => ({ kind: "open" }),
      listen: () => () => undefined,
      send: (frame) => {
        sent.push(frame)
        return { kind: "sent" }
      },
    }
    const workspace = createWorkspaceStore()
    workspace.open(memoryDirectoryHandle("project", content))
    const store = createSessionStore({ connection, workspace: workspace.workspace, api: api() })
    await store.selectSession("s1")
    await store.sendMessage(text)
    dispose()
    const frame = sent[0]
    if (frame === undefined) throw new Error("no frame was sent")
    return frame
  })
}

describe("sendMessage", () => {
  test("sends the trimmed text of AGENTS.md", async () => {
    const frame = await sentFrame({ "AGENTS.md": "\nUse pnpm.\n", "src/a.ts": "export {}\n" }, "Fix the parser")
    expect(frame).toMatchObject({ type: "run_start", text: "Fix the parser", repo_rules: "Use pnpm." })
  })

  test("sends null when AGENTS.md is absent", async () => {
    const frame = await sentFrame({ "README.md": "hello" }, "Again")
    expect(frame).toMatchObject({ type: "run_start", repo_rules: null })
  })
})

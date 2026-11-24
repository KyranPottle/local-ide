import { createSignal } from "solid-js"
import { createStore, reconcile, unwrap } from "solid-js/store"
import type { Attachment, ClientFrame, Mode, ModelInfo, ServerFrame, Session } from "../protocol"
import { engineApi, type EngineApi } from "../engine/engineApi"
import type { ConnectionState, EngineConnection } from "../engine/engineConnection"
import { readPreference, writePreference } from "../preferences/storedPreference"
import type { OpenWorkspace } from "../workspace/workspaceStore"
import {
  initialSessionState,
  isRunActive,
  reduceServerFrame,
  withConnectionLost,
  withoutSession,
  withSessionChanged,
  withSessionDetail,
  type SessionState,
} from "./sessionState"
import { readRepoRules } from "./repoRules"
import { WORKSPACE_TOOL_NAMES } from "./toolArguments"
import { createToolHost, type ChangeApproval } from "./toolHost"
import type { WorkspaceSearch } from "./workerSearch"

const MODEL_PREFERENCE_KEY = "local-ide.last-model"
const MODE_PREFERENCE_KEY = "local-ide.last-mode"
const APPROVAL_PREFERENCE_KEY = "local-ide.change-approval"

const modes: readonly Mode[] = ["agent", "ask"]
const approvals: readonly ChangeApproval[] = ["automatic", "confirm"]

function storedChoice<T extends string>(key: string, choices: readonly T[], fallback: T): T {
  const stored = readPreference(key)
  return choices.find((choice) => choice === stored) ?? fallback
}

export type SessionStoreOptions = {
  readonly connection: EngineConnection
  readonly workspace: () => OpenWorkspace | undefined
  readonly api?: EngineApi
  readonly search?: WorkspaceSearch
}

export function createSessionStore(options: SessionStoreOptions) {
  const { connection, workspace } = options
  const api = options.api ?? engineApi
  const [state, setState] = createStore<SessionState>(structuredClone(initialSessionState))
  const [approval, setApproval] = createSignal(storedChoice(APPROVAL_PREFERENCE_KEY, approvals, "automatic"))
  const [backgroundFailure, setBackgroundFailure] = createSignal<string>()
  const [storedModel, setStoredModel] = createSignal(readPreference(MODEL_PREFERENCE_KEY))
  const [preferredMode, setStoredMode] = createSignal(storedChoice(MODE_PREFERENCE_KEY, modes, "agent"))
  const toolHost = createToolHost({ workspace, approval, search: options.search })

  const current = () => unwrap(state)
  const update = (next: SessionState) => setState(reconcile(next))

  function openWorkspace(): OpenWorkspace {
    const open = workspace()
    if (!open) throw new Error("no workspace is open")
    return open
  }

  function selectedSessionId(): string {
    const id = current().selectedId
    if (id === null) throw new Error("no session is selected")
    return id
  }

  function refuseDuringRun(): void {
    if (isRunActive(current().run)) throw new Error("a run is active, wait for it to finish or cancel it")
  }

  // Reads the store proxy so that a caller in a reactive scope follows the model list
  function preferredModelInfo(): ModelInfo | undefined {
    const stored = storedModel()
    return state.models.find(({ profile }) => profile.id === stored) ?? state.models.find(({ available }) => available)
  }

  function preferredModel(): string {
    const model = preferredModelInfo()
    if (!model) throw new Error("no model is available")
    return model.profile.id
  }

  function rememberModel(model: string): void {
    writePreference(MODEL_PREFERENCE_KEY, model)
    setStoredModel(model)
  }

  function rememberMode(mode: Mode): void {
    writePreference(MODE_PREFERENCE_KEY, mode)
    setStoredMode(mode)
  }

  function sendFrame(frame: ClientFrame): void {
    if (connection.send(frame).kind === "not_connected") throw new Error("not connected to the engine")
  }

  async function refreshModels(): Promise<void> {
    const models = await api.fetchModels()
    update({ ...current(), models })
  }

  async function refreshStatus(): Promise<void> {
    const status = await api.fetchStatus()
    update({ ...current(), engineActiveRun: status.active_run })
  }

  async function search(searchText: string): Promise<void> {
    update({ ...current(), searchText })
    const sessions = await api.fetchSessions(searchText)
    // A later search can finish first
    if (current().searchText !== searchText) return
    update({ ...current(), sessions })
  }

  async function loadSession(id: string): Promise<void> {
    update(withSessionDetail(current(), await api.fetchSessionDetail(id)))
  }

  async function selectSession(id: string): Promise<void> {
    update({ ...current(), selectedId: id })
    await loadSession(id)
  }

  // The next message then starts a session with the preferred model and mode
  function clearSelection(): void {
    update({ ...current(), selectedId: null })
  }

  async function createSession(model: string, mode: Mode): Promise<Session> {
    const session = await api.createSession({ workspace: openWorkspace().name, model, mode })
    rememberModel(model)
    rememberMode(mode)
    update(withSessionChanged(current(), session))
    await selectSession(session.id)
    return session
  }

  async function changeSelectedSession(change: { model: string } | { mode: Mode }): Promise<void> {
    refuseDuringRun()
    const id = selectedSessionId()
    update(withSessionChanged(current(), await api.updateSession(id, change)))
    await loadSession(id)
  }

  async function backgroundRefresh(): Promise<void> {
    const selectedId = current().selectedId
    try {
      await Promise.all([
        refreshModels(),
        refreshStatus(),
        search(current().searchText),
        ...(selectedId === null ? [] : [loadSession(selectedId)]),
      ])
      setBackgroundFailure(undefined)
    } catch (error) {
      setBackgroundFailure(error instanceof Error ? error.message : String(error))
    }
  }

  async function answerToolCall(frame: Extract<ServerFrame, { type: "tool_call" }>): Promise<void> {
    // A result for a closed connection has no reader, the engine cancels that run
    connection.send(await toolHost.execute(frame.call))
  }

  connection.listen({
    frame(frame) {
      if (frame.type === "tool_call") {
        void answerToolCall(frame)
        return
      }
      update(reduceServerFrame(current(), frame))
    },
    state(connectionState: ConnectionState) {
      if (connectionState.kind === "open") {
        void backgroundRefresh()
        return
      }
      if (connectionState.kind === "closed") {
        update(withConnectionLost(current()))
        toolHost.declineAll()
      }
    },
  })

  return {
    state,
    connectionState: connection.state,
    backgroundFailure,
    approval,
    pendingChanges: toolHost.pendingChanges,
    journal: toolHost.journal,
    approveChange: toolHost.approve,
    declineChange: toolHost.decline,
    revertChange: toolHost.revert,
    refreshModels,
    refreshStatus,
    search,
    loadSession,
    selectSession,
    createSession,
    clearSelection,

    setApproval(value: ChangeApproval): void {
      setApproval(value)
      writePreference(APPROVAL_PREFERENCE_KEY, value)
    },

    preferredMode,
    preferredModelInfo,
    setPreferredModel: rememberModel,
    setPreferredMode: rememberMode,

    async sendMessage(text: string, attachments: readonly Attachment[] = []): Promise<void> {
      refuseDuringRun()
      const open = openWorkspace()
      if (connection.state().kind !== "open") throw new Error("not connected to the engine")
      const sessionId = current().selectedId
        ?? (await createSession(preferredModel(), preferredMode())).id
      const snapshot = await open.snapshot()
      sendFrame({
        type: "run_start",
        session_id: sessionId,
        text,
        attachments: [...attachments],
        workspace: snapshot,
        capabilities: WORKSPACE_TOOL_NAMES,
        repo_rules: await readRepoRules(open.fileSystem),
      })
      update({ ...current(), run: { kind: "starting", sessionId } })
    },

    cancel(): void {
      sendFrame({ type: "run_cancel" })
    },

    async rename(id: string, title: string): Promise<void> {
      update(withSessionChanged(current(), await api.updateSession(id, { title })))
    },

    async deleteSession(id: string): Promise<void> {
      await api.deleteSession(id)
      update(withoutSession(current(), id))
    },

    async fork(id: string, seq: number): Promise<void> {
      const session = await api.forkSession(id, { seq })
      update(withSessionChanged(current(), session))
      await selectSession(session.id)
    },

    async changeModel(model: string): Promise<void> {
      await changeSelectedSession({ model })
      rememberModel(model)
    },

    async changeMode(mode: Mode): Promise<void> {
      await changeSelectedSession({ mode })
      rememberMode(mode)
    },
  }
}

export type SessionStore = ReturnType<typeof createSessionStore>

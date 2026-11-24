import { createSignal } from "solid-js"
import type { ClientFrame, ToolCall } from "../protocol"
import { WorkspaceFileError, type WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"
import { documentState, type OpenWorkspace } from "../workspace/workspaceStore"
import { cutReadOutputLine, splitLines } from "./textLines"
import {
  isWorkspaceToolName,
  parseToolArguments,
  type EditArguments,
  type ReadArguments,
  type WriteArguments,
} from "./toolArguments"
import { failureFromError, toolFailure, toolSuccess, type ToolOutput } from "./toolOutput"
import { workerSearch, type WorkspaceSearch } from "./workerSearch"

const JOURNAL_LIMIT = 200
export const JOURNAL_CHARACTER_LIMIT = 16_000_000

export type ChangeApproval = "automatic" | "confirm"

export type EarlierContent = { readonly kind: "absent" } | { readonly kind: "text", readonly text: string }

export type FileChange = {
  readonly callId: string
  readonly path: string
  readonly earlier: EarlierContent
  readonly content: string
}

export type RevertResult =
  | { readonly kind: "reverted" }
  | { readonly kind: "conflict" }
  | { readonly kind: "unknown_change" }
  | { readonly kind: "failed", readonly message: string }

export type ToolResultFrame = Extract<ClientFrame, { type: "tool_result" }>

type JournalRecord = { readonly change: FileChange, readonly workspace: OpenWorkspace }
type PendingRecord = { readonly change: FileChange, readonly decide: (approved: boolean) => void }

export type ToolHostOptions = {
  readonly workspace: () => OpenWorkspace | undefined
  readonly approval: () => ChangeApproval
  readonly search?: WorkspaceSearch
  readonly journalCharacterLimit?: number
}

const unsavedChangesText = (path: string) =>
  `error: ${path} has unsaved changes in the editor. Ask the user to save or discard them.`

function hasUnsavedChanges(workspace: OpenWorkspace, path: string): boolean {
  return workspace.documents.some((document) => document.path === path && documentState(document) !== "saved")
}

async function currentContent(fileSystem: WorkspaceFileSystem, path: string): Promise<EarlierContent> {
  try {
    return { kind: "text", text: await fileSystem.readTextFile(path) }
  } catch (error) {
    if (error instanceof WorkspaceFileError && error.kind === "not_found") return { kind: "absent" }
    throw error
  }
}

function sameContent(a: EarlierContent, b: EarlierContent): boolean {
  if (a.kind === "absent" || b.kind === "absent") return a.kind === b.kind
  return a.text === b.text
}

function journalEntryCharacters(change: FileChange): number {
  const earlierCharacters = change.earlier.kind === "text" ? change.earlier.text.length : 0
  return earlierCharacters + change.content.length
}

function appendJournalRecord(
  records: readonly JournalRecord[],
  record: JournalRecord,
  entryLimit: number,
  characterLimit: number,
): readonly JournalRecord[] {
  if (journalEntryCharacters(record.change) > characterLimit) return records
  let next = [...records, record].slice(-entryLimit)
  let total = next.reduce((sum, entry) => sum + journalEntryCharacters(entry.change), 0)
  while (next.length > 0 && total > characterLimit) {
    const removed = next[0]
    if (!removed) break
    next = next.slice(1)
    total -= journalEntryCharacters(removed.change)
  }
  return next
}

async function readFile(workspace: OpenWorkspace, { path, offset, limit }: ReadArguments): Promise<ToolOutput> {
  const lines = splitLines(await workspace.fileSystem.readTextFile(path))
  if (offset > Math.max(lines.length, 1)) {
    return toolFailure(`error: offset ${offset} is past the end of ${path} (${lines.length} lines)`)
  }
  const shown = lines.slice(offset - 1, offset - 1 + limit)
  const last = offset - 1 + shown.length
  const output = shown.map((line, index) => `${offset + index}|${cutReadOutputLine(line)}`)
  if (last < lines.length) {
    output.push(`[lines ${offset}-${last} of ${lines.length}. Call read with offset=${last + 1} for more.]`)
  }
  return toolSuccess(output.join("\n"))
}

export function createToolHost(options: ToolHostOptions) {
  const search = options.search ?? workerSearch
  const journalCharacterLimit = options.journalCharacterLimit ?? JOURNAL_CHARACTER_LIMIT
  const [journalRecords, setJournalRecords] = createSignal<readonly JournalRecord[]>([])
  const [pendingRecords, setPendingRecords] = createSignal<readonly PendingRecord[]>([])

  function settlePending(callId: string, approved: boolean): void {
    const record = pendingRecords().find(({ change }) => change.callId === callId)
    if (!record) return
    setPendingRecords((records) => records.filter((entry) => entry !== record))
    record.decide(approved)
  }

  function awaitApproval(change: FileChange): Promise<boolean> {
    if (options.approval() === "automatic") return Promise.resolve(true)
    return new Promise((decide) => setPendingRecords((records) => [...records, { change, decide }]))
  }

  async function applyChange(workspace: OpenWorkspace, change: FileChange): Promise<ToolOutput | undefined> {
    if (!(await awaitApproval(change))) return toolFailure("error: the user declined this change")
    // The user can edit the buffer or the file while the change waits for approval
    if (hasUnsavedChanges(workspace, change.path)) return toolFailure(unsavedChangesText(change.path))
    const current = await currentContent(workspace.fileSystem, change.path)
    if (!sameContent(current, change.earlier)) {
      return toolFailure(`error: ${change.path} changed on disk during the approval. Call the tool again.`)
    }
    setJournalRecords((records) =>
      appendJournalRecord(records, { change, workspace }, JOURNAL_LIMIT, journalCharacterLimit),
    )
    await workspace.fileSystem.writeTextFile(change.path, change.content)
    await workspace.reloadFile(change.path)
    return undefined
  }

  async function editFile(workspace: OpenWorkspace, callId: string, edit: EditArguments): Promise<ToolOutput> {
    if (edit.old === "") return toolFailure("error: old text is empty")
    if (edit.old === edit.new) return toolFailure("error: old and new are the same")
    if (hasUnsavedChanges(workspace, edit.path)) return toolFailure(unsavedChangesText(edit.path))
    const text = await workspace.fileSystem.readTextFile(edit.path)
    const pieces = text.split(edit.old)
    const count = pieces.length - 1
    if (count === 0) return toolFailure(`error: old text not found in ${edit.path}`)
    if (count > 1 && !edit.all) {
      return toolFailure(`error: old text matches ${count} places in ${edit.path}. Add surrounding lines or set all=true.`)
    }
    const change = { callId, path: edit.path, earlier: { kind: "text", text } as const, content: pieces.join(edit.new) }
    return (await applyChange(workspace, change)) ?? toolSuccess(`edited ${edit.path}: ${count} replacement(s)`)
  }

  async function writeFile(workspace: OpenWorkspace, callId: string, write: WriteArguments): Promise<ToolOutput> {
    if (hasUnsavedChanges(workspace, write.path)) return toolFailure(unsavedChangesText(write.path))
    const earlier = await currentContent(workspace.fileSystem, write.path)
    const change = { callId, path: write.path, earlier, content: write.content }
    const lineCount = splitLines(write.content).length
    return (await applyChange(workspace, change)) ?? toolSuccess(`wrote ${write.path} (${lineCount} lines)`)
  }

  async function toolOutput(call: ToolCall): Promise<ToolOutput> {
    if (!isWorkspaceToolName(call.name)) return toolFailure(`error: unknown tool: ${call.name}`)
    const workspace = options.workspace()
    if (!workspace) return toolFailure("error: no workspace is open")
    const parsed = parseToolArguments(call.name, call.arguments)
    switch (parsed.tool) {
      case "read":
        return readFile(workspace, parsed.arguments)
      case "glob":
      case "grep":
        return search(workspace.directory, parsed.arguments)
      case "edit":
        return editFile(workspace, call.id, parsed.arguments)
      case "write":
        return writeFile(workspace, call.id, parsed.arguments)
      default: {
        const unhandled: never = parsed
        return unhandled
      }
    }
  }

  return {
    // Resolves with an answer for every call and never rejects
    async execute(call: ToolCall): Promise<ToolResultFrame> {
      const output = await toolOutput(call).catch(failureFromError)
      return { type: "tool_result", call_id: call.id, ...output }
    },

    journal: () => journalRecords().map(({ change }) => change),
    pendingChanges: () => pendingRecords().map(({ change }) => change),

    approve(callId: string): void {
      settlePending(callId, true)
    },

    decline(callId: string): void {
      settlePending(callId, false)
    },

    declineAll(): void {
      for (const { change } of pendingRecords()) settlePending(change.callId, false)
    },

    async revert(callId: string): Promise<RevertResult> {
      const record = journalRecords().find(({ change }) => change.callId === callId)
      if (!record) return { kind: "unknown_change" }
      const { change, workspace } = record
      if (workspace !== options.workspace()) {
        return { kind: "failed", message: "error: the change belongs to a workspace that is not open" }
      }
      try {
        if (hasUnsavedChanges(workspace, change.path)) {
          return { kind: "failed", message: unsavedChangesText(change.path) }
        }
        const current = await currentContent(workspace.fileSystem, change.path)
        if (!sameContent(current, { kind: "text", text: change.content })) return { kind: "conflict" }
        if (change.earlier.kind === "absent") await workspace.fileSystem.removeFile(change.path)
        else await workspace.fileSystem.writeTextFile(change.path, change.earlier.text)
        setJournalRecords((records) => records.filter((entry) => entry !== record))
        await workspace.reloadFile(change.path)
        return { kind: "reverted" }
      } catch (error) {
        return { kind: "failed", message: failureFromError(error).output }
      }
    },
  }
}

export type ToolHost = ReturnType<typeof createToolHost>

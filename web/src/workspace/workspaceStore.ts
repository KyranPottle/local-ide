import { batch, createSignal } from "solid-js"
import { createStore } from "solid-js/store"
import type { WorkspaceSnapshot } from "../protocol"
import { WorkspaceFileError, WorkspaceFileSystem } from "./WorkspaceFileSystem"

export type EditorDocument = {
  readonly path: string
  readonly bufferText: string
  // File text that the buffer edits started from, it differs from diskText only during a conflict
  readonly baseText: string
  // File text at the last read or write
  readonly diskText: string
}

export type DocumentState = "saved" | "modified" | "conflict"

export function documentState(document: EditorDocument): DocumentState {
  if (document.bufferText === document.diskText) return "saved"
  return document.baseText === document.diskText ? "modified" : "conflict"
}

export type EditorReveal = { readonly path: string, readonly line: number | undefined }

function createOpenWorkspace(directory: FileSystemDirectoryHandle) {
  const fileSystem = new WorkspaceFileSystem(directory)
  const [documents, setDocuments] = createStore<EditorDocument[]>([])
  const [activeFilePath, setActiveFilePath] = createSignal<string>()
  const [reveal, setReveal] = createSignal<EditorReveal>()
  const [fileTreeRevision, setFileTreeRevision] = createSignal(0)

  const hasPath = (path: string) => (document: EditorDocument) => document.path === path
  const findDocument = (path: string) => documents.find(hasPath(path))

  function refreshFileTree(): void {
    setFileTreeRevision((revision) => revision + 1)
  }

  async function syncDocument(path: string): Promise<void> {
    if (!findDocument(path)) return
    let diskText: string
    try {
      diskText = await fileSystem.readTextFile(path)
    } catch (error) {
      // A file that another program removed keeps its buffer, so that a save restores the file
      if (error instanceof WorkspaceFileError) return
      throw error
    }
    setDocuments(hasPath(path), (document) => {
      const clean = document.bufferText === document.diskText || document.bufferText === diskText
      return clean ? { bufferText: diskText, baseText: diskText, diskText } : { diskText }
    })
  }

  return {
    name: fileSystem.name,
    directory,
    fileSystem,
    documents,
    activeFilePath,
    reveal,
    fileTreeRevision,
    refreshFileTree,

    activeDocument(): EditorDocument | undefined {
      const path = activeFilePath()
      return path === undefined ? undefined : findDocument(path)
    },

    hasUnsavedChanges(): boolean {
      return documents.some((document) => documentState(document) !== "saved")
    },

    async snapshot(): Promise<WorkspaceSnapshot> {
      return {
        name: fileSystem.name,
        entries: await fileSystem.snapshotEntries(),
        active_file: activeFilePath() ?? null,
      }
    },

    async openFile(path: string, line?: number): Promise<void> {
      if (!findDocument(path)) {
        const text = await fileSystem.readTextFile(path)
        // A second call for the same path can add the document while this call reads the file
        if (!findDocument(path)) {
          setDocuments(documents.length, { path, bufferText: text, baseText: text, diskText: text })
        }
      }
      batch(() => {
        setActiveFilePath(path)
        setReveal({ path, line })
      })
    },

    closeFile(path: string): void {
      const index = documents.findIndex(hasPath(path))
      if (index === -1) return
      batch(() => {
        setDocuments((open) => open.filter((document) => document.path !== path))
        if (activeFilePath() === path) setActiveFilePath(documents[Math.min(index, documents.length - 1)]?.path)
      })
    },

    async saveFile(path: string): Promise<void> {
      const text = findDocument(path)?.bufferText
      if (text === undefined) return
      await fileSystem.writeTextFile(path, text)
      setDocuments(hasPath(path), { baseText: text, diskText: text })
    },

    // Call after another actor changed the file: a clean buffer takes the new text, a modified buffer gets a conflict
    async reloadFile(path: string): Promise<void> {
      refreshFileTree()
      await syncDocument(path)
    },

    async reloadOpenFiles(): Promise<void> {
      refreshFileTree()
      await Promise.all(documents.map((document) => syncDocument(document.path)))
    },

    bufferText(path: string): string | undefined {
      return findDocument(path)?.bufferText
    },

    setBufferText(path: string, text: string): void {
      setDocuments(hasPath(path), { bufferText: text })
    },

    discardBuffer(path: string): void {
      setDocuments(hasPath(path), (document) => ({ bufferText: document.diskText, baseText: document.diskText }))
    },

    keepBuffer(path: string): void {
      setDocuments(hasPath(path), (document) => ({ baseText: document.diskText }))
    },
  }
}

export type OpenWorkspace = ReturnType<typeof createOpenWorkspace>

export function createWorkspaceStore() {
  const [workspace, setWorkspace] = createSignal<OpenWorkspace>()

  return {
    workspace,
    open(directory: FileSystemDirectoryHandle): void {
      setWorkspace(createOpenWorkspace(directory))
    },
  }
}

export type WorkspaceStore = ReturnType<typeof createWorkspaceStore>

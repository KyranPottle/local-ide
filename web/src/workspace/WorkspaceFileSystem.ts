export const ROOT_DIRECTORY = ""
export const IGNORED_DIRECTORY_NAMES: ReadonlySet<string> = new Set([
  ".git",
  "node_modules",
  "target",
  "dist",
  "build",
  ".next",
  ".venv",
  "__pycache__",
])
const SNAPSHOT_ENTRY_LIMIT = 60
const BINARY_PROBE_BYTES = 8000

export type DirectoryEntry = { readonly name: string, readonly kind: FileSystemHandleKind }

export type WorkspaceFileErrorKind =
  | "outside_workspace"
  | "not_found"
  | "not_a_file"
  | "not_a_directory"
  | "binary"

// The text is the tool output that the model reads
const errorText: Record<WorkspaceFileErrorKind, (path: string) => string> = {
  outside_workspace: () => "error: path outside workspace",
  not_found: (path) => `error: file not found: ${path}`,
  not_a_file: (path) => `error: not a file: ${path}`,
  not_a_directory: (path) => `error: not a directory: ${path}`,
  binary: (path) => `error: binary file: ${path}`,
}

export class WorkspaceFileError extends Error {
  readonly kind: WorkspaceFileErrorKind
  readonly path: string

  constructor(kind: WorkspaceFileErrorKind, path: string) {
    super(errorText[kind](path))
    this.name = "WorkspaceFileError"
    this.kind = kind
    this.path = path
  }
}

export function joinPath(directory: string, name: string): string {
  return directory === ROOT_DIRECTORY ? name : `${directory}/${name}`
}

export function fileName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1)
}

export function parentDirectory(path: string): string {
  return path.slice(0, Math.max(path.lastIndexOf("/"), 0))
}

function splitPath(path: string): { parents: string[], name: string } {
  const parents = path.split("/")
  const name = parents.pop()
  const escapes = (segment: string) => segment === "" || segment === ".."
  if (name === undefined || path.includes("\\") || escapes(name) || parents.some(escapes)) {
    throw new WorkspaceFileError("outside_workspace", path)
  }
  return { parents, name }
}

function directorySegments(path: string): string[] {
  if (path === ROOT_DIRECTORY) return []
  const { parents, name } = splitPath(path)
  return [...parents, name]
}

function exceptionName(error: unknown): string | undefined {
  return error instanceof DOMException ? error.name : undefined
}

function workspaceFileError(
  error: unknown,
  path: string,
  mismatchKind: "not_a_file" | "not_a_directory",
  mismatchPath: string,
): unknown {
  if (exceptionName(error) === "NotFoundError") return new WorkspaceFileError("not_found", path)
  if (exceptionName(error) === "TypeMismatchError") return new WorkspaceFileError(mismatchKind, mismatchPath)
  return error
}

function compareText(a: string, b: string): number {
  return Number(a > b) - Number(a < b)
}

function compareEntries(a: DirectoryEntry, b: DirectoryEntry): number {
  return (
    Number(b.kind === "directory") - Number(a.kind === "directory") ||
    compareText(a.name.toLowerCase(), b.name.toLowerCase()) ||
    compareText(a.name, b.name)
  )
}

async function sortedHandles(
  directory: FileSystemDirectoryHandle,
): Promise<(FileSystemDirectoryHandle | FileSystemFileHandle)[]> {
  const handles = []
  for await (const handle of directory.values()) handles.push(handle)
  return handles.sort(compareEntries)
}

async function* walkDirectory(directory: FileSystemDirectoryHandle, path: string): AsyncGenerator<string> {
  for (const handle of await sortedHandles(directory)) {
    const entryPath = joinPath(path, handle.name)
    if (handle.kind === "file") {
      yield entryPath
      continue
    }
    if (IGNORED_DIRECTORY_NAMES.has(handle.name)) continue
    yield* walkDirectory(handle, entryPath)
  }
}

export class WorkspaceFileSystem {
  readonly name: string
  readonly #root: FileSystemDirectoryHandle

  constructor(root: FileSystemDirectoryHandle) {
    this.#root = root
    this.name = root.name
  }

  async listDirectory(path: string): Promise<DirectoryEntry[]> {
    const directory = await this.#directory(directorySegments(path), path, false)
    const handles = await sortedHandles(directory)
    return handles.map(({ name, kind }) => ({ name, kind }))
  }

  async readTextFile(path: string): Promise<string> {
    const handle = await this.#file(path, false)
    const file = await handle.getFile()
    const bytes = new Uint8Array(await file.arrayBuffer())
    if (bytes.subarray(0, BINARY_PROBE_BYTES).includes(0)) throw new WorkspaceFileError("binary", path)
    return new TextDecoder().decode(bytes)
  }

  async writeTextFile(path: string, text: string): Promise<void> {
    const handle = await this.#file(path, true)
    const writable = await handle.createWritable()
    await writable.write(text)
    await writable.close()
  }

  async removeFile(path: string): Promise<void> {
    const { parents, name } = splitPath(path)
    await this.#file(path, false)
    const directory = await this.#directory(parents, path, false)
    await directory.removeEntry(name)
  }

  async exists(path: string): Promise<boolean> {
    if (path === ROOT_DIRECTORY) return true
    try {
      await this.#file(path, false)
      return true
    } catch (error) {
      if (!(error instanceof WorkspaceFileError) || error.kind === "outside_workspace") throw error
      // A directory at the path answers a file lookup with a type mismatch
      return error.kind === "not_a_file"
    }
  }

  walkFiles(): AsyncGenerator<string> {
    return walkDirectory(this.#root, ROOT_DIRECTORY)
  }

  async snapshotEntries(): Promise<string[]> {
    const entries = await this.listDirectory(ROOT_DIRECTORY)
    return entries
      .slice(0, SNAPSHOT_ENTRY_LIMIT)
      .map((entry) => (entry.kind === "directory" ? `${entry.name}/` : entry.name))
  }

  async #directory(segments: readonly string[], path: string, create: boolean): Promise<FileSystemDirectoryHandle> {
    let directory = this.#root
    for (const [index, name] of segments.entries()) {
      try {
        directory = await directory.getDirectoryHandle(name, { create })
      } catch (error) {
        throw workspaceFileError(error, path, "not_a_directory", segments.slice(0, index + 1).join("/"))
      }
    }
    return directory
  }

  async #file(path: string, create: boolean): Promise<FileSystemFileHandle> {
    const { parents, name } = splitPath(path)
    const directory = await this.#directory(parents, path, create)
    try {
      return await directory.getFileHandle(name, { create })
    } catch (error) {
      throw workspaceFileError(error, path, "not_a_file", path)
    }
  }
}

export type MemoryDirectoryContent = { [name: string]: string | Uint8Array<ArrayBuffer> | MemoryDirectoryContent }

function assertEntryName(name: string): void {
  if (name === "" || name === "." || name === ".." || name.includes("/")) throw new TypeError("Name is not allowed.")
}

class MemoryFileHandle {
  readonly kind = "file"
  readonly name: string
  #bytes: Uint8Array<ArrayBuffer>

  constructor(name: string, bytes: Uint8Array<ArrayBuffer>) {
    this.name = name
    this.#bytes = bytes
  }

  async getFile(): Promise<File> {
    return new File([this.#bytes], this.name)
  }

  async createWritable() {
    const chunks: string[] = []
    return {
      write: async (text: string) => {
        chunks.push(text)
      },
      close: async () => {
        this.#bytes = new TextEncoder().encode(chunks.join(""))
      },
    }
  }
}

class MemoryDirectoryHandle {
  readonly kind = "directory"
  readonly name: string
  readonly #entries = new Map<string, MemoryFileHandle | MemoryDirectoryHandle>()

  constructor(name: string, content: MemoryDirectoryContent) {
    this.name = name
    for (const [entryName, entry] of Object.entries(content)) this.#entries.set(entryName, memoryHandle(entryName, entry))
  }

  async getDirectoryHandle(name: string, options?: { create?: boolean }): Promise<MemoryDirectoryHandle> {
    const existing = this.#existing(name, options)
    if (existing instanceof MemoryFileHandle) throw new DOMException("The entry is a file.", "TypeMismatchError")
    if (existing) return existing
    const created = new MemoryDirectoryHandle(name, {})
    this.#entries.set(name, created)
    return created
  }

  async getFileHandle(name: string, options?: { create?: boolean }): Promise<MemoryFileHandle> {
    const existing = this.#existing(name, options)
    if (existing instanceof MemoryDirectoryHandle) {
      throw new DOMException("The entry is a directory.", "TypeMismatchError")
    }
    if (existing) return existing
    const created = new MemoryFileHandle(name, new Uint8Array())
    this.#entries.set(name, created)
    return created
  }

  async removeEntry(name: string): Promise<void> {
    this.#existing(name)
    this.#entries.delete(name)
  }

  async *values() {
    yield* this.#entries.values()
  }

  #existing(name: string, options?: { create?: boolean }) {
    assertEntryName(name)
    const existing = this.#entries.get(name)
    if (!existing && !options?.create) throw new DOMException("The entry does not exist.", "NotFoundError")
    return existing
  }
}

function memoryHandle(name: string, entry: MemoryDirectoryContent[string]): MemoryFileHandle | MemoryDirectoryHandle {
  if (typeof entry === "string") return new MemoryFileHandle(name, new TextEncoder().encode(entry))
  if (entry instanceof Uint8Array) return new MemoryFileHandle(name, entry)
  return new MemoryDirectoryHandle(name, entry)
}

export function memoryDirectoryHandle(name: string, content: MemoryDirectoryContent): FileSystemDirectoryHandle {
  // The fixture implements only the members that WorkspaceFileSystem calls
  return new MemoryDirectoryHandle(name, content) as unknown as FileSystemDirectoryHandle
}

export type FolderAccessSupport = { kind: "available" } | { kind: "braveFlagOff" } | { kind: "unavailable" }

export function folderAccessSupport(
  scope: Pick<Window, "showDirectoryPicker"> = window,
  browser: { brave?: unknown } = navigator as { brave?: unknown },
): FolderAccessSupport {
  if (typeof scope.showDirectoryPicker === "function") return { kind: "available" }
  // Brave ships the File System Access API behind brave://flags/#file-system-access-api
  if (browser.brave !== undefined) return { kind: "braveFlagOff" }
  return { kind: "unavailable" }
}

// Resolves to undefined when the user closes the picker without a selection
export async function pickDirectory(): Promise<FileSystemDirectoryHandle | undefined> {
  try {
    return await window.showDirectoryPicker?.({ mode: "readwrite" })
  } catch (error) {
    if (error instanceof DOMException && error.name === "AbortError") return undefined
    throw error
  }
}

// Call during the drop event: the transfer items are readable only until the event listener returns
export function droppedDirectory(transfer: DataTransfer): Promise<FileSystemDirectoryHandle | undefined> {
  const handles = [...transfer.items]
    .filter((item) => item.kind === "file")
    .map((item) => item.getAsFileSystemHandle())
  return Promise.all(handles).then((resolved) =>
    resolved.find((handle): handle is FileSystemDirectoryHandle => handle?.kind === "directory"),
  )
}

export async function requestReadWriteAccess(directory: FileSystemDirectoryHandle): Promise<void> {
  const permission = await directory.requestPermission({ mode: "readwrite" })
  if (permission !== "granted") throw new Error(`Access to ${directory.name} was not granted`)
}

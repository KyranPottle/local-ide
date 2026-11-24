const DATABASE_NAME = "local-ide"
const OBJECT_STORE_NAME = "workspaces"
const RECENT_KEY = "recent"
const RECENT_WORKSPACE_LIMIT = 12

function settled<Result>(request: IDBRequest<Result>): Promise<Result> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

async function withObjectStore<Result>(
  mode: IDBTransactionMode,
  operation: (objectStore: IDBObjectStore) => IDBRequest<Result>,
): Promise<Result> {
  const opening = indexedDB.open(DATABASE_NAME, 1)
  opening.onupgradeneeded = () => opening.result.createObjectStore(OBJECT_STORE_NAME)
  const database = await settled(opening)
  try {
    return await settled(operation(database.transaction(OBJECT_STORE_NAME, mode).objectStore(OBJECT_STORE_NAME)))
  } finally {
    database.close()
  }
}

async function withoutDirectory(
  directories: FileSystemDirectoryHandle[],
  directory: FileSystemDirectoryHandle,
): Promise<FileSystemDirectoryHandle[]> {
  const same = await Promise.all(directories.map((recent) => recent.isSameEntry(directory)))
  return directories.filter((_, index) => !same[index])
}

export async function listRecentWorkspaces(): Promise<FileSystemDirectoryHandle[]> {
  const stored: FileSystemDirectoryHandle[] | undefined = await withObjectStore("readonly", (objectStore) =>
    objectStore.get(RECENT_KEY),
  )
  return stored ?? []
}

export async function rememberWorkspace(directory: FileSystemDirectoryHandle): Promise<void> {
  const others = await withoutDirectory(await listRecentWorkspaces(), directory)
  const recent = [directory, ...others].slice(0, RECENT_WORKSPACE_LIMIT)
  await withObjectStore("readwrite", (objectStore) => objectStore.put(recent, RECENT_KEY))
}

export async function forgetWorkspace(directory: FileSystemDirectoryHandle): Promise<void> {
  const recent = await withoutDirectory(await listRecentWorkspaces(), directory)
  await withObjectStore("readwrite", (objectStore) => objectStore.put(recent, RECENT_KEY))
}

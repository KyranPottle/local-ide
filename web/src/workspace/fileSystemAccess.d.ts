// Chromium File System Access members that the TypeScript DOM library omits

interface FileSystemHandle {
  requestPermission(descriptor: { mode: "read" | "readwrite" }): Promise<PermissionState>
}

interface DataTransferItem {
  getAsFileSystemHandle(): Promise<FileSystemFileHandle | FileSystemDirectoryHandle | null>
}

interface Window {
  showDirectoryPicker?(options: { mode: "read" | "readwrite" }): Promise<FileSystemDirectoryHandle>
}

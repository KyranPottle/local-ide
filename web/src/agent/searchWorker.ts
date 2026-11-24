import { WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"
import type { SearchRequest } from "./workspaceSearch"
import { runSearch } from "./workspaceSearch"

export type SearchWorkerMessage = { readonly root: FileSystemDirectoryHandle, readonly request: SearchRequest }

self.onmessage = async (event: MessageEvent<SearchWorkerMessage>) => {
  const { root, request } = event.data
  self.postMessage(await runSearch(new WorkspaceFileSystem(root), request))
}

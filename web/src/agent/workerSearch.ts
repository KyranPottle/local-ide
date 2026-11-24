import type { SearchWorkerMessage } from "./searchWorker"
import { toolFailure, type ToolOutput } from "./toolOutput"
import type { SearchRequest } from "./workspaceSearch"

const SEARCH_TIME_LIMIT_MILLISECONDS = 10_000

export type WorkspaceSearch = (root: FileSystemDirectoryHandle, request: SearchRequest) => Promise<ToolOutput>

// A model can send a catastrophic regular expression, and only a worker can be stopped in the middle of a match
export const workerSearch: WorkspaceSearch = (root, request) =>
  new Promise((resolve) => {
    const worker = new Worker(new URL("./searchWorker.ts", import.meta.url), { type: "module" })
    const finish = (output: ToolOutput) => {
      clearTimeout(timer)
      worker.terminate()
      resolve(output)
    }
    const timer = setTimeout(
      () => finish(toolFailure("error: search timed out. Narrow the pattern or the path.")),
      SEARCH_TIME_LIMIT_MILLISECONDS,
    )
    worker.onmessage = (event: MessageEvent<ToolOutput>) => finish(event.data)
    worker.onerror = (event) => finish(toolFailure(`error: search failed: ${event.message}`))
    const message: SearchWorkerMessage = { root, request }
    worker.postMessage(message)
  })

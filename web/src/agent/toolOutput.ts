import { WorkspaceFileError } from "../workspace/WorkspaceFileSystem"

export type ToolOutput = { readonly output: string, readonly is_error: boolean }

export const toolSuccess = (output: string): ToolOutput => ({ output, is_error: false })
export const toolFailure = (output: string): ToolOutput => ({ output, is_error: true })

export function failureFromError(error: unknown): ToolOutput {
  if (error instanceof WorkspaceFileError) return toolFailure(error.message)
  const message = error instanceof Error ? error.message : String(error)
  return toolFailure(`error: ${message}`)
}

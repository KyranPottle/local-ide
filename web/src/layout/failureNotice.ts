import { createSignal } from "solid-js"
import { WorkspaceFileError, type WorkspaceFileErrorKind } from "../workspace/WorkspaceFileSystem"

const fileErrorReason: Record<WorkspaceFileErrorKind, string> = {
  outside_workspace: "The path is outside the workspace",
  not_found: "File not found",
  not_a_file: "Not a file",
  not_a_directory: "Not a directory",
  binary: "Binary file",
}

function failureText(error: unknown): string {
  if (error instanceof WorkspaceFileError) return `${fileErrorReason[error.kind]}: ${error.path}`
  return error instanceof Error ? error.message : String(error)
}

const [failureNotice, setFailureNotice] = createSignal<string>()

export { failureNotice }

export function reportFailure(error: unknown): void {
  setFailureNotice(failureText(error))
}

export function dismissFailureNotice(): void {
  setFailureNotice(undefined)
}

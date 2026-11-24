import { WorkspaceFileError, type WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"

export const REPO_RULES_PATH = "AGENTS.md"
export const RULES_CHARACTER_LIMIT = 2000

export async function readRepoRules(fileSystem: WorkspaceFileSystem): Promise<string | null> {
  let text: string
  try {
    text = await fileSystem.readTextFile(REPO_RULES_PATH)
  } catch (error) {
    if (error instanceof WorkspaceFileError && error.kind === "not_found") return null
    throw error
  }
  const rules = text.trim()
  if (rules === "") return null
  if ([...rules].length > RULES_CHARACTER_LIMIT) {
    throw new Error(`repo rules must hold at most ${RULES_CHARACTER_LIMIT} characters`)
  }
  return rules
}

import { describe, expect, test } from "vitest"
import { WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"
import { memoryDirectoryHandle } from "../workspace/memoryDirectoryHandle"
import { RULES_CHARACTER_LIMIT, readRepoRules } from "./repoRules"

function files(content: Record<string, string>): WorkspaceFileSystem {
  return new WorkspaceFileSystem(memoryDirectoryHandle("project", content))
}

describe("readRepoRules", () => {
  test("the rules cap is 2000 characters", () => {
    expect(RULES_CHARACTER_LIMIT).toBe(2000)
  })

  test("a missing file sends no rules", async () => {
    expect(await readRepoRules(files({ "README.md": "hello" }))).toBeNull()
  })

  test("a blank file sends no rules", async () => {
    expect(await readRepoRules(files({ "AGENTS.md": " \n\t" }))).toBeNull()
  })

  test("the trimmed text of AGENTS.md is the repo rules", async () => {
    expect(await readRepoRules(files({ "AGENTS.md": "\nUse pnpm.\n" }))).toBe("Use pnpm.")
  })

  test("a text at the cap is kept", async () => {
    const rules = "é".repeat(RULES_CHARACTER_LIMIT)
    expect(await readRepoRules(files({ "AGENTS.md": ` ${rules} ` }))).toBe(rules)
  })

  test("a text past the cap is rejected", async () => {
    const rules = "x".repeat(RULES_CHARACTER_LIMIT + 1)
    await expect(readRepoRules(files({ "AGENTS.md": rules }))).rejects.toThrow(
      "repo rules must hold at most 2000 characters",
    )
  })
})

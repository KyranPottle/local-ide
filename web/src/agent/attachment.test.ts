import { expect, test } from "vitest"
import { memoryDirectoryHandle } from "../workspace/memoryDirectoryHandle"
import { WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"
import { buildAttachment } from "./attachment"

const numberedLines = (count: number) => Array.from({ length: count }, (_, index) => `${index + 1}`).join("\n")

const fileSystem = new WorkspaceFileSystem(
  memoryDirectoryHandle("project", { "short.txt": "a\nb\nc\n", "long.txt": numberedLines(450) }),
)

test("attaches a whole file", async () => {
  expect(await buildAttachment(fileSystem, "short.txt")).toEqual({ path: "short.txt", lines: null, content: "a\nb\nc" })
})

test("attaches a line range", async () => {
  expect(await buildAttachment(fileSystem, "short.txt", { first: 2, last: 3 })).toEqual({
    path: "short.txt",
    lines: { first: 2, last: 3 },
    content: "b\nc",
  })
})

test("cuts at 400 lines with a note", async () => {
  const attachment = await buildAttachment(fileSystem, "long.txt", { first: 11, last: 450 })
  const lines = attachment.content.split("\n")
  expect(attachment.lines).toEqual({ first: 11, last: 410 })
  expect(lines).toHaveLength(401)
  expect(lines[399]).toBe("410")
  expect(lines[400]).toBe("[attachment cut at 400 lines: lines 11-410 of 11-450]")
})

test("rejects a range outside the file", async () => {
  await expect(buildAttachment(fileSystem, "short.txt", { first: 5, last: 6 })).rejects.toThrow(
    "line range 5-6 is outside short.txt (3 lines)",
  )
  await expect(buildAttachment(fileSystem, "../x")).rejects.toThrow("error: path outside workspace")
})

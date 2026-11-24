import { describe, expect, test } from "vitest"
import { memoryDirectoryHandle, type MemoryDirectoryContent } from "./memoryDirectoryHandle"
import { ROOT_DIRECTORY, WorkspaceFileSystem } from "./WorkspaceFileSystem"

function workspace(content: MemoryDirectoryContent): WorkspaceFileSystem {
  return new WorkspaceFileSystem(memoryDirectoryHandle("project", content))
}

async function walkedPaths(fileSystem: WorkspaceFileSystem): Promise<string[]> {
  const paths = []
  for await (const path of fileSystem.walkFiles()) paths.push(path)
  return paths
}

function bytesWithNulAt(length: number, index: number): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(length).fill(0x61)
  bytes[index] = 0
  return bytes
}

describe("path validation", () => {
  const rejectedPaths = [
    ["a leading slash", "/etc/passwd"],
    ["a parent segment at the start", "../secret.txt"],
    ["a parent segment in the middle", "src/../../secret.txt"],
    ["a parent segment at the end", "src/.."],
    ["a parent segment alone", ".."],
    ["an empty segment in the middle", "src//main.ts"],
    ["an empty segment at the end", "src/"],
    ["a backslash separator", "src\\main.ts"],
    ["a backslash parent segment", "..\\secret.txt"],
  ] as const

  const fileSystem = workspace({ src: { "main.ts": "export {}\n" } })

  test.each(rejectedPaths)("read rejects %s", async (_case, path) => {
    await expect(fileSystem.readTextFile(path)).rejects.toThrow("error: path outside workspace")
  })

  test.each(rejectedPaths)("write rejects %s", async (_case, path) => {
    await expect(fileSystem.writeTextFile(path, "text")).rejects.toThrow("error: path outside workspace")
  })

  test.each(rejectedPaths)("list rejects %s", async (_case, path) => {
    await expect(fileSystem.listDirectory(path)).rejects.toThrow("error: path outside workspace")
  })

  test.each(rejectedPaths)("existence check rejects %s", async (_case, path) => {
    await expect(fileSystem.exists(path)).rejects.toThrow("error: path outside workspace")
  })

  test("read and write reject the empty path", async () => {
    await expect(fileSystem.readTextFile("")).rejects.toThrow("error: path outside workspace")
    await expect(fileSystem.writeTextFile("", "text")).rejects.toThrow("error: path outside workspace")
  })

  test("a rejected write creates no entry", async () => {
    const target = workspace({})
    await expect(target.writeTextFile("made/../escape.txt", "text")).rejects.toThrow("error: path outside workspace")
    expect(await target.listDirectory(ROOT_DIRECTORY)).toEqual([])
  })
})

describe("listDirectory", () => {
  test("puts directories first and ignores case in the name order", async () => {
    const fileSystem = workspace({
      "zeta.txt": "",
      src: {},
      "Alpha.txt": "",
      ".git": {},
      "beta.txt": "",
      Docs: {},
    })

    expect(await fileSystem.listDirectory(ROOT_DIRECTORY)).toEqual([
      { name: ".git", kind: "directory" },
      { name: "Docs", kind: "directory" },
      { name: "src", kind: "directory" },
      { name: "Alpha.txt", kind: "file" },
      { name: "beta.txt", kind: "file" },
      { name: "zeta.txt", kind: "file" },
    ])
  })

  test("lists a nested directory", async () => {
    const fileSystem = workspace({ src: { editor: { "tabs.ts": "" }, "main.ts": "" } })

    expect(await fileSystem.listDirectory("src")).toEqual([
      { name: "editor", kind: "directory" },
      { name: "main.ts", kind: "file" },
    ])
  })

  test("reports a missing directory and a file in the place of a directory", async () => {
    const fileSystem = workspace({ "README.md": "" })

    await expect(fileSystem.listDirectory("src")).rejects.toThrow("error: file not found: src")
    await expect(fileSystem.listDirectory("README.md")).rejects.toThrow("error: not a directory: README.md")
  })
})

describe("readTextFile", () => {
  test("reads UTF-8 text", async () => {
    const fileSystem = workspace({ notes: { "café.md": "Grüße, 世界\n" } })

    expect(await fileSystem.readTextFile("notes/café.md")).toBe("Grüße, 世界\n")
  })

  test("rejects a file with a NUL byte in the first 8000 bytes", async () => {
    const fileSystem = workspace({
      "logo.png": new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x00, 0x0d]),
      "edge.bin": bytesWithNulAt(8001, 7999),
    })

    await expect(fileSystem.readTextFile("logo.png")).rejects.toThrow("error: binary file: logo.png")
    await expect(fileSystem.readTextFile("edge.bin")).rejects.toThrow("error: binary file: edge.bin")
  })

  test("reads a file whose first NUL byte is after the first 8000 bytes", async () => {
    const fileSystem = workspace({ "late.txt": bytesWithNulAt(8001, 8000) })

    const text = await fileSystem.readTextFile("late.txt")

    expect(text.length).toBe(8001)
    expect(text.endsWith("a\0")).toBe(true)
  })

  test("reads a file above 2 MB", async () => {
    const fileSystem = workspace({ "big.log": "a".repeat(2_097_153) })

    expect((await fileSystem.readTextFile("big.log")).length).toBe(2_097_153)
  })

  test("reports a missing file and a directory in the place of a file", async () => {
    const fileSystem = workspace({ src: {}, "README.md": "" })

    await expect(fileSystem.readTextFile("missing.txt")).rejects.toThrow("error: file not found: missing.txt")
    await expect(fileSystem.readTextFile("src/missing.txt")).rejects.toThrow("error: file not found: src/missing.txt")
    await expect(fileSystem.readTextFile("lib/missing.txt")).rejects.toThrow("error: file not found: lib/missing.txt")
    await expect(fileSystem.readTextFile("src")).rejects.toThrow("error: not a file: src")
    await expect(fileSystem.readTextFile("README.md/part")).rejects.toThrow("error: not a directory: README.md")
  })
})

describe("writeTextFile", () => {
  test("creates the parent directories", async () => {
    const fileSystem = workspace({})

    await fileSystem.writeTextFile("src/editor/tabs.ts", "export const tabs = []\n")

    expect(await fileSystem.listDirectory(ROOT_DIRECTORY)).toEqual([{ name: "src", kind: "directory" }])
    expect(await fileSystem.listDirectory("src")).toEqual([{ name: "editor", kind: "directory" }])
    expect(await fileSystem.listDirectory("src/editor")).toEqual([{ name: "tabs.ts", kind: "file" }])
    expect(await fileSystem.readTextFile("src/editor/tabs.ts")).toBe("export const tabs = []\n")
  })

  test("replaces the content of an existing file", async () => {
    const fileSystem = workspace({ "notes.md": "a long first version\n" })

    await fileSystem.writeTextFile("notes.md", "short\n")

    expect(await fileSystem.readTextFile("notes.md")).toBe("short\n")
  })

  test("rejects a path through a file and a path of a directory", async () => {
    const fileSystem = workspace({ src: {}, "README.md": "" })

    await expect(fileSystem.writeTextFile("README.md/part.txt", "text")).rejects.toThrow(
      "error: not a directory: README.md",
    )
    await expect(fileSystem.writeTextFile("src", "text")).rejects.toThrow("error: not a file: src")
  })
})

describe("exists", () => {
  test("reports files, directories, and the root", async () => {
    const fileSystem = workspace({ src: { "main.ts": "" }, "README.md": "" })

    expect(await fileSystem.exists("README.md")).toBe(true)
    expect(await fileSystem.exists("src/main.ts")).toBe(true)
    expect(await fileSystem.exists("src")).toBe(true)
    expect(await fileSystem.exists(ROOT_DIRECTORY)).toBe(true)
  })

  test("reports missing entries", async () => {
    const fileSystem = workspace({ src: {}, "README.md": "" })

    expect(await fileSystem.exists("missing.txt")).toBe(false)
    expect(await fileSystem.exists("src/missing.txt")).toBe(false)
    expect(await fileSystem.exists("lib/missing.txt")).toBe(false)
    expect(await fileSystem.exists("README.md/part")).toBe(false)
  })
})

describe("walkFiles", () => {
  test("yields each file path in list order", async () => {
    const fileSystem = workspace({
      "README.md": "",
      src: { "main.ts": "", editor: { "tabs.ts": "" } },
      docs: { "guide.md": "" },
    })

    expect(await walkedPaths(fileSystem)).toEqual([
      "docs/guide.md",
      "src/editor/tabs.ts",
      "src/main.ts",
      "README.md",
    ])
  })

  test("skips the ignored directories at each depth", async () => {
    const fileSystem = workspace({
      ".git": { HEAD: "" },
      node_modules: { solid: { "index.js": "" } },
      target: { "debug.bin": "" },
      dist: { "index.js": "" },
      build: { "out.o": "" },
      ".next": { "cache.json": "" },
      ".venv": { "pyvenv.cfg": "" },
      __pycache__: { "main.pyc": "" },
      packages: {
        core: { "lib.rs": "", target: { "core.rlib": "" }, node_modules: { "dep.js": "" } },
      },
      "build.rs": "",
      "target.md": "",
    })

    expect(await walkedPaths(fileSystem)).toEqual(["packages/core/lib.rs", "build.rs", "target.md"])
  })
})

describe("snapshotEntries", () => {
  test("marks each directory with a slash suffix", async () => {
    const fileSystem = workspace({ "README.md": "", src: {}, "Cargo.toml": "", crates: {} })

    expect(await fileSystem.snapshotEntries()).toEqual(["crates/", "src/", "Cargo.toml", "README.md"])
  })

  test("holds at most 60 entries", async () => {
    const content: MemoryDirectoryContent = { assets: {}, src: {} }
    for (let index = 10; index < 80; index++) content[`note-${index}.md`] = ""
    const fileSystem = workspace(content)

    const entries = await fileSystem.snapshotEntries()

    expect(entries.length).toBe(60)
    expect(entries.slice(0, 3)).toEqual(["assets/", "src/", "note-10.md"])
    expect(entries[59]).toBe("note-67.md")
  })
})

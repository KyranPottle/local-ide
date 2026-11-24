import { createSignal } from "solid-js"
import { describe, expect, test } from "vitest"
import type { JsonValue } from "../protocol"
import { memoryDirectoryHandle, type MemoryDirectoryContent } from "../workspace/memoryDirectoryHandle"
import { WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"
import { createWorkspaceStore, type OpenWorkspace } from "../workspace/workspaceStore"
import { createToolHost, type ChangeApproval } from "./toolHost"
import { runSearch } from "./workspaceSearch"

type SetupOptions = {
  readonly approval?: ChangeApproval
  readonly journalCharacterLimit?: number
}

function setup(content: MemoryDirectoryContent, options: SetupOptions | ChangeApproval = "automatic") {
  const resolved =
    typeof options === "string" ? { approval: options } : { approval: "automatic" as const, ...options }
  const store = createWorkspaceStore()
  store.open(memoryDirectoryHandle("project", content))
  const workspace = store.workspace() as OpenWorkspace
  const [currentApproval] = createSignal(resolved.approval)
  const host = createToolHost({
    workspace: () => workspace,
    approval: currentApproval,
    search: (root, request) => runSearch(new WorkspaceFileSystem(root), request),
    journalCharacterLimit: resolved.journalCharacterLimit,
  })
  const run = (name: string, args: JsonValue, id = "call-1") => host.execute({ id, name, arguments: args })
  return { host, workspace, run }
}

const numberedLines = (count: number) => Array.from({ length: count }, (_, index) => `line ${index + 1}`).join("\n")

const flush = () => new Promise((resolve) => setTimeout(resolve, 0))

describe("read", () => {
  const { run } = setup({ "ten.txt": numberedLines(10) + "\n", "long.txt": numberedLines(500), "empty.txt": "" })

  test("first window", async () => {
    expect(await run("read", { path: "ten.txt", limit: 4 })).toEqual({
      type: "tool_result",
      call_id: "call-1",
      output: "1|line 1\n2|line 2\n3|line 3\n4|line 4\n[lines 1-4 of 10. Call read with offset=5 for more.]",
      is_error: false,
    })
  })

  test("middle window", async () => {
    expect((await run("read", { path: "ten.txt", offset: 5, limit: 3 })).output).toBe(
      "5|line 5\n6|line 6\n7|line 7\n[lines 5-7 of 10. Call read with offset=8 for more.]",
    )
  })

  test("last window", async () => {
    expect((await run("read", { path: "ten.txt", offset: 8, limit: 4 })).output).toBe("8|line 8\n9|line 9\n10|line 10")
  })

  test("offset past the end", async () => {
    expect(await run("read", { path: "ten.txt", offset: 11 })).toMatchObject({
      output: "error: offset 11 is past the end of ten.txt (10 lines)",
      is_error: true,
    })
  })

  test("omitted limit returns the whole file", async () => {
    const lines = (await run("read", { path: "long.txt" })).output.split("\n")
    expect(lines).toHaveLength(500)
    expect(lines[0]).toBe("1|line 1")
    expect(lines[499]).toBe("500|line 500")
  })

  test("a limit past the end returns the rest of the file", async () => {
    const lines = (await run("read", { path: "long.txt", limit: 1000 })).output.split("\n")
    expect(lines).toHaveLength(500)
    expect(lines[499]).toBe("500|line 500")
  })

  test("empty file", async () => {
    expect(await run("read", { path: "empty.txt" })).toMatchObject({ output: "", is_error: false })
  })

  test("returns a long line whole", async () => {
    const longLine = `${"a".repeat(501)}\u{1F600}`
    expect((await setup({ "wide.txt": longLine }).run("read", { path: "wide.txt" })).output).toBe(`1|${longLine}`)
  })

  test("errors", async () => {
    const binary = setup({ "image.png": new Uint8Array([137, 80, 0, 71]), "note.txt": "x" })
    expect(await binary.run("read", { path: "missing.txt" })).toMatchObject({
      output: "error: file not found: missing.txt",
      is_error: true,
    })
    expect((await binary.run("read", { path: "image.png" })).output).toBe("error: binary file: image.png")
    expect((await binary.run("read", { path: 5 })).output).toBe("error: argument path must be a string")
    expect((await binary.run("read", { path: "note.txt", offset: 0 })).output).toBe(
      "error: argument offset must be an integer of 1 or more",
    )
    expect((await binary.run("read", "note.txt")).output).toBe("error: arguments must be an object")
  })
})

describe("glob", () => {
  const { run } = setup({
    "README.md": "# readme\n",
    "Cargo.toml": "[package]\n",
    src: {
      "main.rs": "fn main() {}\n",
      "lib.rs": "pub fn lib() {}\n",
      "a.ts": "",
      "b.tsx": "",
      "c1.ts": "",
      util: { "deep.rs": "" },
    },
    docs: { "guide.md": "" },
    node_modules: { pkg: { "index.js": "" } },
    target: { "out.rs": "" },
    ".git": { config: "" },
  })

  const cases: [string, string][] = [
    ["*.rs", "src/lib.rs\nsrc/main.rs\nsrc/util/deep.rs"],
    ["src/*.rs", "src/lib.rs\nsrc/main.rs"],
    ["**/*.rs", "src/lib.rs\nsrc/main.rs\nsrc/util/deep.rs"],
    ["src/**/*.rs", "src/lib.rs\nsrc/main.rs\nsrc/util/deep.rs"],
    ["src/**", "src/a.ts\nsrc/b.tsx\nsrc/c1.ts\nsrc/lib.rs\nsrc/main.rs\nsrc/util/deep.rs"],
    ["*.{ts,tsx}", "src/a.ts\nsrc/b.tsx\nsrc/c1.ts"],
    ["src/?.ts", "src/a.ts"],
    ["src/c[0-9].ts", "src/c1.ts"],
    ["src/[!a]*.ts*", "src/b.tsx\nsrc/c1.ts"],
    ["{src,docs}/*.md", "docs/guide.md"],
    ["README.md", "README.md"],
    ["*.js", "no matches"],
    ["**/out.rs", "no matches"],
    ["config", "no matches"],
    ["src/*.{rs", "no matches"],
    ["../*", "no matches"],
    ["**", "Cargo.toml\nREADME.md\ndocs/guide.md\nsrc/a.ts\nsrc/b.tsx\nsrc/c1.ts\nsrc/lib.rs\nsrc/main.rs\nsrc/util/deep.rs"],
  ]

  test.each(cases)("%s", async (pattern, expected) => {
    expect(await run("glob", { pattern })).toMatchObject({ output: expected, is_error: false })
  })

  test("omits a path with a control character", async () => {
    const escaped = "ok.txt"
    const hidden = `weird${String.fromCharCode(10)}.txt`
    const content = { [escaped]: "", [hidden]: "", sibling: "" } as MemoryDirectoryContent
    expect((await setup(content).run("glob", { pattern: "*.txt" })).output).toBe(escaped)
    expect((await setup(content).run("glob", { pattern: "sibling" })).output).toBe("sibling")
  })

  test("caps the output at 100 paths", async () => {
    const files = Object.fromEntries(Array.from({ length: 105 }, (_, index) => [`f${String(index).padStart(3, "0")}.txt`, ""]))
    const lines = (await setup(files).run("glob", { pattern: "*.txt" })).output.split("\n")
    expect(lines).toHaveLength(101)
    expect(lines[0]).toBe("f000.txt")
    expect(lines[99]).toBe("f099.txt")
    expect(lines[100]).toBe("[5 more paths not shown. Narrow the pattern.]")
  })
})

describe("grep", () => {
  const { run } = setup({
    src: { "main.rs": "use std::io;\nfn main() {}\n", "lib.rs": "pub fn lib() {}\n", "app.ts": "function main() {}\n" },
    docs: { "notes.md": "main idea\n" },
    "image.bin": new Uint8Array([109, 97, 105, 110, 0]),
    "root.txt": "main\n",
    node_modules: { "dep.js": "main\n" },
  })

  test("searches the workspace", async () => {
    expect(await run("grep", { pattern: "main" })).toMatchObject({
      output: "docs/notes.md:1|main idea\nsrc/app.ts:1|function main() {}\nsrc/main.rs:2|fn main() {}\nroot.txt:1|main",
      is_error: false,
    })
  })

  test("is case-sensitive", async () => {
    expect((await run("grep", { pattern: "MAIN" })).output).toBe("no matches")
  })

  test("narrows to a directory", async () => {
    expect((await run("grep", { pattern: "fn \\w+", path: "src" })).output).toBe(
      "src/lib.rs:1|pub fn lib() {}\nsrc/main.rs:2|fn main() {}",
    )
  })

  test("narrows to a file", async () => {
    expect((await run("grep", { pattern: "^use", path: "src/main.rs" })).output).toBe("src/main.rs:1|use std::io;")
  })

  test("filters with a glob", async () => {
    expect((await run("grep", { pattern: "main", glob: "*.ts" })).output).toBe("src/app.ts:1|function main() {}")
    expect((await run("grep", { pattern: "main", path: "src", glob: "src/*.rs" })).output).toBe(
      "src/main.rs:2|fn main() {}",
    )
  })

  test("reports a missing path and an invalid pattern", async () => {
    expect(await run("grep", { pattern: "x", path: "nope" })).toMatchObject({
      output: "error: file not found: nope",
      is_error: true,
    })
    expect(await run("grep", { pattern: "(" })).toMatchObject({
      output: "error: Invalid regular expression: /(/: Unterminated group",
      is_error: true,
    })
  })

  test("caps the output at 50 matches", async () => {
    const lines = (await setup({ "hits.txt": numberedLines(60) }).run("grep", { pattern: "line" })).output.split("\n")
    expect(lines).toHaveLength(51)
    expect(lines[49]).toBe("hits.txt:50|line 50")
    expect(lines[50]).toBe("[more matches not shown. Narrow the pattern or the path.]")
  })

  test("cuts a line at 200 characters", async () => {
    const line = "0123456789".repeat(25)
    const output = (await setup({ "wide.txt": line }).run("grep", { pattern: "0" })).output
    expect(output).toBe(`wide.txt:1|${"0123456789".repeat(20)}`)
    expect(output).toHaveLength(211)
  })

  test("omits a file with a control character in the path", async () => {
    const escaped = "visible.txt"
    const hidden = `hidden${String.fromCharCode(10)}.txt`
    const content = {
      [escaped]: "needle\n",
      [hidden]: "needle\n",
      "plain.txt": "needle\n",
    } as MemoryDirectoryContent
    expect((await setup(content).run("grep", { pattern: "needle" })).output).toBe(
      `plain.txt:1|needle\n${escaped}:1|needle`,
    )
  })
})

describe("edit", () => {
  test("replaces one match", async () => {
    const { run, workspace } = setup({ "a.txt": "alpha\nbeta\n" })
    expect(await run("edit", { path: "a.txt", old: "beta", new: "gamma" })).toMatchObject({
      output: "edited a.txt: 1 replacement(s)",
      is_error: false,
    })
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("alpha\ngamma\n")
  })

  test("reports zero and several matches", async () => {
    const { run } = setup({ "a.txt": "x x x" })
    expect(await run("edit", { path: "a.txt", old: "y", new: "z" })).toMatchObject({
      output: "error: old text not found in a.txt",
      is_error: true,
    })
    expect(await run("edit", { path: "a.txt", old: "x", new: "z" })).toMatchObject({
      output: "error: old text matches 3 places in a.txt. Add surrounding lines or set all=true.",
      is_error: true,
    })
  })

  test("replaces all matches", async () => {
    const { run, workspace } = setup({ "a.txt": "x x x" })
    expect((await run("edit", { path: "a.txt", old: "x", new: "z", all: true })).output).toBe(
      "edited a.txt: 3 replacement(s)",
    )
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("z z z")
  })

  test("replaces multi-line text", async () => {
    const { run, workspace } = setup({ "a.txt": "one\ntwo\nthree\n" })
    await run("edit", { path: "a.txt", old: "one\ntwo\n", new: "1\n" })
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("1\nthree\n")
  })

  test("keeps dollar signs in the new text", async () => {
    const { run, workspace } = setup({ "a.txt": "price" })
    await run("edit", { path: "a.txt", old: "price", new: "$& $1 $$ $'" })
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("$& $1 $$ $'")
  })

  test("rejects empty and unchanged text", async () => {
    const { run } = setup({ "a.txt": "a" })
    expect((await run("edit", { path: "a.txt", old: "", new: "b" })).output).toBe("error: old text is empty")
    expect((await run("edit", { path: "a.txt", old: "a", new: "a" })).output).toBe("error: old and new are the same")
  })

  test("refreshes a clean open buffer", async () => {
    const { run, workspace } = setup({ "a.txt": "old" })
    await workspace.openFile("a.txt")
    await run("edit", { path: "a.txt", old: "old", new: "new" })
    expect(workspace.bufferText("a.txt")).toBe("new")
  })
})

describe("write", () => {
  test("creates parent directories", async () => {
    const { run, workspace } = setup({})
    expect(await run("write", { path: "new/deep/file.txt", content: "a\nb\n" })).toMatchObject({
      output: "wrote new/deep/file.txt (2 lines)",
      is_error: false,
    })
    expect(await workspace.fileSystem.readTextFile("new/deep/file.txt")).toBe("a\nb\n")
  })

  test("counts lines", async () => {
    const { run } = setup({})
    expect((await run("write", { path: "a.txt", content: "" })).output).toBe("wrote a.txt (0 lines)")
    expect((await run("write", { path: "b.txt", content: "a\nb" })).output).toBe("wrote b.txt (2 lines)")
  })
})

test("refuses a file with unsaved changes", async () => {
  const { run, workspace } = setup({ "a.txt": "disk" })
  await workspace.openFile("a.txt")
  workspace.setBufferText("a.txt", "buffer")
  const refusal = {
    output: "error: a.txt has unsaved changes in the editor. Ask the user to save or discard them.",
    is_error: true,
  }
  expect(await run("edit", { path: "a.txt", old: "disk", new: "x" })).toMatchObject(refusal)
  expect(await run("write", { path: "a.txt", content: "x" })).toMatchObject(refusal)
  expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("disk")
})

describe("change journal", () => {
  test("reverts an edit", async () => {
    const { host, run, workspace } = setup({ "a.txt": "before" })
    await run("edit", { path: "a.txt", old: "before", new: "after" }, "edit-1")
    expect(host.journal()).toEqual([
      { callId: "edit-1", path: "a.txt", earlier: { kind: "text", text: "before" }, content: "after" },
    ])
    expect(await host.revert("edit-1")).toEqual({ kind: "reverted" })
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("before")
    expect(host.journal()).toEqual([])
  })

  test("reports a conflict when the file changed after the edit", async () => {
    const { host, run, workspace } = setup({ "a.txt": "before" })
    await run("edit", { path: "a.txt", old: "before", new: "after" }, "edit-1")
    await workspace.fileSystem.writeTextFile("a.txt", "user text")
    expect(await host.revert("edit-1")).toEqual({ kind: "conflict" })
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("user text")
  })

  test("deletes a created file", async () => {
    const { host, run, workspace } = setup({})
    await run("write", { path: "dir/new.txt", content: "made" }, "write-1")
    expect(host.journal()[0]?.earlier).toEqual({ kind: "absent" })
    expect(await host.revert("write-1")).toEqual({ kind: "reverted" })
    expect(await workspace.fileSystem.exists("dir/new.txt")).toBe(false)
  })

  test("reports an unknown change", async () => {
    expect(await setup({}).host.revert("nothing")).toEqual({ kind: "unknown_change" })
  })

  test("keeps the latest 200 entries", async () => {
    const { host, run } = setup({})
    for (let index = 0; index < 201; index += 1) await run("write", { path: "a.txt", content: `${index}` }, `w${index}`)
    expect(host.journal()).toHaveLength(200)
    expect(host.journal()[0]?.callId).toBe("w1")
  })

  test("drops the oldest entry when the character total passes the limit", async () => {
    const block = "a".repeat(11)
    const { host, run } = setup({}, { journalCharacterLimit: 30 })
    await run("write", { path: "first.txt", content: block }, "w0")
    await run("write", { path: "second.txt", content: block }, "w1")
    await run("write", { path: "third.txt", content: block }, "w2")
    expect(host.journal().map((entry) => entry.callId)).toEqual(["w1", "w2"])
  })

  test("does not record an entry that alone passes the character limit", async () => {
    const { host, run, workspace } = setup({}, { journalCharacterLimit: 20 })
    await run("write", { path: "big.txt", content: "x".repeat(25) }, "oversize")
    expect(host.journal()).toEqual([])
    expect(await workspace.fileSystem.readTextFile("big.txt")).toBe("x".repeat(25))
    expect(await host.revert("oversize")).toEqual({ kind: "unknown_change" })
  })
})

describe("confirm mode", () => {
  test("writes after approval", async () => {
    const { host, run, workspace } = setup({ "a.txt": "old" }, "confirm")
    const result = run("write", { path: "a.txt", content: "new" }, "write-1")
    await flush()
    expect(host.pendingChanges()).toEqual([
      { callId: "write-1", path: "a.txt", earlier: { kind: "text", text: "old" }, content: "new" },
    ])
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("old")
    host.approve("write-1")
    expect(await result).toMatchObject({ output: "wrote a.txt (1 lines)", is_error: false })
    expect(host.pendingChanges()).toEqual([])
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("new")
  })

  test("answers a decline", async () => {
    const { host, run, workspace } = setup({ "a.txt": "old" }, "confirm")
    const result = run("edit", { path: "a.txt", old: "old", new: "new" }, "edit-1")
    await flush()
    host.decline("edit-1")
    expect(await result).toEqual({
      type: "tool_result",
      call_id: "edit-1",
      output: "error: the user declined this change",
      is_error: true,
    })
    expect(await workspace.fileSystem.readTextFile("a.txt")).toBe("old")
    expect(host.journal()).toEqual([])
  })
})

test("answers without a workspace", async () => {
  const host = createToolHost({ workspace: () => undefined, approval: () => "automatic" })
  expect(await host.execute({ id: "c", name: "read", arguments: { path: "a.txt" } })).toEqual({
    type: "tool_result",
    call_id: "c",
    output: "error: no workspace is open",
    is_error: true,
  })
})

test.each(["delegate", "shell", "toString", "__proto__"])("answers the unknown tool %s", async (name) => {
  expect(await setup({}).run(name, {})).toMatchObject({ output: `error: unknown tool: ${name}`, is_error: true })
})

describe("path traversal", () => {
  const paths = ["../x", "/etc/passwd", "a/../../b", "..\\x", "a\\b"]
  const calls: [string, (path: string) => JsonValue][] = [
    ["read", (path) => ({ path })],
    ["grep", (path) => ({ pattern: "x", path })],
    ["edit", (path) => ({ path, old: "a", new: "b" })],
    ["write", (path) => ({ path, content: "x" })],
  ]

  test.each(calls.flatMap(([tool, args]) => paths.map((path) => [tool, path, args] as const)))(
    "%s rejects %s",
    async (tool, path, args) => {
      const { run, workspace } = setup({ a: { "x.txt": "a" } })
      expect(await run(tool, args(path))).toMatchObject({ output: "error: path outside workspace", is_error: true })
      expect(await workspace.fileSystem.listDirectory("")).toEqual([{ name: "a", kind: "directory" }])
    },
  )

  test("glob matches only workspace paths", async () => {
    expect((await setup({ "x.txt": "" }).run("glob", { pattern: "/etc/*" })).output).toBe("no matches")
  })
})

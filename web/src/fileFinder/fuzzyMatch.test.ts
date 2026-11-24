import { expect, test } from "vitest"
import { fuzzyScore, rankPaths } from "./fuzzyMatch"

const paths = [
  "src/workspace/WorkspaceFileSystem.ts",
  "src/workspace/workspaceStore.ts",
  "src/editor/EditorTabs.tsx",
  "src/fileFinder/fuzzyMatch.ts",
  "README.md",
  "docs/readme-notes.md",
  "src/main.tsx",
]

test.each([
  ["a word-start match in one file name", "wfs", ["src/workspace/WorkspaceFileSystem.ts"]],
  [
    "the tighter word-start match first",
    "ws",
    ["src/workspace/workspaceStore.ts", "src/workspace/WorkspaceFileSystem.ts"],
  ],
  ["the shorter path first on an equal score", "readme", ["README.md", "docs/readme-notes.md"]],
  ["an upper-case query like a lower-case query", "README", ["README.md", "docs/readme-notes.md"]],
  ["a match inside a file name", "tabs", ["src/editor/EditorTabs.tsx"]],
  ["a file name match before a scattered match", "main", ["src/main.tsx"]],
  [
    "a file name start before a later word start",
    "fm",
    ["src/fileFinder/fuzzyMatch.ts", "src/workspace/WorkspaceFileSystem.ts"],
  ],
  ["a directory and file name query", "edtabs", ["src/editor/EditorTabs.tsx"]],
  ["no path for a query that is not a subsequence", "xyz", []],
])("ranks %s", (_case, query, expected) => {
  expect(rankPaths(query, paths, 50)).toEqual(expected)
})

test("ranks a consecutive match above a separated match", () => {
  expect(rankPaths("abc", ["src/a_b_c.ts", "src/abc.ts"], 50)).toEqual(["src/abc.ts", "src/a_b_c.ts"])
})

test("ranks a file name match above a directory name match", () => {
  expect(rankPaths("store", ["store/index.ts", "src/store.ts"], 50)).toEqual(["src/store.ts", "store/index.ts"])
})

test("keeps the given order for an empty query", () => {
  expect(rankPaths("", paths, 3)).toEqual([
    "src/workspace/WorkspaceFileSystem.ts",
    "src/workspace/workspaceStore.ts",
    "src/editor/EditorTabs.tsx",
  ])
})

test("cuts the result at the limit", () => {
  expect(rankPaths("readme", paths, 1)).toEqual(["README.md"])
})

test("gives no score when the query is not a subsequence", () => {
  expect(fuzzyScore("tsx", "src/main.ts")).toBeUndefined()
  expect(fuzzyScore("niam", "src/main.tsx")).toBeUndefined()
})

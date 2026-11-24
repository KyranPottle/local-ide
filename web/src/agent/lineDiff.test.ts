import { expect, test } from "vitest"
import { diffLines, type LineDiff } from "./lineDiff"

const cases: [string, string, string, LineDiff][] = [
  ["identical", "a\nb\n", "a\nb\n", {
    capped: false,
    lines: [
      { kind: "unchanged", text: "a", oldLine: 1, newLine: 1 },
      { kind: "unchanged", text: "b", oldLine: 2, newLine: 2 },
    ],
  }],
  ["created file", "", "x\ny", {
    capped: false,
    lines: [{ kind: "added", text: "x", newLine: 1 }, { kind: "added", text: "y", newLine: 2 }],
  }],
  ["deleted content", "x\n", "", { capped: false, lines: [{ kind: "removed", text: "x", oldLine: 1 }] }],
  ["changed middle line", "a\nb\nc", "a\nB\nc", {
    capped: false,
    lines: [
      { kind: "unchanged", text: "a", oldLine: 1, newLine: 1 },
      { kind: "removed", text: "b", oldLine: 2 },
      { kind: "added", text: "B", newLine: 2 },
      { kind: "unchanged", text: "c", oldLine: 3, newLine: 3 },
    ],
  }],
  ["inserted line", "a\nc", "a\nb\nc", {
    capped: false,
    lines: [
      { kind: "unchanged", text: "a", oldLine: 1, newLine: 1 },
      { kind: "added", text: "b", newLine: 2 },
      { kind: "unchanged", text: "c", oldLine: 2, newLine: 3 },
    ],
  }],
]

test.each(cases)("%s", (_name, before, after, expected) => {
  expect(diffLines(before, after)).toEqual(expected)
})

test("caps each side at 2000 lines", () => {
  const before = Array.from({ length: 2001 }, (_, index) => `${index}`).join("\n")
  const diff = diffLines(before, before)
  expect(diff.capped).toBe(true)
  expect(diff.lines).toHaveLength(2000)
  expect(diff.lines.at(-1)).toEqual({ kind: "unchanged", text: "1999", oldLine: 2000, newLine: 2000 })
})

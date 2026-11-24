import { expect, test } from "vitest"
import { diffLines } from "../agent/lineDiff"
import { diffRows } from "./diffRows"

test("unchanged runs outside the context fold into gaps", () => {
  const before = "a\nb\nc\nd\ne\nf\ng\n"
  const after = "a\nb\nc\nD\ne\nf\ng\n"
  expect(diffRows(diffLines(before, after).lines, 1)).toEqual([
    { kind: "gap", count: 2 },
    { kind: "unchanged", text: "c", oldLine: 3, newLine: 3 },
    { kind: "removed", text: "d", oldLine: 4 },
    { kind: "added", text: "D", newLine: 4 },
    { kind: "unchanged", text: "e", oldLine: 5, newLine: 5 },
    { kind: "gap", count: 2 },
  ])
})

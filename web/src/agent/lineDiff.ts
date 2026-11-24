import { splitLines } from "./textLines"

export const LINE_DIFF_LIMIT = 2000

export type DiffLine =
  | { readonly kind: "unchanged", readonly text: string, readonly oldLine: number, readonly newLine: number }
  | { readonly kind: "removed", readonly text: string, readonly oldLine: number }
  | { readonly kind: "added", readonly text: string, readonly newLine: number }

export type LineDiff = { readonly lines: readonly DiffLine[], readonly capped: boolean }

export function diffLines(before: string, after: string): LineDiff {
  const allOld = splitLines(before)
  const allNew = splitLines(after)
  const oldLines = allOld.slice(0, LINE_DIFF_LIMIT)
  const newLines = allNew.slice(0, LINE_DIFF_LIMIT)
  const capped = allOld.length > LINE_DIFF_LIMIT || allNew.length > LINE_DIFF_LIMIT
  const columns = newLines.length + 1
  // Longest common subsequence lengths of the suffixes, which fit 16 bits at the line limit
  const common = new Uint16Array((oldLines.length + 1) * columns)
  for (let i = oldLines.length - 1; i >= 0; i -= 1) {
    for (let j = newLines.length - 1; j >= 0; j -= 1) {
      common[i * columns + j] = oldLines[i] === newLines[j]
        ? (common[(i + 1) * columns + j + 1] as number) + 1
        : Math.max(common[(i + 1) * columns + j] as number, common[i * columns + j + 1] as number)
    }
  }
  const lines: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < oldLines.length || j < newLines.length) {
    const oldText = oldLines[i]
    const newText = newLines[j]
    if (oldText !== undefined && oldText === newText) {
      lines.push({ kind: "unchanged", text: oldText, oldLine: i + 1, newLine: j + 1 })
      i += 1
      j += 1
      continue
    }
    const preferRemoval = newText === undefined ||
      (oldText !== undefined && (common[(i + 1) * columns + j] as number) >= (common[i * columns + j + 1] as number))
    if (preferRemoval && oldText !== undefined) {
      lines.push({ kind: "removed", text: oldText, oldLine: i + 1 })
      i += 1
      continue
    }
    lines.push({ kind: "added", text: newText as string, newLine: j + 1 })
    j += 1
  }
  return { lines, capped }
}

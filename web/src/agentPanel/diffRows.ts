import type { DiffLine } from "../agent/lineDiff"

export type DiffRow = DiffLine | { readonly kind: "gap", readonly count: number }

export function diffRows(lines: readonly DiffLine[], context: number): DiffRow[] {
  const changed = lines.flatMap((line, index) => (line.kind === "unchanged" ? [] : [index]))
  const near = (index: number) => changed.some((position) => Math.abs(position - index) <= context)
  const rows: DiffRow[] = []
  let gap = 0
  lines.forEach((line, index) => {
    if (line.kind === "unchanged" && !near(index)) {
      gap += 1
      return
    }
    if (gap > 0) rows.push({ kind: "gap", count: gap })
    gap = 0
    rows.push(line)
  })
  if (gap > 0) rows.push({ kind: "gap", count: gap })
  return rows
}

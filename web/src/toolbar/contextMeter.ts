import type { ContextReport } from "../protocol"

export type ContextLevel = "normal" | "warning" | "over"

const WARNING_FRACTION = 0.8

// Rounds down to one decimal so that a count never shows above its true value, 21952 shows as 21.9k
export function formatTokenCount(tokens: number): string {
  if (tokens < 1000) return String(tokens)
  return `${Math.floor(tokens / 100) / 10}k`
}

export const promptLimit = (report: ContextReport) => report.budget - report.reserve

export function contextLevel(report: ContextReport): ContextLevel {
  const fraction = report.total / promptLimit(report)
  if (fraction >= 1) return "over"
  return fraction >= WARNING_FRACTION ? "warning" : "normal"
}

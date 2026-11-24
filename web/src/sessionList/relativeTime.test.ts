import { expect, test } from "vitest"
import { formatRelativeTime } from "./relativeTime"

const now = new Date(2026, 10, 23, 12, 0, 0).getTime()

test.each([
  ["the same instant", now, "now"],
  ["59 seconds ago", now - 59_000, "now"],
  ["a future time", now + 5_000, "now"],
  ["1 minute ago", now - 60_000, "1 min"],
  ["5 minutes 30 seconds ago", now - 330_000, "5 min"],
  ["59 minutes ago", now - 3_540_000, "59 min"],
  ["1 hour ago", now - 3_600_000, "1 h"],
  ["3 hours 59 minutes ago", now - 14_340_000, "3 h"],
  ["23 hours ago", now - 82_800_000, "23 h"],
  ["1 day ago", now - 86_400_000, "1 d"],
  ["6 days ago", now - 518_400_000, "6 d"],
  ["7 days ago", new Date(2026, 10, 16, 12, 0, 0).getTime(), "2026-11-16"],
  ["last year", new Date(2025, 0, 5, 9, 30, 0).getTime(), "2025-01-05"],
])("%s", (_name, at, expected) => {
  expect(formatRelativeTime(at, now)).toBe(expected)
})

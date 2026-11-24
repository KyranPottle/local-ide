import { describe, expect, test } from "vitest"
import type { ContextReport } from "../protocol"
import { contextLevel, formatTokenCount, promptLimit } from "./contextMeter"

const report = (total: number): ContextReport => ({ budget: 12000, reserve: 2000, system: 0, tools: 0, history: total, total })

describe("formatTokenCount", () => {
  test.each([
    [0, "0"],
    [999, "999"],
    [1000, "1k"],
    [7210, "7.2k"],
    [7260, "7.2k"],
    [21952, "21.9k"],
    [21900, "21.9k"],
    [24000, "24k"],
    [128000, "128k"],
  ])("%i", (tokens, expected) => {
    expect(formatTokenCount(tokens)).toBe(expected)
  })
})

describe("contextLevel", () => {
  test.each([
    [0, "normal"],
    [7999, "normal"],
    [8000, "warning"],
    [9999, "warning"],
    [10000, "over"],
    [15000, "over"],
  ] as const)("%i of 10000", (total, expected) => {
    expect(contextLevel(report(total))).toBe(expected)
  })

  test("the limit is the budget without the reply reserve", () => {
    expect(promptLimit({ budget: 24000, reserve: 2048, system: 0, tools: 0, history: 0, total: 0 })).toBe(21952)
  })
})

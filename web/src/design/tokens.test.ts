import { describe, expect, test } from "vitest"
import tokenSheet from "./tokens.css?raw"

type ColorScheme = "light" | "dark"

function tokenColor(token: string, scheme: ColorScheme): string {
  const pair = new RegExp(`--${token}: light-dark\\((#[0-9a-f]{6}), (#[0-9a-f]{6})\\);`).exec(tokenSheet)
  const color = pair?.[scheme === "light" ? 1 : 2]
  if (!color) throw new Error(`tokens.css has no opaque colour pair for --${token}`)
  return color
}

// Relative luminance and contrast ratio of WCAG 2.2
function luminance(color: string): number {
  const [red, green, blue] = [1, 3, 5].map((offset) => {
    const channel = Number.parseInt(color.slice(offset, offset + 2), 16) / 255
    return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4
  })
  return 0.2126 * (red ?? 0) + 0.7152 * (green ?? 0) + 0.0722 * (blue ?? 0)
}

function contrast(foreground: string, background: string): number {
  const [darker, lighter] = [luminance(foreground), luminance(background)].sort((a, b) => a - b)
  return ((lighter ?? 0) + 0.05) / ((darker ?? 0) + 0.05)
}

const schemes: ColorScheme[] = ["light", "dark"]
const surfaces = ["surface-base", "surface-raised", "surface-overlay", "surface-hover", "surface-selected"]
const readableOnBase = [
  "accent",
  "error",
  "warning",
  "success",
  "syntax-keyword",
  "syntax-string",
  "syntax-constant",
  "syntax-function",
  "syntax-type",
  "syntax-tag",
]

describe.each(schemes)("%s scheme", (scheme) => {
  test.each(surfaces)("body text has a contrast of 7 or more on %s", (surface) => {
    expect(contrast(tokenColor("text-primary", scheme), tokenColor(surface, scheme))).toBeGreaterThanOrEqual(7)
  })

  test.each(surfaces)("secondary and tertiary text have a contrast of 4.5 or more on %s", (surface) => {
    expect(contrast(tokenColor("text-secondary", scheme), tokenColor(surface, scheme))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(tokenColor("text-tertiary", scheme), tokenColor(surface, scheme))).toBeGreaterThanOrEqual(4.5)
  })

  test.each(readableOnBase)("%s has a contrast of 4.5 or more on the base surface", (token) => {
    expect(contrast(tokenColor(token, scheme), tokenColor("surface-base", scheme))).toBeGreaterThanOrEqual(4.5)
  })

  test("primary button text has a contrast of 4.5 or more on the button", () => {
    const text = tokenColor("accent-on-surface", scheme)
    expect(contrast(text, tokenColor("accent-surface", scheme))).toBeGreaterThanOrEqual(4.5)
    expect(contrast(text, tokenColor("accent-surface-hover", scheme))).toBeGreaterThanOrEqual(4.5)
  })

  test("no surface or text colour is pure black or pure white", () => {
    const colors = [...surfaces, "text-primary", "accent-on-surface"].map((token) => tokenColor(token, scheme))
    expect(colors).not.toContain("#000000")
    expect(colors).not.toContain("#ffffff")
  })
})

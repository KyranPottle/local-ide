import { createSignal } from "solid-js"
import { readPreference, writePreference } from "../preferences/storedPreference"

type ColorScheme = "dark" | "light"

const PREFERENCE_KEY = "local-ide.color-scheme"

function initialColorScheme(): ColorScheme {
  const stored = readPreference(PREFERENCE_KEY)
  if (stored === "dark" || stored === "light") return stored
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark"
}

const [colorScheme, setColorScheme] = createSignal(initialColorScheme())
document.documentElement.dataset.colorScheme = colorScheme()

export { colorScheme }

export function toggleColorScheme(): void {
  const next = colorScheme() === "dark" ? "light" : "dark"
  writePreference(PREFERENCE_KEY, next)
  document.documentElement.dataset.colorScheme = next
  setColorScheme(next)
}

import { onCleanup } from "solid-js"

export function registerCommandShortcuts(shortcuts: Readonly<Record<string, () => void>>): void {
  const onKeyDown = (event: KeyboardEvent) => {
    if (!event.metaKey || event.ctrlKey || event.altKey || event.shiftKey) return
    const key = event.key.toLowerCase()
    if (!Object.hasOwn(shortcuts, key)) return
    event.preventDefault()
    event.stopPropagation()
    shortcuts[key]?.()
  }
  // The capture phase and the stop keep the key from the editor key bindings and from the focused control
  window.addEventListener("keydown", onKeyDown, { capture: true })
  onCleanup(() => window.removeEventListener("keydown", onKeyDown, { capture: true }))
}

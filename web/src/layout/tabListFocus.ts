export function moveTabFocus(event: KeyboardEvent & { currentTarget: HTMLElement }): void {
  const tabs = [...event.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')]
  const current = tabs.findIndex((tab) => tab === document.activeElement)
  const targets: Record<string, HTMLElement | undefined> = {
    ArrowLeft: tabs[(current - 1 + tabs.length) % tabs.length],
    ArrowRight: tabs[(current + 1) % tabs.length],
    Home: tabs[0],
    End: tabs[tabs.length - 1],
  }
  if (current === -1 || !Object.hasOwn(targets, event.key)) return
  event.preventDefault()
  targets[event.key]?.focus()
}

// Chrome and Brave throw on each localStorage access when the user blocks site data

export function readPreference(key: string): string | undefined {
  try {
    return localStorage.getItem(key) ?? undefined
  } catch {
    return undefined
  }
}

export function writePreference(key: string, value: string): void {
  try {
    localStorage.setItem(key, value)
  } catch {
    // The preference then lasts for this page load only
  }
}

export const BRAVE_FLAG_ADDRESS = "brave://flags/#file-system-access-api"

export const BRAVE_FLAG_HEADING = "Brave has local folder access turned off."

export const BRAVE_FLAG_STEPS = [
  "Open this address in a new tab:",
  "Set File System Access API to Enabled.",
  "Relaunch Brave and load this page again.",
] as const

export const UNAVAILABLE_NOTICE = "This IDE needs Chrome, Edge, or Brave, because this browser cannot open a local folder."

// Resolves to false when the clipboard refuses the write, so the address stays for manual selection
export async function copyText(text: string, clipboard: Pick<Clipboard, "writeText"> | undefined): Promise<boolean> {
  if (!clipboard) return false
  try {
    await clipboard.writeText(text)
    return true
  } catch {
    return false
  }
}

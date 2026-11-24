import { type Browser, type BrowserContext, expect, type Page } from "@playwright/test"
import type { Session, SessionDetail } from "../src/protocol"

export type ScriptedReply = { readonly text: string, readonly reasoning?: string, readonly chunkDelayMilliseconds?: number }
export type ChatMessage = { readonly role: string, readonly content: string }
export type RecordedRequest = { readonly model: string, readonly messages: readonly ChatMessage[] }
export type MockCounters = { readonly activeRequests: number, readonly maxConcurrentRequests: number, readonly disconnectedRequests: number }

export const origin = () => process.env.E2E_ORIGIN as string
const mockUrl = () => process.env.E2E_MOCK_URL as string

export async function queueReplies(replies: readonly ScriptedReply[]): Promise<void> {
  const response = await fetch(`${mockUrl()}/control/replies`, { method: "POST", body: JSON.stringify(replies) })
  expect(response.status).toBe(204)
}

export async function recordedRequests(): Promise<RecordedRequest[]> {
  return (await (await fetch(`${mockUrl()}/control/requests`)).json()) as RecordedRequest[]
}

export async function mockCounters(): Promise<MockCounters> {
  return (await (await fetch(`${mockUrl()}/control/state`)).json()) as MockCounters
}

export async function resetMock(): Promise<void> {
  const response = await fetch(`${mockUrl()}/control/reset`, { method: "POST" })
  expect(response.status).toBe(204)
}

export async function listSessions(query = ""): Promise<Session[]> {
  return (await (await fetch(`${origin()}/api/sessions?q=${encodeURIComponent(query)}`)).json()) as Session[]
}

export async function sessionDetail(id: string): Promise<SessionDetail> {
  return (await (await fetch(`${origin()}/api/sessions/${id}`)).json()) as SessionDetail
}

// The engine accepts a mutating request only with its own origin
export async function deleteAllSessions(): Promise<void> {
  const roots = (await listSessions()).filter((session) => session.origin.kind !== "delegate")
  for (const session of roots) {
    const response = await fetch(`${origin()}/api/sessions/${session.id}`, { method: "DELETE", headers: { Origin: origin() } })
    expect(response.status).toBe(204)
  }
}

export async function newContext(browser: Browser): Promise<BrowserContext> {
  return browser.newContext({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" })
}

// Files written before the first open replace the default scratch files
export async function seedScratchFiles(page: Page, files: Readonly<Record<string, string>>): Promise<void> {
  await page.evaluate(async (entries) => {
    const root = await navigator.storage.getDirectory()
    const scratch = await root.getDirectoryHandle("scratch", { create: true })
    for (const [path, text] of Object.entries(entries)) {
      const names = path.split("/")
      let directory = scratch
      for (const name of names.slice(0, -1)) directory = await directory.getDirectoryHandle(name, { create: true })
      const file = await directory.getFileHandle(names.at(-1) as string, { create: true })
      const writable = await file.createWritable()
      await writable.write(text)
      await writable.close()
    }
  }, files)
}

export async function scratchFileExists(page: Page, path: string): Promise<boolean> {
  return page.evaluate(async (name) => {
    const scratch = await (await navigator.storage.getDirectory()).getDirectoryHandle("scratch")
    return scratch.getFileHandle(name).then(() => true, () => false)
  }, path)
}

export async function openScratch(page: Page, files?: Readonly<Record<string, string>>): Promise<void> {
  await page.goto(origin())
  if (files) await seedScratchFiles(page, files)
  await page.getByRole("button", { name: "Scratch workspace" }).click()
  await expect(page.locator(".status-bar")).toContainText("Connected")
}

export const transcriptOf = (page: Page) => page.getByRole("log", { name: "Transcript" })

// The engine echoes the user event after the run starts, so a later idle check cannot pass before the run
export async function submitComposer(page: Page): Promise<void> {
  const composer = page.getByRole("textbox", { name: "Message" })
  const text = await composer.inputValue()
  await composer.press("Enter")
  await expect(transcriptOf(page).locator(".message-user-text").filter({ hasText: text })).not.toHaveCount(0)
}

export async function send(page: Page, text: string): Promise<void> {
  await page.getByRole("textbox", { name: "Message" }).fill(text)
  await submitComposer(page)
}

export async function waitForIdle(page: Page): Promise<void> {
  await expect(page.locator(".status-bar")).toContainText("Idle")
  await expect(page.getByRole("button", { name: "Send", exact: true })).toBeVisible()
}

export async function chooseModel(page: Page, label: RegExp): Promise<void> {
  await page.getByRole("button", { name: /^Model: / }).click()
  await page.getByRole("menuitemradio", { name: label }).click()
}

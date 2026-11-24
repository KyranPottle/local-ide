import { expect, type Page, test } from "@playwright/test"
import { deleteAllSessions, openScratch, queueReplies, recordedRequests, resetMock, send, submitComposer, waitForIdle } from "./stackControl"
import { toolCallText } from "./textToolCodec.mjs"

const screenshotDirectory = new URL("../screenshots/", import.meta.url).pathname

const FIRST_MESSAGE = "Fix the first task so that it takes 25 minutes"

async function setColorScheme(page: Page, scheme: "dark" | "light"): Promise<void> {
  await page.evaluate((value) => {
    document.documentElement.dataset.colorScheme = value
  }, scheme)
}

test.describe.serial("agent panel", () => {
  let page: Page

  test.beforeAll(async ({ browser }) => {
    await resetMock()
    await deleteAllSessions()
    page = await browser.newPage({ viewport: { width: 1440, height: 900 }, reducedMotion: "reduce" })
    await openScratch(page)
  })

  test.afterAll(async () => {
    await page.close()
  })

  test("a run reads, edits, answers, and reverts", async () => {
    await queueReplies([
      { text: `I will read the notes first.\n${toolCallText("read", { path: "README.md" })}`, reasoning: "The user wants a change in the task list." },
      { text: toolCallText("edit", { path: "src/example.ts", old: "minutes: 20", new: "minutes: 25" }) },
      { text: "## Done\n\nThe first task now takes **25 minutes**.\n\n- Read `README.md`\n- Edited one line in `src/example.ts`" },
    ])
    await send(page, FIRST_MESSAGE)
    await waitForIdle(page)

    const transcript = page.getByRole("log", { name: "Transcript" })
    const toolRows = transcript.locator(".tool-row")
    await expect(toolRows).toHaveCount(2)
    await expect(toolRows.nth(0)).toContainText("read")
    await expect(toolRows.nth(0)).toContainText("README.md")
    await expect(toolRows.nth(1)).toContainText("edit")
    await expect(toolRows.nth(1)).toContainText("src/example.ts")

    await expect(transcript.locator(".diff-line-removed")).toContainText("minutes: 20")
    await expect(transcript.locator(".diff-line-added")).toContainText("minutes: 25")
    await expect(transcript.getByRole("heading", { name: "Done" })).toBeVisible()
    await expect(transcript.locator("strong")).toHaveText("25 minutes")

    await toolRows.nth(1).getByRole("button", { name: "src/example.ts", exact: true }).click()
    const editor = page.locator(".cm-content")
    await expect(editor).toContainText("minutes: 25, done: true")

    const meter = page.getByTestId("context-meter-text")
    await expect(meter).toHaveText(/^\d+(\.\d)?k? \/ 15\.8k$/)
    await meter.click()
    const breakdown = page.getByRole("dialog", { name: "Context" })
    await expect(breakdown).toContainText("Reply reserve8,192")
    await expect(breakdown).toContainText("Budget24,000")
    await page.keyboard.press("Escape")

    await page.mouse.move(0, 0)
    await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur())
    await setColorScheme(page, "dark")
    await page.screenshot({ path: `${screenshotDirectory}agent-dark.png` })
    await setColorScheme(page, "light")
    await page.screenshot({ path: `${screenshotDirectory}agent-light.png` })
    await setColorScheme(page, "dark")

    await transcript.getByRole("button", { name: "Revert" }).click()
    await expect(transcript.getByRole("status").filter({ hasText: "Reverted" })).toBeVisible()
    await expect(editor).toContainText("minutes: 20, done: true")
  })

  test("a model change reaches the next request with the full history", async () => {
    await page.getByRole("button", { name: /^Model: / }).click()
    await page.getByRole("menuitemradio", { name: /Fast/ }).click()
    const transcript = page.getByRole("log", { name: "Transcript" })
    await expect(transcript).toContainText("Model changed: Coder to Fast")

    await queueReplies([{ text: "Only the first task changed, and the change is reverted." }])
    await send(page, "What changed?")
    await waitForIdle(page)
    await expect(transcript).toContainText("Only the first task changed")

    const last = (await recordedRequests()).at(-1)
    expect(last?.model).toBe("fast")
    expect(JSON.stringify(last?.messages)).toContain(FIRST_MESSAGE)

  })

  test("edit and resend starts a new session and keeps the first one", async () => {
    const transcript = page.getByRole("log", { name: "Transcript" })
    const firstMessage = transcript.locator(".message-user").first()
    await firstMessage.hover()
    await firstMessage.getByRole("button", { name: "Edit and resend" }).click()
    await expect(page.getByRole("textbox", { name: "Message" })).toHaveValue(FIRST_MESSAGE)

    await queueReplies([{ text: "Edited answer." }])
    await submitComposer(page)
    await waitForIdle(page)
    await expect(transcript).toContainText("Edited answer.")

    await page.getByRole("tab", { name: "Sessions" }).click()
    const rows = page.getByRole("list", { name: "Sessions" }).locator(".session-row-select")
    await expect(rows).toHaveCount(2)
    await rows.nth(1).click()
    await expect(transcript).toContainText("Only the first task changed")
    await expect(transcript.locator(".tool-row")).toHaveCount(2)
  })
})

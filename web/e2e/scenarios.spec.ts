import { type BrowserContext, expect, type Page, test } from "@playwright/test"
import {
  chooseModel,
  deleteAllSessions,
  listSessions,
  mockCounters,
  newContext,
  openScratch,
  origin,
  queueReplies,
  recordedRequests,
  resetMock,
  scratchFileExists,
  send,
  sessionDetail,
  transcriptOf,
  waitForIdle,
} from "./stackControl"
import { toolCallText } from "./textToolCodec.mjs"

const SLOW_CHUNK_DELAY_MILLISECONDS = 40
const SLOW_TEXT = "Streaming word ".repeat(160)
const PATH_OUTSIDE_WORKSPACE = "error: path outside workspace"

let context: BrowserContext
let page: Page

test.beforeEach(async ({ browser }) => {
  await resetMock()
  await deleteAllSessions()
  context = await newContext(browser)
  page = await context.newPage()
})

test.afterEach(async () => {
  await context.close()
})

const requestText = (index: number, requests: readonly { messages: unknown }[]) => JSON.stringify(requests[index]?.messages)

test("a delegate child runs between two primary requests", async () => {
  await openScratch(page)
  await queueReplies([
    { text: toolCallText("delegate", { task: "Read README.md and give its first heading", model: "fast" }) },
    { text: toolCallText("read", { path: "README.md" }) },
    { text: "The first heading is **Scratch workspace**." },
    { text: "The sub-agent found the heading Scratch workspace." },
  ])
  await send(page, "Ask a sub-agent for the README heading")
  await waitForIdle(page)

  const requests = await recordedRequests()
  expect(requests.map(({ model }) => model)).toEqual(["coder", "fast", "fast", "coder"])
  expect((await mockCounters()).maxConcurrentRequests).toBe(1)
  expect(requests[0]?.messages[0]?.content).toContain("delegate(task, model?)")
  expect(requests[1]?.messages[0]?.content).not.toContain("delegate(")

  const transcript = transcriptOf(page)
  const delegate = transcript.locator(".delegate")
  await expect(delegate.locator(".delegate-model")).toHaveText("Fast")
  await delegate.locator(".delegate-header").click()
  const child = delegate.locator(".delegate-body")
  await expect(child.locator(".tool-row")).toContainText("README.md")
  await expect(child.locator(".message-assistant")).toContainText("The first heading is Scratch workspace.")
  await expect(child.locator(".message-usage")).toContainText("Fast")
  await expect(transcript).toContainText("The sub-agent found the heading Scratch workspace.")

  const sessions = await listSessions()
  const parent = sessions.find(({ origin: sessionOrigin }) => sessionOrigin.kind === "root")
  const delegated = sessions.find(({ origin: sessionOrigin }) => sessionOrigin.kind === "delegate")
  expect(delegated?.model).toBe("fast")
  expect(delegated?.origin).toMatchObject({ kind: "delegate", parent: parent?.id })
})

test("sessions switch, survive a reload, match a search, rename, and delete", async () => {
  await openScratch(page)
  await queueReplies([{ text: "Alpha answer." }, { text: "Zeppelin answer." }])
  await send(page, "First topic about alpha")
  await waitForIdle(page)
  await page.locator(".agent-header").getByRole("button", { name: "New session" }).click()
  await send(page, "Second topic about zeppelin")
  await waitForIdle(page)

  const transcript = transcriptOf(page)
  const rows = page.getByRole("list", { name: "Sessions" }).locator(".session-row")
  async function selectAndCheck(title: string, answer: string, absent: string): Promise<void> {
    await rows.filter({ hasText: title }).locator(".session-row-select").click()
    await expect(transcript).toContainText(answer)
    await expect(transcript).not.toContainText(absent)
  }

  await page.getByRole("tab", { name: "Sessions" }).click()
  await expect(rows).toHaveCount(2)
  await selectAndCheck("First topic about alpha", "Alpha answer.", "Zeppelin")
  await selectAndCheck("Second topic about zeppelin", "Zeppelin answer.", "Alpha")

  await page.reload()
  await page.getByRole("button", { name: "Scratch workspace" }).click()
  await expect(page.locator(".status-bar")).toContainText("Connected")
  await page.getByRole("tab", { name: "Sessions" }).click()
  await expect(rows).toHaveCount(2)
  await expect(rows.nth(0)).toContainText("Second topic about zeppelin")
  await expect(rows.nth(1)).toContainText("First topic about alpha")
  await selectAndCheck("First topic about alpha", "Alpha answer.", "Zeppelin")
  await selectAndCheck("Second topic about zeppelin", "Zeppelin answer.", "Alpha")

  const search = page.getByRole("searchbox", { name: "Search sessions" })
  await search.fill("zeppelin")
  await expect(rows).toHaveCount(1)
  await expect(rows).toContainText("Second topic about zeppelin")
  await search.fill("")
  await expect(rows).toHaveCount(2)

  const first = rows.filter({ hasText: "First topic about alpha" })
  await first.hover()
  await first.getByRole("button", { name: "Rename" }).click()
  const titleInput = page.getByRole("list", { name: "Sessions" }).getByRole("textbox", { name: "Session title" })
  await titleInput.fill("Renamed alpha")
  await titleInput.press("Enter")
  await expect(rows.filter({ hasText: "Renamed alpha" })).toHaveCount(1)
  expect((await listSessions()).map(({ title }) => title).sort()).toEqual(["Renamed alpha", "Second topic about zeppelin"])

  const second = rows.filter({ hasText: "Second topic about zeppelin" })
  await second.hover()
  await second.getByRole("button", { name: "Delete" }).click()
  await page.getByRole("group", { name: "Delete session" }).getByRole("button", { name: "Delete" }).click()
  await expect(rows).toHaveCount(1)
  await expect(rows).toContainText("Renamed alpha")
  expect((await listSessions()).map(({ title }) => title)).toEqual(["Renamed alpha"])
})

test("a fork from the first answer copies the events through that answer", async () => {
  await openScratch(page)
  await queueReplies([{ text: "First answer." }, { text: "Second answer." }, { text: "Fork answer." }])
  await send(page, "Turn one")
  await waitForIdle(page)
  await send(page, "Turn two")
  await waitForIdle(page)
  const [parent] = await listSessions()

  const transcript = transcriptOf(page)
  const firstAnswer = transcript.locator(".message-assistant").first()
  await firstAnswer.hover()
  await firstAnswer.getByRole("button", { name: "Fork from here" }).click()
  await expect(transcript).not.toContainText("Second answer.")
  await expect(transcript).toContainText("First answer.")
  await expect(transcript.locator(".message-user")).toHaveCount(1)

  const fork = (await listSessions()).find(({ origin: sessionOrigin }) => sessionOrigin.kind === "fork")
  expect(fork?.origin).toEqual({ kind: "fork", parent: parent?.id, seq: 2 })
  const forkId = fork?.id as string
  expect((await sessionDetail(forkId)).events.map(({ body }) => body.type)).toEqual(["user", "assistant"])

  await send(page, "Turn in the fork")
  await waitForIdle(page)
  await expect(transcript).toContainText("Fork answer.")
  const parentEvents = (await sessionDetail(parent?.id as string)).events
  expect(parentEvents.map(({ body }) => body.type)).toEqual(["user", "assistant", "user", "assistant"])
  expect(parentEvents.at(-1)?.body).toMatchObject({ text: "Second answer." })
  expect((await sessionDetail(forkId)).events.at(-1)?.body).toMatchObject({ text: "Fork answer." })
})

test("confirm mode declines one write and applies the next", async () => {
  await openScratch(page)
  await page.getByRole("button", { name: "Agent settings" }).click()
  await page.getByRole("radio", { name: "Confirm each change" }).click()
  await page.keyboard.press("Escape")
  await queueReplies([
    { text: toolCallText("write", { path: "notes.txt", content: "first draft\n" }) },
    { text: toolCallText("write", { path: "notes.txt", content: "approved text\n" }) },
    { text: "notes.txt holds the approved text." },
  ])
  await send(page, "Create notes.txt")

  const transcript = transcriptOf(page)
  await expect(transcript.locator(".diff-line-added")).toHaveText(["1first draft"])
  await expect(transcript.getByRole("button", { name: "Apply" })).toHaveCount(1)
  await transcript.getByRole("button", { name: "Decline" }).click()

  await expect(transcript.locator(".diff-line-added")).toHaveText(["1approved text"])
  expect(await scratchFileExists(page, "notes.txt")).toBe(false)
  expect(requestText(1, await recordedRequests())).toContain("error: the user declined this change")
  await transcript.getByRole("button", { name: "Apply" }).click()
  await waitForIdle(page)
  await expect(transcript).toContainText("notes.txt holds the approved text.")

  const tree = page.getByRole("tree", { name: "Files" })
  await tree.getByText("notes.txt", { exact: true }).click()
  await expect(page.locator(".cm-content")).toHaveText("approved text")
})

test("stop ends a slow stream, keeps the partial text, and frees the run lock", async () => {
  await openScratch(page)
  await queueReplies([{ text: SLOW_TEXT, chunkDelayMilliseconds: SLOW_CHUNK_DELAY_MILLISECONDS }, { text: "Complete answer after the stop." }])
  await send(page, "Write a long reply")
  const transcript = transcriptOf(page)
  await expect(transcript).toContainText("Streaming word Streaming word")
  await page.getByRole("button", { name: "Stop" }).click()

  await expect(transcript.locator(".run-ending")).toHaveText("Stopped")
  await expect(transcript.locator(".message-assistant").first()).toContainText("Streaming word")
  await expect.poll(async () => (await mockCounters()).disconnectedRequests).toBe(1)

  await send(page, "Answer again")
  await waitForIdle(page)
  await expect(transcript).toContainText("Complete answer after the stop.")
  expect((await recordedRequests()).length).toBe(2)
})

test("compaction elides early tool results under a tiny budget", async () => {
  const longLines = Array.from({ length: 60 }, (_, index) => `Long line ${String(index + 1).padStart(3, "0")} ${"x".repeat(80)}`)
  await openScratch(page, {
    "big.txt": `${longLines.join("\n")}\n`,
    "a.txt": "alpha\n",
    "b.txt": "beta\n",
    "c.txt": "gamma\n",
  })
  await chooseModel(page, /Compact Test/)
  await page.evaluate(() => {
    const record = window as unknown as { meterStates: string[] }
    record.meterStates = []
    new MutationObserver(() => {
      if (!document.body.textContent?.includes("Earlier tool results elided")) return
      const bar = document.querySelector(".context-meter-bar")?.className ?? ""
      const text = document.querySelector("[data-testid=context-meter-text]")?.textContent ?? ""
      record.meterStates.push(`${bar.includes("context-meter-over") ? "over" : "within"} ${text}`)
    }).observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true })
  })
  await queueReplies([
    { text: toolCallText("read", { path: "big.txt" }) },
    { text: toolCallText("read", { path: "a.txt" }) },
    { text: toolCallText("read", { path: "b.txt" }) },
    { text: toolCallText("read", { path: "c.txt" }) },
    { text: "I read four files." },
  ])
  await send(page, "Read the files")
  await waitForIdle(page)

  const transcript = transcriptOf(page)
  await expect(transcript.locator(".transcript-marker")).toHaveText(["Earlier tool results elided"])
  await expect(transcript).toContainText("I read four files.")
  await expect(page.locator(".context-meter-bar")).not.toHaveClass(/context-meter-over/)
  // The compaction event arrives one frame before its context report, so the first states show the report of the earlier request
  const meterStates = await page.evaluate(() => (window as unknown as { meterStates: string[] }).meterStates)
  const afterReport = meterStates.slice(meterStates.findIndex((state) => state !== meterStates[0]))
  expect(meterStates[0]).toBe("over 2.2k / 2k")
  expect(afterReport.length).toBeGreaterThan(0)
  expect(afterReport.filter((state) => state.startsWith("over"))).toEqual([])

  const requests = await recordedRequests()
  expect(requests.length).toBe(5)
  expect(requestText(3, requests)).toContain("Long line 001")
  expect(requestText(4, requests)).toContain("[elided result of read {\\\"path\\\":\\\"big.txt\\\"}")
  expect(requestText(4, requests)).not.toContain("Long line 001")
})

test("a second page is blocked while the first page runs", async () => {
  const second = await context.newPage()
  await openScratch(page)
  await openScratch(second)
  await queueReplies([{ text: SLOW_TEXT, chunkDelayMilliseconds: SLOW_CHUNK_DELAY_MILLISECONDS / 4 }, { text: "Page two answer." }])
  await send(page, "Slow reply on page one")
  await expect(transcriptOf(page)).toContainText("Streaming word")

  const secondComposer = second.getByRole("textbox", { name: "Message" })
  await secondComposer.fill("Message from page two")
  await secondComposer.press("Enter")
  await expect(second.locator(".composer-block")).toContainText("Another page runs")
  expect((await recordedRequests()).length).toBe(1)

  await waitForIdle(page)
  await expect(second.locator(".composer-block")).toHaveCount(0, { timeout: 10_000 })
  await send(second, "Message from page two again")
  await waitForIdle(second)
  await expect(transcriptOf(second)).toContainText("Page two answer.")
  expect((await recordedRequests()).length).toBe(2)
  expect((await mockCounters()).maxConcurrentRequests).toBe(1)
})

test("hostile names and Markdown stay inert text", async () => {
  const hostileName = "<img src=x onerror=window.__injected=1>.txt"
  const requestedUrls: string[] = []
  context.on("request", (request) => requestedUrls.push(request.url()))
  await openScratch(page, { [hostileName]: "plain text\n" })
  await queueReplies([
    { text: toolCallText("read", { path: hostileName }) },
    {
      text: [
        "Here is the result.",
        "",
        "<img src=x onerror=\"window.__injected=1\">",
        "",
        "<script>window.__injected=1</script>",
        "",
        "[link](javascript:window.__injected=1)",
      ].join("\n"),
    },
  ])
  await send(page, "Read the odd file")
  await waitForIdle(page)

  const transcript = transcriptOf(page)
  await expect(transcript).toContainText("Here is the result.")
  await expect(transcript.locator(".tool-row")).toContainText(hostileName)
  await expect(page.getByRole("tree", { name: "Files" }).getByText(hostileName, { exact: true })).toBeVisible()
  await expect(transcript).toContainText("<script>window.__injected=1</script>")
  expect(await page.evaluate(() => (window as unknown as { __injected?: unknown }).__injected)).toBeUndefined()
  await expect(transcript.locator("img, script")).toHaveCount(0)
  await expect(page.locator("a[href^='javascript:']")).toHaveCount(0)
  await expect(transcript.locator("a")).toHaveCount(0)

  const engineHost = new URL(origin()).host
  const foreign = requestedUrls.filter((url) => /^(https?|wss?):/.test(url) && new URL(url).host !== engineHost)
  expect(foreign).toEqual([])
  expect(requestedUrls.length).toBeGreaterThan(0)
})

test("paths outside the workspace fail through the full stack", async () => {
  await openScratch(page)
  await queueReplies([
    { text: toolCallText("read", { path: "../../etc/passwd" }) },
    { text: toolCallText("write", { path: "/tmp/local-ide-escape.txt", content: "escape\n" }) },
    { text: "Both paths are outside the workspace." },
  ])
  await send(page, "Read and write outside the folder")
  await waitForIdle(page)

  const transcript = transcriptOf(page)
  const tools = transcript.locator(".transcript-tool")
  await expect(tools).toHaveCount(2)
  for (const index of [0, 1]) {
    const tool = tools.nth(index)
    await expect(tool.locator(".tool-state-error")).toHaveCount(1)
    await tool.locator(".tool-row-toggle").click()
    await expect(tool.locator(".tool-output-error")).toHaveText(PATH_OUTSIDE_WORKSPACE)
  }
  await expect(tools.nth(0)).toContainText("../../etc/passwd")
  await expect(tools.nth(1)).toContainText("/tmp/local-ide-escape.txt")

  const requests = await recordedRequests()
  expect(requestText(1, requests)).toContain(PATH_OUTSIDE_WORKSPACE)
  expect(requestText(2, requests)).toContain(PATH_OUTSIDE_WORKSPACE)
})

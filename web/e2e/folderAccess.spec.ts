import { expect, test } from "@playwright/test"
import { newContext, origin } from "./stackControl"

test("Brave without the File System Access API shows the flag instruction", async ({ browser }) => {
  const context = await newContext(browser)
  await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: origin() })
  await context.addInitScript(() => {
    delete (window as Partial<Window>).showDirectoryPicker
    delete (Window.prototype as Partial<Window>).showDirectoryPicker
    Object.defineProperty(Navigator.prototype, "brave", { value: { isBrave: () => Promise.resolve(true) } })
  })
  const page = await context.newPage()
  await page.goto(origin())

  const notice = page.locator(".welcome-brave")
  await expect(notice.getByRole("heading")).toHaveText("Brave has local folder access turned off.")
  await expect(notice.locator("li")).toHaveText([
    /Open this address in a new tab:\s*brave:\/\/flags\/#file-system-access-api/,
    "Set File System Access API to Enabled.",
    "Relaunch Brave and load this page again.",
  ])
  await expect(page.getByRole("button", { name: "Open folder" })).toBeDisabled()
  await expect(page.getByRole("button", { name: "Scratch workspace" })).toBeEnabled()

  await notice.getByRole("button", { name: "Copy" }).click()
  await expect(notice.getByRole("status")).toHaveText("Copied.")
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("brave://flags/#file-system-access-api")
  await context.close()
})

test("another browser without the File System Access API names the supported browsers", async ({ browser }) => {
  const context = await newContext(browser)
  await context.addInitScript(() => {
    delete (window as Partial<Window>).showDirectoryPicker
    delete (Window.prototype as Partial<Window>).showDirectoryPicker
  })
  const page = await context.newPage()
  await page.goto(origin())

  await expect(page.getByRole("alert")).toHaveText(
    "This IDE needs Chrome, Edge, or Brave, because this browser cannot open a local folder.",
  )
  await expect(page.locator(".welcome-brave")).toHaveCount(0)
  await expect(page.getByRole("button", { name: "Open folder" })).toBeDisabled()
  await context.close()
})

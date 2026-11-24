import { defineConfig, devices } from "@playwright/test"

export default defineConfig({
  testDir: "e2e",
  globalSetup: "./e2e/launchStack.mjs",
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  reporter: "list",
  use: {
    ...devices["Desktop Chrome"],
    viewport: { width: 1440, height: 900 },
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium" }],
})

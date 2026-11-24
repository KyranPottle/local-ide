import { startStack } from "./launchStack.mjs"

const DEMO_PORT = 7878
const DEMO_PROFILES = [
  { id: "demo-primary", label: "Demo responder A", role: "Rule-based demo, not a real model", context_budget: 16000, reply_reserve: 2048 },
  { id: "demo-secondary", label: "Demo responder B", role: "Second demo id for the model picker", context_budget: 12000, reply_reserve: 2048 },
]

const stack = await startStack({
  responder: "demo",
  mockModels: DEMO_PROFILES.map(({ id }) => id),
  exampleProfiles: false,
  profiles: DEMO_PROFILES,
  preferredPort: DEMO_PORT,
})

let stopping = false
async function shutDown() {
  if (stopping) return
  stopping = true
  console.log("\nStopping the local demo")
  await stack.stop()
  process.exit(0)
}

process.on("SIGINT", shutDown)
process.on("SIGTERM", shutDown)
console.log(`\nlocal-ide demo: ${stack.origin}`)
console.log("Open this URL in Chrome or Brave. Press Ctrl+C to stop.")

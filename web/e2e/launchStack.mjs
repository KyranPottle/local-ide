// Stop closes only the mock and the engine
// The default export is the Playwright global setup, and the demo command uses startStack
import { execFileSync, spawn } from "node:child_process"
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs"
import { createServer } from "node:net"
import { tmpdir } from "node:os"
import { join, resolve } from "node:path"
import { startMockInferenceServer } from "./mockInferenceServer.mjs"

const repositoryRoot = resolve(import.meta.dirname, "../..")
const HEALTH_TIMEOUT_MILLISECONDS = 30_000
const ENGINE_STOP_TIMEOUT_MILLISECONDS = 5_000

// A tiny budget makes a few long tool results pass the ceiling, so that the compaction scenario can elide them
const E2E_PROFILES = [
  { id: "compact-test", label: "Compact Test", role: "Compaction scenario", context_budget: 2256, reply_reserve: 256 },
]
const E2E_MOCK_MODELS = ["coder", "fast", "compact-test"]

function probePort(port) {
  return new Promise((resolvePort, reject) => {
    const probe = createServer()
    probe.once("error", reject)
    probe.listen(port, "127.0.0.1", () => {
      const { port: bound } = probe.address()
      probe.close(() => resolvePort(bound))
    })
  })
}

async function firstFreePort(preferred) {
  if (preferred === 0) return probePort(0)
  for (let port = preferred; port < preferred + 100; port += 1) {
    const bound = await probePort(port).catch(() => undefined)
    if (bound !== undefined) return bound
  }
  throw new Error(`no free port from ${preferred} to ${preferred + 99}`)
}

function profileToml({ id, label, role, context_budget, reply_reserve }) {
  return [
    "[[model]]",
    `id = ${JSON.stringify(id)}`,
    `label = ${JSON.stringify(label)}`,
    `role = ${JSON.stringify(role)}`,
    `context_budget = ${context_budget}`,
    `reply_reserve = ${reply_reserve}`,
    `tool_codec = "text"`,
    "reasoning = false",
    "delegate_target = false",
  ].join("\n")
}

// TOML string values go through JSON quoting, which matches TOML basic strings for these paths
function engineConfig({ listen, dataDirectory, mockUrl, exampleProfiles, profiles }) {
  const replacements = {
    listen: JSON.stringify(listen),
    public_origin: JSON.stringify(`http://${listen}`),
    allowed_peers: JSON.stringify(["127.0.0.1"]),
    data_dir: JSON.stringify(dataDirectory),
    assets_dir: JSON.stringify(join(repositoryRoot, "web/dist")),
    base_url: JSON.stringify(`${mockUrl}/v1`),
    // The mock speaks only the text codec, and the example profiles hold the calibrated codecs
    tool_codec: JSON.stringify("text"),
  }
  const example = readFileSync(join(repositoryRoot, "config/local-ide.example.toml"), "utf8")
  const settings = exampleProfiles ? example : example.slice(0, example.indexOf("[[model]]"))
  const configured = settings.replace(/^(\w+) = .*$/gm, (line, key) => (Object.hasOwn(replacements, key) ? `${key} = ${replacements[key]}` : line))
  return [configured.trimEnd(), ...profiles.map(profileToml)].join("\n\n") + "\n"
}

async function waitForHealth(origin, engine) {
  const deadline = Date.now() + HEALTH_TIMEOUT_MILLISECONDS
  while (Date.now() < deadline) {
    if (engine.exitCode !== null) throw new Error(`the engine stopped with code ${engine.exitCode}`)
    const healthy = await fetch(`${origin}/api/health`).then((response) => response.ok, () => false)
    if (healthy) return
    await new Promise((wait) => setTimeout(wait, 200))
  }
  throw new Error("the engine did not answer /api/health")
}

function stopEngine(engine) {
  if (engine.exitCode !== null || engine.signalCode !== null) return Promise.resolve()
  return new Promise((stopped) => {
    const forced = setTimeout(() => engine.kill("SIGKILL"), ENGINE_STOP_TIMEOUT_MILLISECONDS)
    engine.once("exit", () => {
      clearTimeout(forced)
      stopped()
    })
    engine.kill("SIGTERM")
  })
}

export async function startStack({ responder, mockModels, exampleProfiles, profiles, preferredPort }) {
  execFileSync("pnpm", ["--dir", "web", "build"], { cwd: repositoryRoot, stdio: "inherit" })
  execFileSync("cargo", ["build", "-p", "engine"], { cwd: repositoryRoot, stdio: "inherit" })

  const mock = await startMockInferenceServer({ models: mockModels, responder })
  const mockUrl = `http://127.0.0.1:${mock.port}`
  const listen = `127.0.0.1:${await firstFreePort(preferredPort)}`
  const temporaryDirectory = mkdtempSync(join(tmpdir(), "local-ide-stack-"))
  const configPath = join(temporaryDirectory, "local-ide.toml")
  const dataDirectory = join(temporaryDirectory, "data")
  writeFileSync(configPath, engineConfig({ listen, dataDirectory, mockUrl, exampleProfiles, profiles }))

  const engine = spawn(join(repositoryRoot, "target/debug/local-ide"), ["--config", configPath], {
    cwd: repositoryRoot,
    env: { ...process.env, LLAMA_SWAP_KEY: "local-dummy-key" },
    stdio: ["ignore", "inherit", "inherit"],
  })

  async function stop() {
    await stopEngine(engine)
    mock.server.closeAllConnections()
    await new Promise((closed) => mock.server.close(closed))
    rmSync(temporaryDirectory, { recursive: true, force: true })
  }

  const origin = `http://${listen}`
  try {
    await waitForHealth(origin, engine)
  } catch (error) {
    await stop()
    throw error
  }
  return { origin, mockUrl, enginePid: engine.pid, stop }
}

export default async function launchStack() {
  const stack = await startStack({
    responder: "scripted",
    mockModels: E2E_MOCK_MODELS,
    exampleProfiles: true,
    profiles: E2E_PROFILES,
    preferredPort: 0,
  })
  process.env.E2E_ORIGIN = stack.origin
  process.env.E2E_MOCK_URL = stack.mockUrl
  return stack.stop
}

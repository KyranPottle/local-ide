import { afterEach, beforeEach, describe, expect, test, vi } from "vitest"
import {
  createSession,
  deleteSession,
  EngineApiError,
  fetchModels,
  fetchSessionDetail,
  fetchSessions,
  fetchStatus,
  forkSession,
  updateSession,
} from "./engineApi"

const fetchStub = vi.fn<typeof fetch>()

beforeEach(() => {
  fetchStub.mockReset()
  fetchStub.mockResolvedValue(new Response('{"ok":true}', { status: 200 }))
  vi.stubGlobal("fetch", fetchStub)
})

afterEach(() => {
  vi.unstubAllGlobals()
})

const json = { "Content-Type": "application/json" }

describe("routes", () => {
  const cases: [string, () => Promise<unknown>, string, RequestInit][] = [
    ["status", () => fetchStatus(), "/api/status", { method: "GET" }],
    ["models", () => fetchModels(), "/api/models", { method: "GET" }],
    ["sessions", () => fetchSessions(), "/api/sessions", { method: "GET" }],
    ["session search", () => fetchSessions("a&b c"), "/api/sessions?q=a%26b%20c", { method: "GET" }],
    [
      "create",
      () => createSession({ workspace: "project", model: "coder", mode: "agent" }),
      "/api/sessions",
      { method: "POST", headers: json, body: '{"workspace":"project","model":"coder","mode":"agent"}' },
    ],
    ["detail", () => fetchSessionDetail("s/1"), "/api/sessions/s%2F1", { method: "GET" }],
    [
      "update",
      () => updateSession("s1", { title: "New" }),
      "/api/sessions/s1",
      { method: "PATCH", headers: json, body: '{"title":"New"}' },
    ],
    [
      "fork",
      () => forkSession("s1", { seq: 4 }),
      "/api/sessions/s1/fork",
      { method: "POST", headers: json, body: '{"seq":4}' },
    ],
  ]

  test.each(cases)("%s", async (_name, call, url, init) => {
    expect(await call()).toEqual({ ok: true })
    expect(fetchStub).toHaveBeenCalledWith(url, init)
  })

  test("delete", async () => {
    fetchStub.mockResolvedValue(new Response(null, { status: 204 }))
    expect(await deleteSession("s1")).toBe(undefined)
    expect(fetchStub).toHaveBeenCalledWith("/api/sessions/s1", { method: "DELETE" })
  })
})

describe("errors", () => {
  test("maps the ApiError message and the status", async () => {
    fetchStub.mockResolvedValue(new Response('{"message":"session has the active run"}', { status: 409 }))
    const error = await deleteSession("s1").catch((reason: unknown) => reason)
    expect(error).toBeInstanceOf(EngineApiError)
    expect(error).toMatchObject({ status: 409, message: "session has the active run" })
  })

  test("uses the status text without a JSON body", async () => {
    fetchStub.mockResolvedValue(new Response("<html>", { status: 502, statusText: "Bad Gateway" }))
    await expect(fetchStatus()).rejects.toMatchObject({ status: 502, message: "Bad Gateway" })
  })
})

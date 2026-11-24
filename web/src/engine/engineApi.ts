import type {
  ApiError,
  CreateSession,
  EngineStatus,
  ForkSession,
  ModelInfo,
  Session,
  SessionDetail,
  UpdateSession,
} from "../protocol"

export class EngineApiError extends Error {
  readonly status: number

  constructor(status: number, message: string) {
    super(message)
    this.name = "EngineApiError"
    this.status = status
  }
}

async function errorMessage(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as Partial<ApiError>
    if (typeof body.message === "string") return body.message
  } catch {
    // A proxy error page has no JSON body, so the status text stands in
  }
  return response.statusText || `request failed with status ${response.status}`
}

async function request(method: string, url: string, body?: unknown): Promise<Response> {
  const init: RequestInit = body === undefined
    ? { method }
    : { method, headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) }
  const response = await fetch(url, init)
  if (!response.ok) throw new EngineApiError(response.status, await errorMessage(response))
  return response
}

async function requestJson<T>(method: string, url: string, body?: unknown): Promise<T> {
  return (await (await request(method, url, body)).json()) as T
}

const sessionUrl = (id: string) => `/api/sessions/${encodeURIComponent(id)}`

export const fetchStatus = () => requestJson<EngineStatus>("GET", "/api/status")

export const fetchModels = () => requestJson<ModelInfo[]>("GET", "/api/models")

export function fetchSessions(searchText = ""): Promise<Session[]> {
  const query = searchText === "" ? "" : `?q=${encodeURIComponent(searchText)}`
  return requestJson("GET", `/api/sessions${query}`)
}

export const createSession = (body: CreateSession) => requestJson<Session>("POST", "/api/sessions", body)

export const fetchSessionDetail = (id: string) => requestJson<SessionDetail>("GET", sessionUrl(id))

export const updateSession = (id: string, body: UpdateSession) => requestJson<Session>("PATCH", sessionUrl(id), body)

export async function deleteSession(id: string): Promise<void> {
  await request("DELETE", sessionUrl(id))
}

export const forkSession = (id: string, body: ForkSession) =>
  requestJson<Session>("POST", `${sessionUrl(id)}/fork`, body)

export type EngineApi = {
  readonly fetchStatus: typeof fetchStatus
  readonly fetchModels: typeof fetchModels
  readonly fetchSessions: typeof fetchSessions
  readonly createSession: typeof createSession
  readonly fetchSessionDetail: typeof fetchSessionDetail
  readonly updateSession: typeof updateSession
  readonly deleteSession: typeof deleteSession
  readonly forkSession: typeof forkSession
}

export const engineApi: EngineApi = {
  fetchStatus,
  fetchModels,
  fetchSessions,
  createSession,
  fetchSessionDetail,
  updateSession,
  deleteSession,
  forkSession,
}

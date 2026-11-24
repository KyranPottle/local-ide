import { createSignal } from "solid-js"
import type { ClientFrame, ServerFrame } from "../protocol"

const FIRST_RETRY_MILLISECONDS = 1000
const MAX_RETRY_MILLISECONDS = 15_000

export type ConnectionState =
  | { readonly kind: "connecting" }
  | { readonly kind: "open" }
  | { readonly kind: "closed", readonly retryAt: number }

export type SendResult = { readonly kind: "sent" } | { readonly kind: "not_connected" }

export type EngineSocket = Pick<WebSocket, "onopen" | "onclose" | "onmessage" | "send">

export type EngineSocketFactory = (url: string) => EngineSocket

export type ConnectionListener = {
  readonly frame?: (frame: ServerFrame) => void
  readonly state?: (state: ConnectionState) => void
}

export function engineSocketUrl(location: Pick<Location, "protocol" | "host">): string {
  const scheme = location.protocol === "https:" ? "wss" : "ws"
  return `${scheme}://${location.host}/api/ws`
}

function parseServerFrame(data: unknown): ServerFrame | undefined {
  if (typeof data !== "string") return undefined
  try {
    const frame: unknown = JSON.parse(data)
    if (typeof frame === "object" && frame !== null && typeof (frame as { type?: unknown }).type === "string") {
      return frame as ServerFrame
    }
  } catch {
    // A malformed frame is dropped, the engine is the only sender
  }
  return undefined
}

export function createEngineConnection(
  url: string,
  createSocket: EngineSocketFactory = (address) => new WebSocket(address),
) {
  const [state, setState] = createSignal<ConnectionState>({ kind: "connecting" })
  const listeners = new Set<ConnectionListener>()
  let socket: EngineSocket | undefined
  let retryDelay = FIRST_RETRY_MILLISECONDS

  function changeState(next: ConnectionState): void {
    setState(next)
    for (const listener of listeners) listener.state?.(next)
  }

  function connect(): void {
    changeState({ kind: "connecting" })
    const current = createSocket(url)
    socket = current
    current.onopen = () => {
      retryDelay = FIRST_RETRY_MILLISECONDS
      changeState({ kind: "open" })
    }
    current.onmessage = (event) => {
      const frame = parseServerFrame(event.data)
      if (!frame) return
      for (const listener of listeners) listener.frame?.(frame)
    }
    current.onclose = () => {
      socket = undefined
      const delay = retryDelay
      retryDelay = Math.min(retryDelay * 2, MAX_RETRY_MILLISECONDS)
      setTimeout(connect, delay)
      changeState({ kind: "closed", retryAt: Date.now() + delay })
    }
  }

  connect()

  return {
    state,

    listen(listener: ConnectionListener): () => void {
      listeners.add(listener)
      return () => listeners.delete(listener)
    },

    send(frame: ClientFrame): SendResult {
      if (!socket || state().kind !== "open") return { kind: "not_connected" }
      socket.send(JSON.stringify(frame))
      return { kind: "sent" }
    },
  }
}

export type EngineConnection = ReturnType<typeof createEngineConnection>

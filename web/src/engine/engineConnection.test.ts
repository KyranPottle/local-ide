import { afterEach, beforeEach, expect, test, vi } from "vitest"
import type { ServerFrame } from "../protocol"
import { createEngineConnection, engineSocketUrl, type ConnectionState, type EngineSocket } from "./engineConnection"

class FakeSocket implements EngineSocket {
  onopen: EngineSocket["onopen"] = null
  onclose: EngineSocket["onclose"] = null
  onmessage: EngineSocket["onmessage"] = null
  readonly sent: string[] = []
  readonly url: string

  constructor(url: string) {
    this.url = url
  }

  send(text: string): void {
    this.sent.push(text)
  }

  open(): void {
    this.onopen?.call(this as unknown as WebSocket, new Event("open"))
  }

  close(): void {
    this.onclose?.call(this as unknown as WebSocket, new Event("close") as CloseEvent)
  }

  receive(data: string): void {
    this.onmessage?.call(this as unknown as WebSocket, new MessageEvent("message", { data }))
  }
}

let sockets: FakeSocket[]

function connect() {
  const connection = createEngineConnection("ws://host/api/ws", (url) => {
    const socket = new FakeSocket(url)
    sockets.push(socket)
    return socket
  })
  const states: ConnectionState[] = []
  const frames: ServerFrame[] = []
  connection.listen({ state: (state) => states.push(state), frame: (frame) => frames.push(frame) })
  return { connection, states, frames }
}

const latest = () => sockets.at(-1) as FakeSocket

beforeEach(() => {
  sockets = []
  vi.useFakeTimers({ now: 0 })
})

afterEach(() => {
  vi.useRealTimers()
})

test("the socket URL follows the page scheme", () => {
  expect(engineSocketUrl({ protocol: "https:", host: "ide.example.ts.net" })).toBe("wss://ide.example.ts.net/api/ws")
  expect(engineSocketUrl({ protocol: "http:", host: "127.0.0.1:5173" })).toBe("ws://127.0.0.1:5173/api/ws")
})

test("reconnects with a doubling delay up to 15 seconds", () => {
  const { connection } = connect()
  const retryTimes: number[] = []
  for (let attempt = 0; attempt < 6; attempt += 1) {
    latest().close()
    const state = connection.state()
    if (state.kind !== "closed") throw new Error("expected a closed connection")
    retryTimes.push(state.retryAt - Date.now())
    vi.advanceTimersByTime(state.retryAt - Date.now())
  }
  expect(retryTimes).toEqual([1000, 2000, 4000, 8000, 15000, 15000])
  expect(sockets).toHaveLength(7)
  expect(connection.state()).toEqual({ kind: "connecting" })
})

test("an open resets the delay", () => {
  const { connection, states } = connect()
  latest().close()
  vi.advanceTimersByTime(1000)
  latest().close()
  vi.advanceTimersByTime(2000)
  latest().open()
  latest().close()
  expect(connection.state()).toEqual({ kind: "closed", retryAt: 4000 })
  expect(states.map((state) => state.kind)).toEqual([
    "closed",
    "connecting",
    "closed",
    "connecting",
    "open",
    "closed",
  ])
})

test("a send while closed fails", () => {
  const { connection } = connect()
  expect(connection.send({ type: "run_cancel" })).toEqual({ kind: "not_connected" })
  latest().open()
  expect(connection.send({ type: "run_cancel" })).toEqual({ kind: "sent" })
  latest().close()
  expect(connection.send({ type: "run_cancel" })).toEqual({ kind: "not_connected" })
  expect(sockets[0]?.sent).toEqual(['{"type":"run_cancel"}'])
})

test("parses server frames and drops malformed ones", () => {
  const { frames } = connect()
  latest().open()
  latest().receive('{"type":"run_started","session_id":"s1"}')
  latest().receive("not json")
  latest().receive('{"no_type":1}')
  expect(frames).toEqual([{ type: "run_started", session_id: "s1" }])
  expect(latest().url).toBe("ws://host/api/ws")
})

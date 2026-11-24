import { expect, test } from "vitest"
import type { ClientFrame, ServerFrame } from "./protocol"

test("a server frame decodes to the generated binding", () => {
  const frame: ServerFrame = JSON.parse(
    '{"type":"run_finished","session_id":"0193a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b","outcome":{"kind":"step_limit"}}',
  )

  if (frame.type !== "run_finished") throw new Error(`unexpected frame type ${frame.type}`)
  expect(frame.outcome.kind).toBe("step_limit")
})

test("a client frame encodes with its type tag", () => {
  const frame: ClientFrame = { type: "tool_result", call_id: "call_1", output: "no matches", is_error: false }

  expect(JSON.stringify(frame)).toBe('{"type":"tool_result","call_id":"call_1","output":"no matches","is_error":false}')
})

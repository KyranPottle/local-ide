import { createServer } from "node:http"
import { demoReply } from "./demoResponder.mjs"

const CHUNK_CHARACTERS = 16
const CHUNK_DELAY_MILLISECONDS = 8
const CHARS_PER_TOKEN = 3.5
const FALLBACK_REPLY = { text: "No scripted reply.", reasoning: "" }

async function readBody(request) {
  const chunks = []
  for await (const chunk of request) chunks.push(chunk)
  return Buffer.concat(chunks).toString("utf8")
}

function pieces(text) {
  const result = []
  for (let index = 0; index < text.length; index += CHUNK_CHARACTERS) result.push(text.slice(index, index + CHUNK_CHARACTERS))
  return result
}

const pause = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds))

// Usage that follows the prompt size keeps the token scale of the engine near 1
function promptTokens(messages) {
  const characters = messages.reduce((total, message) => total + String(message.content ?? "").length, 0)
  return Math.ceil(characters / CHARS_PER_TOKEN)
}

function sendJson(response, value) {
  response.writeHead(200, { "Content-Type": "application/json" })
  response.end(JSON.stringify(value))
}

export function startMockInferenceServer({ models, port = 0, responder = "scripted" }) {
  const replies = []
  const requests = []
  const counters = { activeRequests: 0, maxConcurrentRequests: 0, disconnectedRequests: 0 }

  function nextReply(body) {
    if (responder === "demo") return demoReply(body.messages)
    return replies.shift() ?? FALLBACK_REPLY
  }

  async function streamReply(response, body) {
    const reply = nextReply(body)
    const delay = reply.chunkDelayMilliseconds ?? CHUNK_DELAY_MILLISECONDS
    let disconnected = false
    response.on("close", () => {
      if (response.writableFinished) return
      disconnected = true
      counters.disconnectedRequests += 1
    })
    const chunk = (choice, extra = {}) => `data: ${JSON.stringify({ id: "mock", object: "chat.completion.chunk", model: body.model, choices: choice ? [choice] : [], ...extra })}\n\n`
    response.writeHead(200, { "Content-Type": "text/event-stream", "Cache-Control": "no-cache" })
    const deltas = [
      ...pieces(reply.reasoning ?? "").map((piece) => ({ reasoning_content: piece })),
      ...pieces(reply.text).map((piece) => ({ content: piece })),
    ]
    for (const delta of deltas) {
      if (disconnected) return
      response.write(chunk({ index: 0, delta, finish_reason: null }))
      await pause(delay)
    }
    if (disconnected) return
    response.write(chunk({ index: 0, delta: {}, finish_reason: "stop" }))
    response.write(chunk(null, {
      usage: { prompt_tokens: promptTokens(body.messages), completion_tokens: Math.ceil(reply.text.length / 4), total_tokens: 0 },
      timings: { predicted_per_second: 42.5 },
    }))
    response.end("data: [DONE]\n\n")
  }

  async function completeChat(request, response) {
    counters.activeRequests += 1
    counters.maxConcurrentRequests = Math.max(counters.maxConcurrentRequests, counters.activeRequests)
    response.on("close", () => {
      counters.activeRequests -= 1
    })
    const body = JSON.parse(await readBody(request))
    requests.push(body)
    await streamReply(response, body)
  }

  const server = createServer(async (request, response) => {
    const url = new URL(request.url ?? "/", "http://mock")
    const route = `${request.method} ${url.pathname}`
    switch (route) {
      case "GET /v1/models":
        sendJson(response, { object: "list", data: models.map((id) => ({ id, object: "model" })) })
        return
      case "POST /v1/chat/completions":
        await completeChat(request, response)
        return
      case "POST /control/replies":
        replies.push(...JSON.parse(await readBody(request)))
        response.writeHead(204).end()
        return
      case "GET /control/requests":
        sendJson(response, requests)
        return
      case "GET /control/state":
        sendJson(response, counters)
        return
      case "POST /control/reset":
        replies.length = 0
        requests.length = 0
        counters.maxConcurrentRequests = counters.activeRequests
        counters.disconnectedRequests = 0
        response.writeHead(204).end()
        return
      default:
        response.writeHead(404).end()
    }
  })

  return new Promise((resolve) => {
    server.listen(port, "127.0.0.1", () => resolve({ server, port: server.address().port }))
  })
}

import { toolCallText } from "./textToolCodec.mjs"

const NOTICE = "_This is the demo responder, not a real model._"
const EDIT_COMMENT = "edited by the local-ide demo responder"
const TOOL_RESPONSE = /^<tool_response>\n([\s\S]*)\n<\/tool_response>$/
const WORKSPACE_BLOCK = /^<workspace[^>]*>[\s\S]*?<\/workspace>\n/
const EDIT_REQUEST = /^edit\s+(\S+)/
const READ_LINE = /^\d+\|(.*)$/
const LINE_CUT = " [line cut at 500 characters]"

const commentSyntax = {
  c: ["// ", ""], cpp: ["// ", ""], css: ["/* ", " */"], go: ["// ", ""], h: ["// ", ""], html: ["<!-- ", " -->"],
  java: ["// ", ""], js: ["// ", ""], jsx: ["// ", ""], kt: ["// ", ""], md: ["<!-- ", " -->"], mjs: ["// ", ""],
  rs: ["// ", ""], sql: ["-- ", ""], swift: ["// ", ""], ts: ["// ", ""], tsx: ["// ", ""],
}
const DEFAULT_COMMENT_SYNTAX = ["# ", ""]

const answer = (lines) => ({ text: [...lines, "", NOTICE].join("\n") })
const call = (name, args) => ({ text: `${NOTICE}\n${toolCallText(name, args)}` })

function toolOutput(message) {
  return message.role === "user" ? TOOL_RESPONSE.exec(message.content)?.[1] : undefined
}

function fileLines(readOutput) {
  return readOutput.split("\n").flatMap((line) => {
    const match = READ_LINE.exec(line)
    return match ? [match[1]] : []
  })
}

function firstPath(globOutput) {
  return globOutput.split("\n").find((line) => line !== "" && line !== "no matches" && !line.startsWith("["))
}

function commentLine(path) {
  const extension = path.includes(".") ? path.split(".").pop() : ""
  const [opening, closing] = commentSyntax[extension] ?? DEFAULT_COMMENT_SYNTAX
  return `${opening}${EDIT_COMMENT}${closing}`
}

function summaryReply(results) {
  const [listing, content] = results
  if (listing === undefined) return call("glob", { pattern: "*" })
  const path = firstPath(listing)
  if (path === undefined) return answer(["The folder has no files at the top level, so I read nothing."])
  if (content === undefined) return call("read", { path })
  if (content.startsWith("error:")) return answer([`I could not read \`${path}\`: ${content}`])
  const lines = fileLines(content)
  const fileCount = listing.split("\n").filter((line) => line !== "" && !line.startsWith("[")).length
  return answer([
    "## Summary",
    "",
    `I listed ${fileCount} top-level file(s) and read \`${path}\`.`,
    "",
    `- Lines read: ${lines.length}`,
    `- First line: \`${(lines[0] ?? "").trim() || "(empty)"}\``,
  ])
}

function editReply(path, results) {
  const [content, written] = results
  if (content === undefined) return call("read", { path })
  if (content.startsWith("error:")) return answer([`I could not read \`${path}\`: ${content}`])
  if (written !== undefined) return answer([`I appended one comment line to \`${path}\`.`, "", `Tool result: \`${written}\``])
  const lines = fileLines(content)
  // A partial read would drop the rest of the file on write
  if (content.includes("\n[lines ") || lines.some((line) => line.endsWith(LINE_CUT))) {
    return answer([`\`${path}\` is too long for the demo responder, so I left it as it is.`])
  }
  return call("write", { path, content: [...lines, commentLine(path), ""].join("\n") })
}

export function demoReply(messages) {
  const start = messages.findLastIndex((message) => message.role === "user" && toolOutput(message) === undefined)
  const request = (messages[start]?.content ?? "").replace(WORKSPACE_BLOCK, "").trim()
  const results = messages.slice(start + 1).map(toolOutput).filter((output) => output !== undefined)
  const editPath = EDIT_REQUEST.exec(request)?.[1]
  return editPath === undefined ? summaryReply(results) : editReply(editPath, results)
}

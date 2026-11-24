export function toolCallText(name, args) {
  return `<tool_call>\n${JSON.stringify({ name, arguments: args })}\n</tool_call>`
}

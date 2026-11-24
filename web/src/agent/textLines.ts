// A final line break ends the last line and starts no new one, so "a\n" has one line
export function splitLines(text: string): string[] {
  if (text === "") return []
  const lines = text.split(/\r?\n/)
  if (lines.at(-1) === "") lines.pop()
  return lines
}

export const READ_LINE_CHARACTER_LIMIT = 500
const READ_LINE_CUT_MARKER = " [line cut at 500 characters]"

export function cutReadOutputLine(line: string): string {
  if (line.length <= READ_LINE_CHARACTER_LIMIT) return line
  let end = READ_LINE_CHARACTER_LIMIT
  const code = line.charCodeAt(end - 1)
  if (code >= 0xd800 && code <= 0xdbff) end -= 1
  return `${line.slice(0, end)}${READ_LINE_CUT_MARKER}`
}

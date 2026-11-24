// A final line break ends the last line and starts no new one, so "a\n" has one line
export function splitLines(text: string): string[] {
  if (text === "") return []
  const lines = text.split(/\r?\n/)
  if (lines.at(-1) === "") lines.pop()
  return lines
}


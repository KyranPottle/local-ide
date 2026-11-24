import type { Attachment, LineRange } from "../protocol"
import type { WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"
import { splitLines } from "./textLines"

const ATTACHMENT_LINE_LIMIT = 400

export async function buildAttachment(
  fileSystem: WorkspaceFileSystem,
  path: string,
  range?: LineRange,
): Promise<Attachment> {
  const lines = splitLines(await fileSystem.readTextFile(path))
  const first = range?.first ?? 1
  const last = Math.min(range?.last ?? lines.length, lines.length)
  if (range !== undefined && (!Number.isSafeInteger(first) || first < 1 || first > last)) {
    throw new RangeError(`line range ${first}-${range.last} is outside ${path} (${lines.length} lines)`)
  }
  const included = lines.slice(first - 1, Math.min(last, first - 1 + ATTACHMENT_LINE_LIMIT))
  const includedLast = first - 1 + included.length
  const content = included.join("\n")
  const lineRange = range === undefined ? null : { first, last: includedLast }
  if (includedLast === last) return { path, lines: lineRange, content }
  return {
    path,
    lines: lineRange,
    content: `${content}\n[attachment cut at ${ATTACHMENT_LINE_LIMIT} lines: lines ${first}-${includedLast} of ${first}-${last}]`,
  }
}

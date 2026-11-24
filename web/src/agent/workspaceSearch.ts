import { ROOT_DIRECTORY, WorkspaceFileError, type WorkspaceFileSystem } from "../workspace/WorkspaceFileSystem"
import { compileGlob, type PathMatcher } from "./globPattern"
import { failureFromError, toolSuccess, type ToolOutput } from "./toolOutput"
import { splitLines } from "./textLines"

const GLOB_PATH_LIMIT = 100
const GREP_MATCH_LIMIT = 50
const GREP_LINE_CHARACTERS = 200

export function pathHasControlCharacter(path: string): boolean {
  for (let index = 0; index < path.length; index += 1) {
    const code = path.charCodeAt(index)
    if (code < 0x20 || code === 0x7f) return true
  }
  return false
}

export type SearchRequest =
  | { readonly kind: "glob", readonly pattern: string }
  | { readonly kind: "grep", readonly pattern: string, readonly path: string, readonly glob: string | undefined }

async function globSearch(fileSystem: WorkspaceFileSystem, pattern: string): Promise<ToolOutput> {
  const matches = compileGlob(pattern)
  const paths = []
  for await (const path of fileSystem.walkFiles()) {
    if (pathHasControlCharacter(path)) continue
    if (matches(path)) paths.push(path)
  }
  if (paths.length === 0) return toolSuccess("no matches")
  paths.sort((a, b) => Number(a > b) - Number(a < b))
  const shown = paths.slice(0, GLOB_PATH_LIMIT)
  const hidden = paths.length - shown.length
  if (hidden > 0) shown.push(`[${hidden} more paths not shown. Narrow the pattern.]`)
  return toolSuccess(shown.join("\n"))
}

async function* grepCandidates(fileSystem: WorkspaceFileSystem, path: string): AsyncGenerator<string> {
  if (path === ROOT_DIRECTORY) {
    yield* fileSystem.walkFiles()
    return
  }
  try {
    await fileSystem.listDirectory(path)
  } catch (error) {
    if (error instanceof WorkspaceFileError && error.kind === "not_a_directory" && error.path === path) {
      yield path
      return
    }
    throw error
  }
  const prefix = `${path}/`
  for await (const candidate of fileSystem.walkFiles()) {
    if (candidate.startsWith(prefix)) yield candidate
  }
}

async function readSearchableText(fileSystem: WorkspaceFileSystem, path: string): Promise<string | undefined> {
  try {
    return await fileSystem.readTextFile(path)
  } catch (error) {
    if (error instanceof WorkspaceFileError && error.kind === "binary") return undefined
    throw error
  }
}

async function grepSearch(
  fileSystem: WorkspaceFileSystem,
  request: Extract<SearchRequest, { kind: "grep" }>,
): Promise<ToolOutput> {
  const expression = new RegExp(request.pattern)
  const included: PathMatcher = request.glob === undefined ? () => true : compileGlob(request.glob)
  const matches: string[] = []
  for await (const path of grepCandidates(fileSystem, request.path)) {
    if (pathHasControlCharacter(path)) continue
    if (!included(path)) continue
    const text = await readSearchableText(fileSystem, path)
    if (text === undefined) continue
    for (const [index, line] of splitLines(text).entries()) {
      if (!expression.test(line)) continue
      if (matches.length === GREP_MATCH_LIMIT) {
        matches.push("[more matches not shown. Narrow the pattern or the path.]")
        return toolSuccess(matches.join("\n"))
      }
      matches.push(`${path}:${index + 1}|${line.slice(0, GREP_LINE_CHARACTERS)}`)
    }
  }
  if (matches.length === 0) return toolSuccess("no matches")
  return toolSuccess(matches.join("\n"))
}

export async function runSearch(fileSystem: WorkspaceFileSystem, request: SearchRequest): Promise<ToolOutput> {
  try {
    switch (request.kind) {
      case "glob":
        return await globSearch(fileSystem, request.pattern)
      case "grep":
        return await grepSearch(fileSystem, request)
      default: {
        const unhandled: never = request
        return unhandled
      }
    }
  } catch (error) {
    return failureFromError(error)
  }
}

import { fileName } from "../workspace/WorkspaceFileSystem"

const escapeLiteral = (character: string) => character.replace(/[\\^$.*+?()[\]{}|/]/g, "\\$&")

function characterClassSource(pattern: string, start: number): { source: string, end: number } | undefined {
  let index = start + 1
  let source = "["
  if (pattern[index] === "!" || pattern[index] === "^") {
    source += "^"
    index += 1
  }
  // A closing bracket right after the opening bracket is a member of the class
  if (pattern[index] === "]") {
    source += "\\]"
    index += 1
  }
  for (; index < pattern.length; index += 1) {
    const character = pattern[index] as string
    if (character === "]") return { source: `${source}]`, end: index }
    source += character === "\\" || character === "[" ? `\\${character}` : character
  }
  return undefined
}

function hasClosingBrace(pattern: string, start: number): boolean {
  let depth = 0
  for (let index = start; index < pattern.length; index += 1) {
    if (pattern[index] === "{") depth += 1
    if (pattern[index] === "}") depth -= 1
    if (depth === 0) return true
  }
  return false
}

function globSource(pattern: string): string {
  let source = ""
  let braceDepth = 0
  for (let index = 0; index < pattern.length; index += 1) {
    const character = pattern[index] as string
    if (character === "*" && pattern[index + 1] === "*") {
      const atSegmentStart = index === 0 || pattern[index - 1] === "/"
      const followedBySlash = pattern[index + 2] === "/"
      if (atSegmentStart && followedBySlash) {
        source += "(?:[^/]*/)*"
        index += 2
        continue
      }
      source += ".*"
      index += 1
      continue
    }
    if (character === "*") {
      source += "[^/]*"
      continue
    }
    if (character === "?") {
      source += "[^/]"
      continue
    }
    if (character === "[") {
      const characterClass = characterClassSource(pattern, index)
      if (characterClass) {
        source += characterClass.source
        index = characterClass.end
        continue
      }
    }
    if (character === "{" && hasClosingBrace(pattern, index)) {
      braceDepth += 1
      source += "(?:"
      continue
    }
    if (character === "}" && braceDepth > 0) {
      braceDepth -= 1
      source += ")"
      continue
    }
    if (character === "," && braceDepth > 0) {
      source += "|"
      continue
    }
    source += escapeLiteral(character)
  }
  return source
}

export type PathMatcher = (path: string) => boolean

// A pattern without a slash matches the file name at any depth, as in .gitignore
export function compileGlob(pattern: string): PathMatcher {
  const expression = new RegExp(`^${globSource(pattern)}$`)
  if (pattern.includes("/")) return (path) => expression.test(path)
  return (path) => expression.test(fileName(path))
}

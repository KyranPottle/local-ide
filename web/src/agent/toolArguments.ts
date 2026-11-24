import type { JsonValue } from "../protocol"
import { ROOT_DIRECTORY } from "../workspace/WorkspaceFileSystem"
import type { SearchRequest } from "./workspaceSearch"

const READ_DEFAULT_LIMIT = 200
const READ_MAX_LIMIT = 400

export type ReadArguments = { readonly path: string, readonly offset: number, readonly limit: number }
export type EditArguments = { readonly path: string, readonly old: string, readonly new: string, readonly all: boolean }
export type WriteArguments = { readonly path: string, readonly content: string }

export type ToolArguments =
  | { readonly tool: "read", readonly arguments: ReadArguments }
  | { readonly tool: "glob" | "grep", readonly arguments: SearchRequest }
  | { readonly tool: "edit", readonly arguments: EditArguments }
  | { readonly tool: "write", readonly arguments: WriteArguments }

export type WorkspaceToolName = ToolArguments["tool"]

type JsonObject = { readonly [key: string]: JsonValue | undefined }

function argumentObject(value: JsonValue): JsonObject {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("arguments must be an object")
  }
  return value
}

function requiredString(object: JsonObject, key: string): string {
  const value = object[key]
  if (typeof value !== "string") throw new Error(`argument ${key} must be a string`)
  return value
}

function optionalString(object: JsonObject, key: string): string | undefined {
  if (object[key] === undefined || object[key] === null) return undefined
  return requiredString(object, key)
}

function optionalPositiveInteger(object: JsonObject, key: string, fallback: number): number {
  const value = object[key]
  if (value === undefined || value === null) return fallback
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 1) {
    throw new Error(`argument ${key} must be an integer of 1 or more`)
  }
  return value
}

function optionalBoolean(object: JsonObject, key: string): boolean {
  const value = object[key]
  if (value === undefined || value === null) return false
  if (typeof value !== "boolean") throw new Error(`argument ${key} must be a boolean`)
  return value
}

const parsers: Record<WorkspaceToolName, (object: JsonObject) => ToolArguments> = {
  read: (object) => ({
    tool: "read",
    arguments: {
      path: requiredString(object, "path"),
      offset: optionalPositiveInteger(object, "offset", 1),
      limit: Math.min(optionalPositiveInteger(object, "limit", READ_DEFAULT_LIMIT), READ_MAX_LIMIT),
    },
  }),
  glob: (object) => ({ tool: "glob", arguments: { kind: "glob", pattern: requiredString(object, "pattern") } }),
  grep: (object) => ({
    tool: "grep",
    arguments: {
      kind: "grep",
      pattern: requiredString(object, "pattern"),
      path: optionalString(object, "path") ?? ROOT_DIRECTORY,
      glob: optionalString(object, "glob"),
    },
  }),
  edit: (object) => ({
    tool: "edit",
    arguments: {
      path: requiredString(object, "path"),
      old: requiredString(object, "old"),
      new: requiredString(object, "new"),
      all: optionalBoolean(object, "all"),
    },
  }),
  write: (object) => ({
    tool: "write",
    arguments: { path: requiredString(object, "path"), content: requiredString(object, "content") },
  }),
}

export const WORKSPACE_TOOL_NAMES = Object.keys(parsers) as WorkspaceToolName[]

export function isWorkspaceToolName(name: string): name is WorkspaceToolName {
  return Object.hasOwn(parsers, name)
}

export function parseToolArguments(tool: WorkspaceToolName, value: JsonValue): ToolArguments {
  return parsers[tool](argumentObject(value))
}

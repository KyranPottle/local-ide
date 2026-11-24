import { describe, expect, test } from "vitest"
import { folderAccessSupport } from "../workspace/directoryAccess"
import { BRAVE_FLAG_ADDRESS, BRAVE_FLAG_HEADING, BRAVE_FLAG_STEPS, copyText, UNAVAILABLE_NOTICE } from "./folderAccessNotice"

const picker = async () => ({}) as FileSystemDirectoryHandle

describe("folder access support", () => {
  test("a browser with the directory picker is available", () => {
    expect(folderAccessSupport({ showDirectoryPicker: picker }, { brave: {} })).toEqual({ kind: "available" })
  })

  test("Brave without the directory picker has the flag turned off", () => {
    expect(folderAccessSupport({}, { brave: {} })).toEqual({ kind: "braveFlagOff" })
  })

  test("another browser without the directory picker is unavailable", () => {
    expect(folderAccessSupport({}, {})).toEqual({ kind: "unavailable" })
  })
})

describe("folder access notice", () => {
  test("the Brave instruction names the flag, the setting, and the relaunch", () => {
    expect(BRAVE_FLAG_ADDRESS).toBe("brave://flags/#file-system-access-api")
    expect(BRAVE_FLAG_HEADING).toBe("Brave has local folder access turned off.")
    expect(BRAVE_FLAG_STEPS).toEqual([
      "Open this address in a new tab:",
      "Set File System Access API to Enabled.",
      "Relaunch Brave and load this page again.",
    ])
    expect(UNAVAILABLE_NOTICE).toBe("This IDE needs Chrome, Edge, or Brave, because this browser cannot open a local folder.")
  })

  test("a copy writes the text and resolves to true", async () => {
    const written: string[] = []
    const copied = await copyText("brave://flags/#file-system-access-api", { writeText: async (text) => void written.push(text) })
    expect(copied).toBe(true)
    expect(written).toEqual(["brave://flags/#file-system-access-api"])
  })

  test("a refused copy resolves to false without an exception", async () => {
    const refusal = { writeText: () => Promise.reject(new DOMException("Denied", "NotAllowedError")) }
    expect(await copyText("x", refusal)).toBe(false)
    expect(await copyText("x", undefined)).toBe(false)
  })
})

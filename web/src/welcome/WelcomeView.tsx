import { createSignal, For, type JSX, onCleanup, onMount, Show } from "solid-js"
import { ColorSchemeToggle } from "../design/ColorSchemeToggle"
import { Icon } from "../design/Icon"
import { reportFailure } from "../layout/failureNotice"
import { type FolderAccessSupport, folderAccessSupport } from "../workspace/directoryAccess"
import { forgetWorkspace, listRecentWorkspaces } from "../workspace/recentWorkspaces"
import {
  BRAVE_FLAG_ADDRESS,
  BRAVE_FLAG_HEADING,
  BRAVE_FLAG_STEPS,
  copyText,
  UNAVAILABLE_NOTICE,
} from "./folderAccessNotice"
import "./WelcomeView.css"

const CONFIRMATION_MILLISECONDS = 1600

type CopyState = "idle" | "copied" | "failed"

const copyStateTexts: Readonly<Record<CopyState, string>> = {
  idle: "",
  copied: "Copied.",
  failed: "Copy failed. Select the address.",
}

function BraveFlagInstructions() {
  const [copyState, setCopyState] = createSignal<CopyState>("idle")
  let timer: ReturnType<typeof setTimeout> | undefined
  onCleanup(() => clearTimeout(timer))

  async function copyAddress(): Promise<void> {
    setCopyState((await copyText(BRAVE_FLAG_ADDRESS, navigator.clipboard)) ? "copied" : "failed")
    clearTimeout(timer)
    timer = setTimeout(() => setCopyState("idle"), CONFIRMATION_MILLISECONDS)
  }

  return (
    <section class="welcome-notice welcome-brave" role="alert" aria-labelledby="welcome-brave-title">
      <h2 id="welcome-brave-title" class="welcome-brave-title">
        {BRAVE_FLAG_HEADING}
      </h2>
      <ol class="welcome-brave-steps">
        <li>
          {BRAVE_FLAG_STEPS[0]}
          <div class="welcome-brave-address">
            <code>{BRAVE_FLAG_ADDRESS}</code>
            <button class="button" type="button" onClick={copyAddress}>
              Copy
            </button>
            <span class="welcome-brave-copy-state" role="status">
              {copyStateTexts[copyState()]}
            </span>
          </div>
        </li>
        <li>{BRAVE_FLAG_STEPS[1]}</li>
        <li>{BRAVE_FLAG_STEPS[2]}</li>
      </ol>
    </section>
  )
}

function FolderAccessNotice(props: { support: FolderAccessSupport }): JSX.Element {
  const support = props.support
  switch (support.kind) {
    case "available":
      return <div class="welcome-drop-target">Drop a folder here</div>
    case "braveFlagOff":
      return <BraveFlagInstructions />
    case "unavailable":
      return (
        <p class="welcome-notice" role="alert">
          {UNAVAILABLE_NOTICE}
        </p>
      )
    default:
      return support satisfies never
  }
}

export function WelcomeView(props: {
  onOpenFolder: () => void
  onOpenScratch: () => void
  onOpenRecent: (directory: FileSystemDirectoryHandle) => void
}) {
  const [recentWorkspaces, setRecentWorkspaces] = createSignal<FileSystemDirectoryHandle[]>([])
  const support = folderAccessSupport()
  const loadRecentWorkspaces = () => listRecentWorkspaces().then(setRecentWorkspaces, reportFailure)

  onMount(loadRecentWorkspaces)

  return (
    <main class="welcome">
      <div class="welcome-toolbar">
        <ColorSchemeToggle />
      </div>
      <div class="welcome-content">
        <h1 class="welcome-title">local-ide</h1>
        <FolderAccessNotice support={support} />
        <div class="welcome-actions">
          <button
            class="button button-primary"
            type="button"
            disabled={support.kind !== "available"}
            onClick={props.onOpenFolder}
          >
            Open folder
          </button>
          <button class="button" type="button" onClick={props.onOpenScratch}>
            Scratch workspace
          </button>
        </div>
        <Show when={recentWorkspaces().length > 0}>
          <section class="welcome-recent" aria-labelledby="welcome-recent-title">
            <h2 id="welcome-recent-title" class="welcome-recent-title">
              Recent
            </h2>
            <ul>
              <For each={recentWorkspaces()}>
                {(directory) => (
                  <li class="welcome-recent-item">
                    <button class="welcome-recent-open" type="button" onClick={() => props.onOpenRecent(directory)}>
                      <Icon name="folder" />
                      <span class="welcome-recent-name">{directory.name}</span>
                    </button>
                    <button
                      class="icon-button icon-button-small"
                      type="button"
                      aria-label={`Remove ${directory.name} from the recent list`}
                      title="Remove from the recent list"
                      onClick={() => forgetWorkspace(directory).then(loadRecentWorkspaces, reportFailure)}
                    >
                      <Icon name="close" />
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </section>
        </Show>
      </div>
    </main>
  )
}

import { createSignal, onCleanup, Show } from "solid-js"
import { createSessionStore } from "./agent/sessionStore"
import { AgentPanel } from "./agentPanel/AgentPanel"
import { COMPOSER_INPUT_ID } from "./agentPanel/Composer"
import { Icon } from "./design/Icon"
import { createEngineConnection, engineSocketUrl } from "./engine/engineConnection"
import { QuickFileFinder } from "./fileFinder/QuickFileFinder"
import { dismissFailureNotice, failureNotice, reportFailure } from "./layout/failureNotice"
import { IdeLayout } from "./layout/IdeLayout"
import { registerCommandShortcuts } from "./layout/keyboardShortcuts"
import { SessionList } from "./sessionList/SessionList"
import { AgentStatus } from "./statusBar/AgentStatus"
import { AgentToolbar } from "./toolbar/AgentToolbar"
import { droppedDirectory, folderAccessSupport, pickDirectory, requestReadWriteAccess } from "./workspace/directoryAccess"
import { rememberWorkspace } from "./workspace/recentWorkspaces"
import { openScratchDirectory } from "./workspace/scratchWorkspace"
import { createWorkspaceStore } from "./workspace/workspaceStore"
import { WelcomeView } from "./welcome/WelcomeView"
import "./App.css"

export function App() {
  const store = createWorkspaceStore()
  const agent = createSessionStore({ connection: createEngineConnection(engineSocketUrl(location)), workspace: store.workspace })
  const [finderOpen, setFinderOpen] = createSignal(false)
  const [dropPending, setDropPending] = createSignal(false)

  async function openDirectory(directory: FileSystemDirectoryHandle): Promise<void> {
    await requestReadWriteAccess(directory)
    const discard = !store.workspace()?.hasUnsavedChanges() || window.confirm("Discard the unsaved changes?")
    if (discard) store.open(directory)
  }

  async function openLocalDirectory(directory: FileSystemDirectoryHandle): Promise<void> {
    await openDirectory(directory)
    await rememberWorkspace(directory)
  }

  async function openPickedDirectory(): Promise<void> {
    if (folderAccessSupport().kind !== "available") return
    const directory = await pickDirectory()
    if (directory) await openLocalDirectory(directory)
  }

  async function openDroppedDirectory(transfer: DataTransfer): Promise<void> {
    const directory = await droppedDirectory(transfer)
    if (!directory) throw new Error("Drop a folder, not a file")
    await openLocalDirectory(directory)
  }

  const openFolder = () => openPickedDirectory().catch(reportFailure)

  registerCommandShortcuts({
    o: openFolder,
    p: () => setFinderOpen(store.workspace() !== undefined),
  })

  function listen<Name extends keyof WindowEventMap>(name: Name, listener: (event: WindowEventMap[Name]) => void) {
    window.addEventListener(name, listener)
    onCleanup(() => window.removeEventListener(name, listener))
  }

  listen("focus", () => store.workspace()?.reloadOpenFiles().catch(reportFailure))
  listen("beforeunload", (event) => {
    if (store.workspace()?.hasUnsavedChanges()) event.preventDefault()
  })
  listen("dragenter", (event) => {
    if (folderAccessSupport().kind !== "available") return
    if (event.dataTransfer?.types.includes("Files")) setDropPending(true)
  })

  return (
    <>
      <Show
        when={store.workspace()}
        keyed
        fallback={
          <WelcomeView
            onOpenFolder={openFolder}
            onOpenScratch={() => openScratchDirectory().then(openDirectory).catch(reportFailure)}
            onOpenRecent={(directory) => openLocalDirectory(directory).catch(reportFailure)}
          />
        }
      >
        {(workspace) => (
          <>
            <IdeLayout
              workspace={workspace}
              onOpenFolder={openFolder}
              sidebarTab={{
                id: "sessions",
                label: "Sessions",
                content: () => <SessionList store={agent} workspaceName={workspace.name} />,
              }}
              rightPanel={() => <AgentPanel store={agent} workspace={workspace} />}
              toolbar={() => <AgentToolbar store={agent} />}
              statusBar={() => <AgentStatus store={agent} />}
              focusRightPanel={() => document.getElementById(COMPOSER_INPUT_ID)?.focus()}
            />
            <Show when={finderOpen()}>
              <QuickFileFinder workspace={workspace} onClose={() => setFinderOpen(false)} />
            </Show>
          </>
        )}
      </Show>

      <Show when={dropPending()}>
        <div
          class="drop-overlay"
          onDragOver={(event) => event.preventDefault()}
          onDragLeave={() => setDropPending(false)}
          onDrop={(event) => {
            event.preventDefault()
            setDropPending(false)
            if (event.dataTransfer) openDroppedDirectory(event.dataTransfer).catch(reportFailure)
          }}
        >
          <div class="drop-overlay-target">Drop a folder to open it</div>
        </div>
      </Show>

      <Show when={failureNotice()}>
        {(text) => (
          <div class="failure-notice" role="alert">
            <span>{text()}</span>
            <button
              class="icon-button icon-button-small"
              type="button"
              aria-label="Dismiss"
              onClick={dismissFailureNotice}
            >
              <Icon name="close" />
            </button>
          </div>
        )}
      </Show>
    </>
  )
}

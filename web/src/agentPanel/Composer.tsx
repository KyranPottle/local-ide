import { createSignal, For, Match, Show, Switch } from "solid-js"
import { buildAttachment } from "../agent/attachment"
import { Icon } from "../design/Icon"
import { QuickFileFinder } from "../fileFinder/QuickFileFinder"
import { reportFailure } from "../layout/failureNotice"
import type { Attachment, Session } from "../protocol"
import type { OpenWorkspace } from "../workspace/workspaceStore"
import { attachmentLabel } from "./transcript"

export const COMPOSER_INPUT_ID = "agent-composer-input"

export type ComposerBlock =
  | { readonly kind: "ready" }
  | { readonly kind: "disconnected" }
  | { readonly kind: "busy_elsewhere", readonly sessionId: string, readonly session: Session | undefined }

export function Composer(props: {
  workspace: OpenWorkspace
  draft: string
  onDraftChange: (text: string) => void
  block: ComposerBlock
  running: boolean
  onSend: (text: string, attachments: readonly Attachment[]) => Promise<void>
  onStop: () => void
  onSelectSession: (id: string) => void
}) {
  const [attachments, setAttachments] = createSignal<readonly Attachment[]>([])
  const [finderOpen, setFinderOpen] = createSignal(false)
  let input!: HTMLTextAreaElement
  // Caret position of the typed @, which the chosen attachment replaces
  let mentionPosition: number | undefined

  const disabled = () => props.block.kind !== "ready"
  const canSend = () => !disabled() && !props.running && props.draft.trim() !== ""

  async function attach(path: string): Promise<void> {
    const attachment = await buildAttachment(props.workspace.fileSystem, path)
    setAttachments((current) => [...current.filter((entry) => entry.path !== path), attachment])
  }

  async function send(): Promise<void> {
    if (!canSend()) return
    const text = props.draft
    const sent = attachments()
    props.onDraftChange("")
    setAttachments([])
    try {
      await props.onSend(text, sent)
    } catch (error) {
      props.onDraftChange(text)
      setAttachments(sent)
      throw error
    }
  }

  function onKeyDown(event: KeyboardEvent): void {
    if (event.key !== "Enter" || event.shiftKey || event.isComposing) return
    event.preventDefault()
    send().catch(reportFailure)
  }

  function onInput(event: InputEvent & { currentTarget: HTMLTextAreaElement }): void {
    props.onDraftChange(event.currentTarget.value)
    if (event.inputType !== "insertText" || event.data !== "@") return
    mentionPosition = event.currentTarget.selectionStart - 1
    setFinderOpen(true)
  }

  function chooseMention(path: string): void {
    const position = mentionPosition
    mentionPosition = undefined
    if (position !== undefined && props.draft[position] === "@") {
      props.onDraftChange(props.draft.slice(0, position) + props.draft.slice(position + 1))
    }
    attach(path).catch(reportFailure)
  }

  function attachActiveFile(): void {
    const path = props.workspace.activeFilePath()
    if (path !== undefined) attach(path).catch(reportFailure)
  }

  return (
    <div class="composer">
      <Switch>
        <Match when={props.block.kind === "disconnected"}>
          <p class="composer-block" role="status">Not connected to the engine</p>
        </Match>
        <Match when={props.block.kind === "busy_elsewhere" && props.block} keyed>
          {(block) => (
            <p class="composer-block" role="status">
              <span>Another page runs {block.session?.title ?? "a session"}</span>
              <Show when={block.session}>
                <button class="composer-block-action" type="button" onClick={() => props.onSelectSession(block.sessionId)}>
                  Select it
                </button>
              </Show>
            </p>
          )}
        </Match>
      </Switch>
      <div class="composer-box" classList={{ "composer-box-disabled": disabled() }}>
        <Show when={attachments().length > 0}>
          <ul class="attachment-chips" aria-label="Attachments">
            <For each={attachments()}>
              {(attachment) => (
                <li class="attachment-chip">
                  <span>{attachmentLabel(attachment)}</span>
                  <button
                    class="attachment-chip-remove"
                    type="button"
                    aria-label={`Remove ${attachment.path}`}
                    onClick={() => setAttachments((current) => current.filter((entry) => entry !== attachment))}
                  >
                    <Icon name="close" />
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
        <textarea
          ref={input}
          id={COMPOSER_INPUT_ID}
          class="composer-input"
          aria-label="Message"
          placeholder="Ask the agent, @ to attach a file"
          rows={2}
          disabled={disabled()}
          value={props.draft}
          onInput={onInput}
          onKeyDown={onKeyDown}
        />
        <div class="composer-controls">
          <button
            class="icon-button icon-button-small"
            type="button"
            aria-label="Attach the active file"
            title="Attach the active file"
            disabled={disabled() || props.workspace.activeFilePath() === undefined}
            onClick={attachActiveFile}
          >
            <Icon name="attach" />
          </button>
          <Show
            when={props.running}
            fallback={
              <button
                class="composer-send"
                type="button"
                aria-label="Send"
                title="Send (Enter)"
                disabled={!canSend()}
                onClick={() => send().catch(reportFailure)}
              >
                <Icon name="send" />
              </button>
            }
          >
            <button class="composer-send composer-stop" type="button" aria-label="Stop" title="Stop" onClick={props.onStop}>
              <Icon name="stop" />
            </button>
          </Show>
        </div>
      </div>
      <Show when={finderOpen()}>
        <QuickFileFinder
          workspace={props.workspace}
          onClose={() => {
            setFinderOpen(false)
            input.focus()
          }}
          onChoose={chooseMention}
        />
      </Show>
    </div>
  )
}

import { type Component, createEffect, createMemo, createSignal, For, Index, type JSX, on, Show } from "solid-js"
import { Dynamic } from "solid-js/web"
import { diffLines } from "../agent/lineDiff"
import type { RevertResult } from "../agent/toolHost"
import { Icon, type IconName } from "../design/Icon"
import { formatTokenCount } from "../toolbar/contextMeter"
import { CopyButton } from "./CopyButton"
import { diffRows } from "./diffRows"
import { MarkdownView } from "./MarkdownView"
import type { ChangeReview, RunEnding, ToolState, TranscriptItem } from "./transcript"
import { useTranscriptActions } from "./transcriptActions"

type ItemOf<Kind extends TranscriptItem["kind"]> = Extract<TranscriptItem, { kind: Kind }>
type ItemProps<Kind extends TranscriptItem["kind"]> = { item: ItemOf<Kind>, nested: boolean }

const DIFF_CONTEXT_LINES = 3

const toolIcons: Readonly<Record<string, IconName>> = {
  read: "file",
  glob: "search",
  grep: "search",
  edit: "pencil",
  write: "file",
  delegate: "delegate",
}

const toolStateLabels: Readonly<Record<ToolState["kind"], string>> = {
  running: "Running",
  done: "Done",
  error: "Error",
  stopped: "Stopped",
}

const revertMessages: Readonly<Record<RevertResult["kind"], string>> = {
  reverted: "Reverted",
  conflict: "The file changed after this edit, so it stays as it is",
  unknown_change: "This change is no longer in the change journal",
  failed: "Revert failed",
}

function ToolStateMark(props: { state: ToolState }) {
  return (
    <span class={`tool-state tool-state-${props.state.kind}`} title={toolStateLabels[props.state.kind]}>
      <Show when={props.state.kind === "running"} fallback={<span class="visually-hidden">{toolStateLabels[props.state.kind]}</span>}>
        <span class="tool-spinner" role="status" aria-label="Running" />
      </Show>
      <Show when={props.state.kind === "done"}><Icon name="check" /></Show>
      <Show when={props.state.kind === "error"}><Icon name="close" /></Show>
    </span>
  )
}

const toolOutput = (state: ToolState) => (state.kind === "done" || state.kind === "error" ? state.output : undefined)

function ToolRow(props: {
  name: string
  argument: string
  path: string | null
  state: ToolState
  expanded: boolean
  onToggle: () => void
}) {
  const actions = useTranscriptActions()
  return (
    <div class="tool-row">
      <button
        class="tool-row-toggle"
        type="button"
        aria-expanded={props.expanded}
        aria-label={`${props.name} ${props.argument}`}
        onClick={props.onToggle}
      >
        <span class="tool-row-chevron" classList={{ "tool-row-chevron-open": props.expanded }}><Icon name="chevron" /></span>
        <Icon name={toolIcons[props.name] ?? "settings"} />
        <span class="tool-row-name">{props.name}</span>
      </button>
      <Show when={props.path} fallback={<span class="tool-row-argument">{props.argument}</span>}>
        {(path) => (
          <button class="tool-row-argument tool-row-path" type="button" title={`Open ${path()}`} onClick={() => actions.openFile(path())}>
            {path()}
          </button>
        )}
      </Show>
      <ToolStateMark state={props.state} />
    </div>
  )
}

function ToolOutputBlock(props: { state: ToolState }) {
  return (
    <Show when={toolOutput(props.state)} fallback={<p class="tool-output-empty">{toolStateLabels[props.state.kind]}</p>}>
      {(output) => <pre class="tool-output" classList={{ "tool-output-error": props.state.kind === "error" }}>{output()}</pre>}
    </Show>
  )
}

function ToolCallView(props: ItemProps<"tool">) {
  const [expanded, setExpanded] = createSignal(false)
  return (
    <div class="transcript-tool">
      <ToolRow
        name={props.item.call.name}
        argument={props.item.argument}
        path={props.item.path}
        state={props.item.state}
        expanded={expanded()}
        onToggle={() => setExpanded((open) => !open)}
      />
      <Show when={expanded()}><ToolOutputBlock state={props.item.state} /></Show>
    </div>
  )
}

function DiffView(props: { review: Exclude<ChangeReview, { kind: "none" }> }) {
  const rows = createMemo(() => {
    const { earlier, content } = props.review.change
    const diff = diffLines(earlier.kind === "text" ? earlier.text : "", content)
    return { rows: diffRows(diff.lines, DIFF_CONTEXT_LINES), capped: diff.capped }
  })
  return (
    <div class="diff" role="table" aria-label={`Changes to ${props.review.change.path}`}>
      <For each={rows().rows}>
        {(row) => {
          if (row.kind === "gap") {
            return <div class="diff-gap" role="row"><span role="cell">{row.count} unchanged lines</span></div>
          }
          const oldLine = row.kind === "added" ? "" : row.oldLine
          const newLine = row.kind === "removed" ? "" : row.newLine
          return (
            <div class={`diff-line diff-line-${row.kind}`} role="row">
              <span class="diff-number" role="cell">{oldLine}</span>
              <span class="diff-number" role="cell">{newLine}</span>
              <span class="diff-text" role="cell">{row.text}</span>
            </div>
          )
        }}
      </For>
      <Show when={rows().capped}><div class="diff-gap" role="row"><span role="cell">Diff limited to the first 2000 lines</span></div></Show>
    </div>
  )
}

function FileChangeView(props: ItemProps<"file_change">) {
  const actions = useTranscriptActions()
  const [expanded, setExpanded] = createSignal(false)
  const [revertMessage, setRevertMessage] = createSignal<string>()
  const [reverting, setReverting] = createSignal(false)

  async function revert(): Promise<void> {
    setReverting(true)
    const result = await actions.revert(props.item.call.id)
    setReverting(false)
    setRevertMessage(result.kind === "failed" ? `${revertMessages.failed}: ${result.message}` : revertMessages[result.kind])
  }

  const review = () => props.item.review
  const reviewWithChange = () => {
    const current = review()
    return current.kind === "none" ? undefined : current
  }

  return (
    <div class="transcript-tool">
      <ToolRow
        name={props.item.call.name}
        argument={props.item.path}
        path={props.item.path || null}
        state={props.item.state}
        expanded={expanded()}
        onToggle={() => setExpanded((open) => !open)}
      />
      <Show when={expanded()}><ToolOutputBlock state={props.item.state} /></Show>
      <Show when={reviewWithChange()}>
        {(current) => (
          <div class="file-change">
            <DiffView review={current()} />
            <div class="file-change-actions">
              <Show
                when={current().kind === "pending"}
                fallback={
                  <button class="button button-small" type="button" disabled={reverting()} onClick={() => void revert()}>Revert</button>
                }
              >
                <span class="file-change-message">Waiting for your decision</span>
                <button class="button button-small" type="button" onClick={() => actions.decline(props.item.call.id)}>Decline</button>
                <button class="button button-small button-primary" type="button" onClick={() => actions.approve(props.item.call.id)}>Apply</button>
              </Show>
            </div>
          </div>
        )}
      </Show>
      <Show when={revertMessage()}>{(message) => <p class="file-change-result" role="status">{message()}</p>}</Show>
    </div>
  )
}

function DelegateView(props: ItemProps<"delegate">) {
  const actions = useTranscriptActions()
  const [open, setOpen] = createSignal(props.item.state.kind === "running")
  createEffect(on(() => props.item.state.kind === "running", (running) => setOpen(running), { defer: true }))
  const answer = () => toolOutput(props.item.state)

  return (
    <div class="delegate">
      <button class="delegate-header" type="button" aria-expanded={open()} onClick={() => setOpen((value) => !value)}>
        <span class="tool-row-chevron" classList={{ "tool-row-chevron-open": open() }}><Icon name="chevron" /></span>
        <Icon name="delegate" />
        <span class="tool-row-name">delegate</span>
        <span class="delegate-model">{props.item.child ? actions.modelLabel(props.item.child.model) : ""}</span>
        <span class="tool-row-argument">{props.item.task}</span>
        <ToolStateMark state={props.item.state} />
      </button>
      <Show
        when={open()}
        fallback={<Show when={answer()}>{(text) => <div class="delegate-answer"><MarkdownView text={text()} /></div>}</Show>}
      >
        <div class="delegate-body">
          <Show when={props.item.child} fallback={<p class="tool-output-empty">Waiting for the child session</p>}>
            {(child) => <TranscriptList items={child().items} nested />}
          </Show>
        </div>
      </Show>
    </div>
  )
}

function UserMessage(props: ItemProps<"user">) {
  const actions = useTranscriptActions()
  return (
    <div class="message message-user">
      <div class="message-user-body">
        <p class="message-user-text">{props.item.text}</p>
        <Show when={props.item.attachments.length > 0}>
          <ul class="attachment-chips" aria-label="Attachments">
            <For each={props.item.attachments}>{(label) => <li class="attachment-chip">{label}</li>}</For>
          </ul>
        </Show>
      </div>
      <Show when={!props.nested}>
        <div class="message-actions">
          <button
            class="icon-button icon-button-small"
            type="button"
            aria-label="Edit and resend"
            title="Edit and resend"
            onClick={() => actions.editAndResend(props.item.seq, props.item.text)}
          >
            <Icon name="pencil" />
          </button>
          <CopyButton text={props.item.text} label="Copy" />
        </div>
      </Show>
    </div>
  )
}

function Reasoning(props: { text: string, open: boolean }) {
  return (
    <Show when={props.text}>
      <details class="reasoning" open={props.open}>
        <summary>Thought</summary>
        <p class="reasoning-text">{props.text}</p>
      </details>
    </Show>
  )
}

function usageLine(item: ItemOf<"assistant">, label: string): string {
  const { usage } = item
  if (!usage) return label
  const parts = [label, `${formatTokenCount(usage.prompt_tokens)} in`, `${formatTokenCount(usage.completion_tokens)} out`]
  if (usage.tokens_per_second !== null) parts.push(`${usage.tokens_per_second.toFixed(1)} tok/s`)
  return parts.join(" · ")
}

function AssistantMessage(props: ItemProps<"assistant">) {
  const actions = useTranscriptActions()
  // A turn with only tool calls shows as its tool rows
  return (
    <Show when={props.item.text || props.item.reasoning}>
      <div class="message message-assistant">
        <Reasoning text={props.item.reasoning} open={false} />
        <Show when={props.item.text}>{(text) => <MarkdownView text={text()} />}</Show>
        <div class="message-footer">
          <span class="message-usage">{usageLine(props.item, actions.modelLabel(props.item.model))}</span>
          <Show when={!props.nested}>
            <div class="message-actions">
              <button
                class="icon-button icon-button-small"
                type="button"
                aria-label="Fork from here"
                title="Fork from here"
                onClick={() => actions.forkThrough(props.item.forkSeq)}
              >
                <Icon name="fork" />
              </button>
              <CopyButton text={props.item.text} label="Copy" />
            </div>
          </Show>
        </div>
      </div>
    </Show>
  )
}

function ReplyInProgress(props: ItemProps<"reply_in_progress">) {
  return (
    <div class="message message-assistant" aria-busy="true">
      <Reasoning text={props.item.reasoning} open />
      <Show when={props.item.text} fallback={<span class="reply-waiting" role="status">Working</span>}>
        {(text) => <MarkdownView text={text()} />}
      </Show>
    </div>
  )
}

function ModelSwitchMarker(props: ItemProps<"model_switch">) {
  const actions = useTranscriptActions()
  return <p class="transcript-marker">Model changed: {actions.modelLabel(props.item.from)} to {actions.modelLabel(props.item.to)}</p>
}

function ElidedMarker() {
  return <p class="transcript-marker">Earlier tool results elided</p>
}

function SummaryMarker(props: ItemProps<"summary">) {
  return (
    <details class="transcript-marker transcript-summary">
      <summary>Earlier conversation summarised</summary>
      <p class="reasoning-text">{props.item.text}</p>
    </details>
  )
}

function runEndingText(ending: RunEnding): string {
  switch (ending.kind) {
    case "cancelled":
      return "Stopped"
    case "step_limit":
      return "Step limit reached"
    case "failed":
      return `Failed: ${ending.message}`
    default: {
      const unhandled: never = ending
      return unhandled
    }
  }
}

function RunEndingLine(props: ItemProps<"run_ending">) {
  const actions = useTranscriptActions()
  return (
    <div class="run-ending" classList={{ "run-ending-failed": props.item.ending.kind === "failed" }} role="status">
      <span>{runEndingText(props.item.ending)}</span>
      <Show when={props.item.ending.kind === "step_limit"}>
        <button class="button button-small" type="button" onClick={actions.continueRun}>Continue</button>
      </Show>
    </div>
  )
}

const itemViews: { readonly [Kind in TranscriptItem["kind"]]: Component<ItemProps<Kind>> } = {
  user: UserMessage,
  assistant: AssistantMessage,
  reply_in_progress: ReplyInProgress,
  tool: ToolCallView,
  file_change: FileChangeView,
  delegate: DelegateView,
  model_switch: ModelSwitchMarker,
  results_elided: ElidedMarker,
  summary: SummaryMarker,
  run_ending: RunEndingLine,
}

export function TranscriptList(props: { items: readonly TranscriptItem[], nested: boolean }): JSX.Element {
  return (
    <Index each={props.items}>
      {(item) => (
        // The table type pairs each kind with its view, which the index lookup cannot express
        <Dynamic component={itemViews[item().kind] as Component<ItemProps<TranscriptItem["kind"]>>} item={item()} nested={props.nested} />
      )}
    </Index>
  )
}

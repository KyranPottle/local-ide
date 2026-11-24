import { For, Show } from "solid-js"
import { isRunActive, modelLabel } from "../agent/sessionState"
import type { SessionStore } from "../agent/sessionStore"
import type { ChangeApproval } from "../agent/toolHost"
import { Icon } from "../design/Icon"
import { reportFailure } from "../layout/failureNotice"
import type { ContextReport, ModelInfo } from "../protocol"
import { contextLevel, formatTokenCount, promptLimit } from "./contextMeter"
import { Popover } from "./Popover"
import "./AgentToolbar.css"

const menuItemSelector = "[role=menuitemradio]:not([aria-disabled=true]), [role=radio]"
const menuFocusSteps: Readonly<Record<string, number>> = { ArrowDown: 1, ArrowUp: -1 }

function moveMenuFocus(event: KeyboardEvent & { currentTarget: HTMLElement }): void {
  const step = menuFocusSteps[event.key]
  if (step === undefined) return
  event.preventDefault()
  const items = [...event.currentTarget.querySelectorAll<HTMLElement>(menuItemSelector)]
  const index = items.indexOf(document.activeElement as HTMLElement)
  items[(index + step + items.length) % items.length]?.focus()
}

const focusChecked = (menu: HTMLElement) =>
  queueMicrotask(() => (menu.querySelector<HTMLElement>("[aria-checked=true]") ?? menu.querySelector<HTMLElement>(menuItemSelector))?.focus())

function ModelPicker(props: { store: SessionStore }) {
  const store = props.store
  const selected = () => {
    const id = store.state.selectedId
    return id === null ? undefined : store.state.loaded[id]?.session
  }
  const currentId = () => selected()?.model ?? store.preferredModelInfo()?.profile.id
  const label = () => {
    const id = currentId()
    return id === undefined ? "No model" : modelLabel(store.state.models, id)
  }

  function choose(model: ModelInfo, close: () => void): void {
    close()
    if (!selected()) {
      store.setPreferredModel(model.profile.id)
      return
    }
    store.changeModel(model.profile.id).catch(reportFailure)
  }

  return (
    <Popover
      trigger={{
        label: `Model: ${label()}`,
        class: "toolbar-button",
        disabledReason: isRunActive(store.state.run) ? "Wait for the run to finish to change the model" : undefined,
        content: (
          <>
            <span class="toolbar-button-text">{label()}</span>
            <span class="toolbar-button-chevron"><Icon name="chevron" /></span>
          </>
        ),
      }}
      panelLabel="Models"
    >
      {(close) => (
        <ul class="model-menu" role="menu" aria-label="Models" ref={focusChecked} onKeyDown={moveMenuFocus}>
          <For each={store.state.models}>
            {(model) => (
              <li role="none">
                <button
                  class="model-menu-item"
                  type="button"
                  role="menuitemradio"
                  aria-checked={model.profile.id === currentId()}
                  aria-disabled={!model.available}
                  tabindex={-1}
                  onClick={() => model.available && choose(model, close)}
                >
                  <span class="model-menu-label">{model.profile.label}</span>
                  <span class="model-menu-budget">{formatTokenCount(model.profile.context_budget)}</span>
                  <span class="model-menu-role">{model.profile.role || model.profile.id}</span>
                  <Show when={!model.available}><span class="model-menu-offline">offline</span></Show>
                </button>
              </li>
            )}
          </For>
        </ul>
      )}
    </Popover>
  )
}

function ContextMeter(props: { report: ContextReport }) {
  const limit = () => promptLimit(props.report)
  const fraction = () => Math.min(props.report.total / Math.max(limit(), 1), 1)
  const text = () => `${formatTokenCount(props.report.total)} / ${formatTokenCount(limit())}`
  const breakdown = () => [
    ["System", props.report.system],
    ["Tools", props.report.tools],
    ["History", props.report.history],
    ["Reply reserve", props.report.reserve],
    ["Budget", props.report.budget],
  ] as const

  return (
    <Popover
      trigger={{
        label: `Context ${text()} tokens`,
        class: "toolbar-button context-meter",
        content: (
          <>
            <span class={`context-meter-bar context-meter-${contextLevel(props.report)}`} aria-hidden="true">
              <span style={{ width: `${fraction() * 100}%` }} />
            </span>
            <span class="context-meter-text" data-testid="context-meter-text">{text()}</span>
          </>
        ),
      }}
      panelLabel="Context"
    >
      {() => (
        <dl class="context-breakdown">
          <For each={breakdown()}>
            {([name, tokens]) => (
              <>
                <dt>{name}</dt>
                <dd>{tokens.toLocaleString("en-US")}</dd>
              </>
            )}
          </For>
        </dl>
      )}
    </Popover>
  )
}

const approvalOptions: readonly { readonly value: ChangeApproval, readonly label: string }[] = [
  { value: "automatic", label: "Automatic" },
  { value: "confirm", label: "Confirm each change" },
]

function SettingsMenu(props: { store: SessionStore }) {
  return (
    <Popover trigger={{ label: "Agent settings", class: "icon-button", content: <Icon name="settings" /> }} panelLabel="Agent settings">
      {() => (
        <div class="settings-menu">
          <p class="settings-menu-heading" id="change-approval-heading">Change approval</p>
          <div role="radiogroup" aria-labelledby="change-approval-heading" ref={focusChecked} onKeyDown={moveMenuFocus}>
            <For each={approvalOptions}>
              {(option) => (
                <button
                  class="settings-menu-option"
                  type="button"
                  role="radio"
                  aria-checked={props.store.approval() === option.value}
                  tabindex={props.store.approval() === option.value ? 0 : -1}
                  onClick={() => props.store.setApproval(option.value)}
                >
                  <span class="settings-menu-check"><Show when={props.store.approval() === option.value}><Icon name="check" /></Show></span>
                  {option.label}
                </button>
              )}
            </For>
          </div>
        </div>
      )}
    </Popover>
  )
}

export function AgentToolbar(props: { store: SessionStore }) {
  const report = () => {
    const id = props.store.state.selectedId
    return id === null ? undefined : props.store.state.loaded[id]?.context ?? undefined
  }
  return (
    <>
      <Show when={report()} keyed>{(current) => <ContextMeter report={current} />}</Show>
      <ModelPicker store={props.store} />
      <SettingsMenu store={props.store} />
    </>
  )
}

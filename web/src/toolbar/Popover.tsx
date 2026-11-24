import { createSignal, type JSX, onCleanup, Show } from "solid-js"

export type PopoverTrigger = {
  readonly label: string
  readonly content: JSX.Element
  readonly class: string
  // A reason makes the trigger inert and shows the reason on hover
  readonly disabledReason?: string
}

export function Popover(props: {
  trigger: PopoverTrigger
  panelLabel: string
  children: (close: () => void) => JSX.Element
}) {
  const [open, setOpen] = createSignal(false)
  let root!: HTMLDivElement
  let button!: HTMLButtonElement

  function close(): void {
    setOpen(false)
    button.focus()
  }

  const onPointerDown = (event: PointerEvent) => {
    if (open() && event.target instanceof Node && !root.contains(event.target)) setOpen(false)
  }
  document.addEventListener("pointerdown", onPointerDown)
  onCleanup(() => document.removeEventListener("pointerdown", onPointerDown))

  return (
    <div
      ref={root}
      class="popover-root"
      onKeyDown={(event) => {
        if (event.key !== "Escape" || !open()) return
        event.stopPropagation()
        close()
      }}
    >
      <button
        ref={button}
        class={props.trigger.class}
        type="button"
        aria-label={props.trigger.label}
        aria-haspopup="true"
        aria-expanded={open()}
        aria-disabled={props.trigger.disabledReason !== undefined}
        title={props.trigger.disabledReason ?? props.trigger.label}
        onClick={() => {
          if (props.trigger.disabledReason === undefined) setOpen((value) => !value)
        }}
      >
        {props.trigger.content}
      </button>
      <Show when={open()}>
        <div class="popover" role="dialog" aria-label={props.panelLabel}>
          {props.children(close)}
        </div>
      </Show>
    </div>
  )
}

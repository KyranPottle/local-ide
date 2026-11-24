const KEYBOARD_STEP = 16

export function ResizeSeparator(props: {
  label: string
  width: number
  minimum: number
  maximum: number
  // 1 when a move to the right widens the panel, -1 when a move to the left widens it
  growth: 1 | -1
  onResize: (width: number) => void
}) {
  function onPointerDown(event: PointerEvent & { currentTarget: HTMLDivElement }) {
    const separator = event.currentTarget
    const startX = event.clientX
    const startWidth = props.width
    const onPointerMove = (move: PointerEvent) => props.onResize(startWidth + (move.clientX - startX) * props.growth)
    event.preventDefault()
    separator.setPointerCapture(event.pointerId)
    separator.addEventListener("pointermove", onPointerMove)
    separator.addEventListener(
      "lostpointercapture",
      () => separator.removeEventListener("pointermove", onPointerMove),
      { once: true },
    )
  }

  function onKeyDown(event: KeyboardEvent) {
    const steps: Record<string, number> = { ArrowLeft: -KEYBOARD_STEP, ArrowRight: KEYBOARD_STEP }
    if (!Object.hasOwn(steps, event.key)) return
    event.preventDefault()
    props.onResize(props.width + (steps[event.key] ?? 0) * props.growth)
  }

  return (
    <div
      class="resize-separator"
      role="separator"
      aria-orientation="vertical"
      aria-label={props.label}
      aria-valuenow={props.width}
      aria-valuemin={props.minimum}
      aria-valuemax={props.maximum}
      tabindex="0"
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
    />
  )
}

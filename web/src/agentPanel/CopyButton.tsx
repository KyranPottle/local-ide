import { createSignal, onCleanup } from "solid-js"
import { Icon } from "../design/Icon"
import { reportFailure } from "../layout/failureNotice"

const CONFIRMATION_MILLISECONDS = 1200

export function CopyButton(props: { text: string, label: string }) {
  const [copied, setCopied] = createSignal(false)
  let timer: ReturnType<typeof setTimeout> | undefined
  onCleanup(() => clearTimeout(timer))

  async function copy(): Promise<void> {
    await navigator.clipboard.writeText(props.text)
    setCopied(true)
    clearTimeout(timer)
    timer = setTimeout(() => setCopied(false), CONFIRMATION_MILLISECONDS)
  }

  return (
    <button
      class="icon-button icon-button-small"
      type="button"
      aria-label={copied() ? "Copied" : props.label}
      title={copied() ? "Copied" : props.label}
      onClick={() => copy().catch(reportFailure)}
    >
      <Icon name={copied() ? "check" : "copy"} />
    </button>
  )
}

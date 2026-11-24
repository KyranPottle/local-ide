import { colorScheme, toggleColorScheme } from "./colorScheme"
import { Icon } from "./Icon"

export function ColorSchemeToggle() {
  const label = () => (colorScheme() === "dark" ? "Switch to light theme" : "Switch to dark theme")

  return (
    <button class="icon-button" type="button" aria-label={label()} title={label()} onClick={toggleColorScheme}>
      <Icon name={colorScheme() === "dark" ? "sun" : "moon"} />
    </button>
  )
}

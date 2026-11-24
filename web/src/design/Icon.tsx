const iconPaths = {
  chevron: "M6 3.5 10.5 8 6 12.5",
  file: "M4.25 1.75h4.5l3 3v9.5h-7.5zM8.75 1.75v3h3",
  folder: "M1.75 4.25a1 1 0 0 1 1-1H6.5L8 5h5.25a1 1 0 0 1 1 1v6.25a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1z",
  close: "M4.25 4.25l7.5 7.5M11.75 4.25l-7.5 7.5",
  refresh: "M13 8a5 5 0 1 1-1.46-3.54M13 2.5v2.75h-2.75",
  sun: "M8 5.25a2.75 2.75 0 1 0 0 5.5 2.75 2.75 0 0 0 0-5.5zM8 1.5V3M8 13v1.5M1.5 8H3M13 8h1.5M3.4 3.4l1.07 1.07M11.53 11.53l1.07 1.07M3.4 12.6l1.07-1.07M11.53 4.47l1.07-1.07",
  moon: "M13.25 9.6A5.5 5.5 0 0 1 6.4 2.75a5.5 5.5 0 1 0 6.85 6.85z",
  sidebar: "M2.75 2.75h10.5a1 1 0 0 1 1 1v8.5a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1v-8.5a1 1 0 0 1 1-1zM6 2.75v10.5",
  rightPanel: "M2.75 2.75h10.5a1 1 0 0 1 1 1v8.5a1 1 0 0 1-1 1H2.75a1 1 0 0 1-1-1v-8.5a1 1 0 0 1 1-1zM10 2.75v10.5",
  plus: "M8 3v10M3 8h10",
  copy: "M5.75 5.75h7.5v7.5h-7.5zM10.25 5.75v-3h-7.5v7.5h3",
  check: "M3.25 8.5l3 3 6.5-7",
  fork: "M4.75 2.5v11M4.75 9.5c0-2.5 6.5-1.5 6.5-4.5V2.5",
  send: "M8 13V3M3.75 7.25 8 3l4.25 4.25",
  stop: "M4.5 4.5h7v7h-7z",
  attach: "M10.75 5.25 6 10a1.25 1.25 0 0 0 1.77 1.77l5.3-5.3a2.75 2.75 0 0 0-3.9-3.9l-5.3 5.3a4.25 4.25 0 0 0 6 6L14 9.75",
  pencil: "M10.25 2.75l3 3-7.75 7.75H2.5v-3zM8.75 4.25l3 3",
  trash: "M2.75 4.25h10.5M6.25 4.25v-1.5h3.5v1.5M4 4.25l.75 9h6.5l.75-9",
  search: "M7 2.75a4.25 4.25 0 1 0 0 8.5 4.25 4.25 0 0 0 0-8.5zM10.25 10.25 13.5 13.5",
  delegate: "M2.75 4.25h6.5M2.75 8h4M2.75 11.75h4M10.5 8.75l2.75 2.75-2.75 2.75M7.5 11.5h5.75",
  settings: "M2.5 4.5h11M2.5 11.5h11M5.5 2.75v3.5M10.5 9.75v3.5",
  arrowDown: "M8 3v10M3.75 8.75 8 13l4.25-4.25",
} as const

export type IconName = keyof typeof iconPaths

export function Icon(props: { name: IconName }) {
  return (
    <svg
      class="icon"
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      stroke-width="1.5"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d={iconPaths[props.name]} />
    </svg>
  )
}

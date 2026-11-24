const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
const WEEK = 7 * DAY

const twoDigits = (value: number) => String(value).padStart(2, "0")

// A time in the future, from a clock skew, counts as now
export function formatRelativeTime(at: number, now: number): string {
  const elapsed = now - at
  if (elapsed < MINUTE) return "now"
  if (elapsed < HOUR) return `${Math.floor(elapsed / MINUTE)} min`
  if (elapsed < DAY) return `${Math.floor(elapsed / HOUR)} h`
  if (elapsed < WEEK) return `${Math.floor(elapsed / DAY)} d`
  const date = new Date(at)
  return `${date.getFullYear()}-${twoDigits(date.getMonth() + 1)}-${twoDigits(date.getDate())}`
}

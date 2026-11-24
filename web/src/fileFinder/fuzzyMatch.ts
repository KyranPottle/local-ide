const CONSECUTIVE_BONUS = 16
const SEGMENT_START_BONUS = 12
const WORD_START_BONUS = 8
const FILE_NAME_BONUS = 4
const GAP_PENALTY = 1

function positionBonus(candidate: string, index: number, fileNameStart: number): number {
  const fileNameBonus = index >= fileNameStart ? FILE_NAME_BONUS : 0
  const previous = candidate.charAt(index - 1)
  if (index === 0 || previous === "/") return SEGMENT_START_BONUS + fileNameBonus
  const startsWord = "._- ".includes(previous) || /[a-z][A-Z]/.test(previous + candidate.charAt(index))
  return startsWord ? WORD_START_BONUS + fileNameBonus : fileNameBonus
}

export function fuzzyScore(query: string, candidate: string): number | undefined {
  const needle = query.toLowerCase()
  const haystack = candidate.toLowerCase()
  const fileNameStart = candidate.lastIndexOf("/") + 1

  // matchedAt[j] is the best score with the last processed query character at j
  // reachedAt[j] is the best score with that character at or before j, less the gap after it
  let matchedAt = new Array<number>(haystack.length).fill(0)
  let reachedAt = new Array<number>(haystack.length).fill(0)

  for (let queryIndex = 0; queryIndex < needle.length; queryIndex++) {
    const nextMatchedAt = new Array<number>(haystack.length).fill(-Infinity)
    const nextReachedAt = new Array<number>(haystack.length).fill(-Infinity)
    for (let index = queryIndex; index < haystack.length; index++) {
      if (haystack.charAt(index) === needle.charAt(queryIndex)) {
        const continued = (matchedAt[index - 1] ?? -Infinity) + CONSECUTIVE_BONUS
        const resumed = reachedAt[index - 1] ?? -Infinity
        const before = queryIndex === 0 ? 0 : Math.max(continued, resumed)
        nextMatchedAt[index] = before + positionBonus(candidate, index, fileNameStart)
      }
      nextReachedAt[index] = Math.max(
        nextMatchedAt[index] ?? -Infinity,
        (nextReachedAt[index - 1] ?? -Infinity) - GAP_PENALTY,
      )
    }
    matchedAt = nextMatchedAt
    reachedAt = nextReachedAt
  }

  const score = Math.max(...matchedAt)
  return Number.isFinite(score) ? score : undefined
}

export function rankPaths(query: string, paths: readonly string[], limit: number): string[] {
  if (query === "") return paths.slice(0, limit)
  return paths
    .flatMap((path) => {
      const score = fuzzyScore(query, path)
      return score === undefined ? [] : [{ path, score }]
    })
    .sort((a, b) => b.score - a.score || a.path.length - b.path.length || Number(a.path > b.path) - Number(a.path < b.path))
    .slice(0, limit)
    .map((ranked) => ranked.path)
}

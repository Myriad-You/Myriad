const MAX_ONBOARDING_TAGS = 28
const MAX_ONBOARDING_TAG_CHARS = 24

export function keepSelectedPersonaTags(
  selected: string[],
  deckLabels: readonly string[],
): string[] {
  const labels = new Set(deckLabels)
  return selected.filter((label) => labels.has(label))
}

export function uniqPersonaTags(values: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const raw of values) {
    const tag = raw.replace(/\s+/g, ' ').trim()
    if (tag.length < 2 || tag.length > MAX_ONBOARDING_TAG_CHARS) continue
    const key = tag.toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    out.push(tag)
    if (out.length >= MAX_ONBOARDING_TAGS) break
  }
  return out
}

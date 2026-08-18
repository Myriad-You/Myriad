export function uniqPersonaTags(values: string[]): string[] {
  const seen = new Set<string>()
  const out: string[] = []
  for (const raw of values) {
    const tag = raw.replace(/\s+/g, ' ').trim()
    if (tag.length < 2 || tag.length > 40) continue
    const key = tag.toLowerCase()
    if (seen.has(key)) continue
    seen.add(key)
    out.push(tag)
    if (out.length >= 24) break
  }
  return out
}

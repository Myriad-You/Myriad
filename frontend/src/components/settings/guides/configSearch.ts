export interface ConfigSearchableItem {
  type: string
  section: string
  title: string
  description: string
  keywords: string[]
  haystack?: string
  guidePath?: string
}

export type RankedSearchItem = ConfigSearchableItem & {
  score: number
  matchSnippet?: string
}

export function parseSearchQuery(raw: string): string[] {
  return raw
    .toLowerCase()
    .trim()
    .split(/\s+/u)
    .map((t) => t.trim())
    .filter((t) => t.length > 0)
}

function buildHaystack(item: ConfigSearchableItem): string {
  if (item.haystack) return item.haystack
  const parts = [
    item.title,
    item.description,
    ...item.keywords.map((k) => String(k)),
  ]
  return parts.join('\n').toLowerCase()
}

export function itemMatchesQuery(
  item: ConfigSearchableItem,
  tokens: string[],
): boolean {
  if (tokens.length === 0) return false
  const title = item.title.toLowerCase()
  const desc = item.description.toLowerCase()
  const hay = buildHaystack(item)
  const kws = item.keywords.map((k) => k.toLowerCase())

  return tokens.every((tok) => {
    if (title.includes(tok) || desc.includes(tok) || hay.includes(tok)) {
      return true
    }
    return kws.some((k) => k.includes(tok) || tok.includes(k))
  })
}

function scoreToken(
  tok: string,
  title: string,
  desc: string,
  hay: string,
  keywords: string[],
): number {
  let s = 0
  if (title === tok) { s += 24
}
  else if (title.startsWith(tok)) { s += 14
}
  else if (title.includes(tok)) {
    const i = title.indexOf(tok)
    s += i <= 2 ? 10 : 7
  }

  if (desc.includes(tok)) {
    const i = desc.indexOf(tok)
    s += i <= 8 ? 4 : 2.5
  }

  for (const k of keywords) {
    if (k === tok) {
      s += 6
      break
    }
    if (k.includes(tok) && tok.length >= 2) {
      s += 2
      break
    }
  }

  const inTitleOrDesc = title.includes(tok) || desc.includes(tok)
  if (!inTitleOrDesc && hay.includes(tok)) {
    s += tok.length >= 4 ? 1.2 : tok.length >= 2 ? 0.6 : 0.2
  }

  return s
}

export function extractMatchSnippet(
  haystack: string,
  tokens: string[],
  fallback: string,
  radius = 28,
): string {
  const hay = haystack.replaceAll(/\s+/g, ' ').trim()
  if (!hay) return fallback

  let bestIdx = -1
  let bestTok = tokens[0] ?? ''
  for (const tok of tokens) {
    const i = hay.indexOf(tok)
    if (i >= 0 && (bestIdx < 0 || i < bestIdx)) {
      bestIdx = i
      bestTok = tok
    }
  }
  if (bestIdx < 0) {
    const f = fallback.replaceAll(/\s+/g, ' ').trim()
    return f.length > 72 ? `${f.slice(0, 71)}…` : f
  }

  let start = Math.max(0, bestIdx - radius)
  const end = Math.min(hay.length, bestIdx + bestTok.length + radius)
  if (start > 0) {
    const cut = hay.slice(start, bestIdx).search(/[。！？；;,.、\s]/u)
    if (cut >= 0) start = start + cut + 1
  }
  let snippet = hay.slice(start, end).trim()
  if (start > 0) snippet = `…${snippet}`
  if (end < hay.length) snippet = `${snippet}…`
  return snippet
}

export function scoreSearchItem(
  item: ConfigSearchableItem,
  tokens: string[],
): RankedSearchItem | null {
  if (!itemMatchesQuery(item, tokens)) return null

  const title = item.title.toLowerCase()
  const desc = item.description.toLowerCase()
  const hay = buildHaystack(item)
  const kws = item.keywords.map((k) => k.toLowerCase())

  let score = 0
  for (const tok of tokens) {
    score += scoreToken(tok, title, desc, hay, kws)
  }

  if (tokens.length > 1 && tokens.every((t) => title.includes(t))) {
    score += 8
  }
  if (
    tokens.length > 1 &&
    tokens.every((t) => title.includes(t) || desc.includes(t))
  ) {
    score += 3
  }

  if (item.type === 'section') score += 1.2
  else if (item.type === 'platform') score += 0.9
  else if (item.type === 'alias') score += 0.4
  else if (item.type === 'guide') score += 0.15

  if (
    item.type === 'guide' &&
    tokens.every((t) => t.length <= 2) &&
    !tokens.some((t) => title.includes(t))
  ) {
    score *= 0.55
  }

  const matchSnippet = extractMatchSnippet(hay, tokens, item.description)

  return { ...item, score, matchSnippet }
}

export interface RankOptions {
  maxResults?: number
  maxGuidesPerSection?: number
}

/** AND + score desc + per-section cap */
export function rankConfigSearch(
  items: ConfigSearchableItem[],
  rawQuery: string,
  opts: RankOptions = {},
): RankedSearchItem[] {
  const { maxResults = 36, maxGuidesPerSection = 4 } = opts
  const tokens = parseSearchQuery(rawQuery)
  if (tokens.length === 0) return []

  const ranked = items
    .map((item) => scoreSearchItem(item, tokens))
    .filter((x): x is RankedSearchItem => x != null && x.score > 0)
    .toSorted((a, b) => {
      if (b.score !== a.score) return b.score - a.score
      const typeOrder = (t: string) =>
        t === 'section' ? 0 : t === 'platform' ? 1 : t === 'alias' ? 2 : 3
      const d = typeOrder(a.type) - typeOrder(b.type)
      if (d !== 0) return d
      return a.title.length - b.title.length
    })

  const guideCountBySection = new Map<string, number>()
  const out: RankedSearchItem[] = []

  for (const item of ranked) {
    if (item.type === 'guide') {
      const n = guideCountBySection.get(item.section) ?? 0
      if (n >= maxGuidesPerSection) continue
      guideCountBySection.set(item.section, n + 1)
    }
    out.push(item)
    if (out.length >= maxResults) break
  }

  return out
}

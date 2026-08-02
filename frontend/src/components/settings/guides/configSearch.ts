/**
 * 配置侧栏搜索：匹配、相关度排序、摘要与结果裁剪。
 * 供 ConfigForm 使用；与 guideSearchIndex 配合。
 */

export interface ConfigSearchableItem {
  type: string
  section: string
  title: string
  description: string
  /** 短关键词（不要求小写） */
  keywords: string[]
  /**
   * 全文检索串（指南 what/chain/frontend/notes 等，小写）。
   * 有则优先用于 includes 与摘要。
   */
  haystack?: string
  /** 指南目录路径（type=guide） */
  guidePath?: string
}

export type RankedSearchItem = ConfigSearchableItem & {
  /** 排序分 */
  score: number
  /** 命中上下文摘要（可选覆盖 description 展示） */
  matchSnippet?: string
}

/** 拆查询：空白分词，过滤空串 */
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

/** 是否命中（多词 AND：每个 token 都要出现在 haystack/title/desc/keywords 之一） */
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

/**
 * 单 token 字段加权分。
 * 标题精确/前缀远高于正文偶然命中，减少指南长文噪音。
 */
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
    // 标题中靠前的命中更好
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

  // 仅在全文（指南正文）出现：弱分
  const inTitleOrDesc = title.includes(tok) || desc.includes(tok)
  if (!inTitleOrDesc && hay.includes(tok)) {
    // 长 token 更可信
    s += tok.length >= 4 ? 1.2 : tok.length >= 2 ? 0.6 : 0.2
  }

  return s
}

/** 从 haystack 截取含首个 token 的可读摘要 */
export function extractMatchSnippet(
  haystack: string,
  tokens: string[],
  fallback: string,
  radius = 28,
): string {
  const hay = haystack.replace(/\s+/g, ' ').trim()
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
    const f = fallback.replace(/\s+/g, ' ').trim()
    return f.length > 72 ? `${f.slice(0, 71)}…` : f
  }

  let start = Math.max(0, bestIdx - radius)
  const end = Math.min(hay.length, bestIdx + bestTok.length + radius)
  // 尽量落在标点边界
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

  // 多词全在标题
  if (tokens.length > 1 && tokens.every((t) => title.includes(t))) {
    score += 8
  }
  // 多词全在标题+描述
  if (
    tokens.length > 1 &&
    tokens.every((t) => title.includes(t) || desc.includes(t))
  ) {
    score += 3
  }

  // 类型微调
  if (item.type === 'section') score += 1.2
  else if (item.type === 'platform') score += 0.9
  else if (item.type === 'alias') score += 0.4
  else if (item.type === 'guide') score += 0.15

  // 过短查询时压低「仅正文」类指南洪水：token 长度 1–2 且未进标题
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
  /** 总结果上限 */
  maxResults?: number
  /** 同一 section 下最多保留几条指南 */
  maxGuidesPerSection?: number
}

/**
 * 过滤 + 排序 + 裁剪。
 * - 多词 AND
 * - 相关度降序
 * - 每 section 指南条数上限，避免一屏全是指南
 */
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
    .sort((a, b) => {
      if (b.score !== a.score) return b.score - a.score
      // 同分：短标题优先、分区优先于指南
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

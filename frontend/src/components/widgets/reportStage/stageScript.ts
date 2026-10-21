/**
 * 舞台播的剧本：几幕，每幕若干句台词。新报告直接带 `script`（后端分好章、句，
 * 句子可以指向一条事实）；旧报告只有 summary / insights，退回成「回响」「洞察」两幕。
 */

export interface StageFact {
  id: string
  label: string
  value: string
}

export interface StageBeat {
  text: string
  fact?: StageFact
  /** 这句出现后停多久再出下一句。 */
  holdMs: number
}

export interface StageChapter {
  key: string
  title: string
  beats: StageBeat[]
}

export interface StageReport {
  platform: string
  headline?: string
  chapters: StageChapter[]
  cardVisuals: unknown
  /** 风格改过，报告还是旧风格写的。 */
  styleStale: boolean
}

const CONTROL_CHARS = /[\u0000-\u001F\u007F-\u009F]/g
const MARKDOWN_SYMBOLS = /[*_~`#>]/g
const WHITESPACE = /\s+/g
// 旧报告没分句，只按句末标点切；逗号不切，免得一句话碎成几截。
const SENTENCE_END = /([。！？!?；;]|\.(?=\s|$))/g

export function cleanStageText(text: string): string {
  return text
    .replaceAll(CONTROL_CHARS, '')
    .replaceAll(MARKDOWN_SYMBOLS, '')
    .replaceAll(WHITESPACE, ' ')
    .trim()
}

function splitSentences(text: string): string[] {
  return cleanStageText(text)
    .replaceAll(SENTENCE_END, '$1￿')
    .split('￿')
    .map((part) => part.trim())
    .filter(Boolean)
}

/** 按字数给停留时间：短句不拖，长句读得完。 */
export function beatHoldMs(text: string): number {
  const length = Array.from(text).length
  return Math.min(5200, Math.max(1400, 700 + length * 70))
}

/** 一幕播完后留给最后一句的余韵。 */
export const CHAPTER_TAIL_MS = 2400

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}

function asText(value: unknown): string {
  if (typeof value === 'string') return value
  if (typeof value === 'number' && Number.isFinite(value)) return String(value)
  return ''
}

function readFacts(raw: unknown): Map<string, StageFact> {
  const facts = new Map<string, StageFact>()
  if (!Array.isArray(raw)) return facts
  for (const item of raw) {
    const record = asRecord(item)
    const id = asText(record?.id)
    const label = cleanStageText(asText(record?.label))
    const value = cleanStageText(asText(record?.value))
    if (id && label && value) facts.set(id, { id, label, value })
  }
  return facts
}

function beat(text: string, fact?: StageFact): StageBeat {
  return { text, fact, holdMs: beatHoldMs(text) }
}

function chaptersFromScript(
  script: Record<string, unknown>,
  facts: Map<string, StageFact>,
  titles: Record<string, string>,
): StageChapter[] {
  const chapters: StageChapter[] = []
  for (const raw of Array.isArray(script.chapters) ? script.chapters : []) {
    const chapter = asRecord(raw)
    if (!chapter) continue
    const key = asText(chapter.key) || `chapter-${chapters.length + 1}`
    const beats = (Array.isArray(chapter.beats) ? chapter.beats : [])
      .map((rawBeat) => {
        const record = asRecord(rawBeat)
        const text = cleanStageText(asText(record?.text))
        if (!text) return null
        return beat(text, facts.get(asText(record?.fact)))
      })
      .filter((item): item is StageBeat => item !== null)
    if (beats.length === 0) continue
    const title = cleanStageText(asText(chapter.title)) || titles[key] || key
    chapters.push({ key, title, beats })
  }
  return chapters
}

function chaptersFromLegacy(
  report: Record<string, unknown>,
  titles: { echo: string; insight: string },
): StageChapter[] {
  const chapters: StageChapter[] = []
  const summary = splitSentences(asText(report.summary)).map((text) => beat(text))
  if (summary.length > 0) {
    chapters.push({ key: 'echo', title: titles.echo, beats: summary })
  }
  const insights = (Array.isArray(report.insights) ? report.insights : [])
    .flatMap((item) => splitSentences(asText(item)))
    .map((text) => beat(text))
  if (insights.length > 0) {
    chapters.push({ key: 'insight', title: titles.insight, beats: insights })
  }
  return chapters
}

/** 把 `/api/reports/latest` 里的一条平台报告整理成舞台剧本。没东西可播时返回 null。 */
export function buildStageReport(
  raw: unknown,
  titles: { echo: string; insight: string },
): StageReport | null {
  const report = asRecord(raw)
  const platform = asText(report?.platform)
  if (!report || !platform) return null
  const script = asRecord(report.script)
  const facts = readFacts(report.facts)
  const fromScript = script ? chaptersFromScript(script, facts, titles) : []
  const chapters = fromScript.length > 0 ? fromScript : chaptersFromLegacy(report, titles)
  if (chapters.length === 0) return null
  const headline = script ? cleanStageText(asText(script.headline)) : ''
  return {
    platform,
    headline: headline || undefined,
    chapters,
    cardVisuals: report.card_visuals ?? null,
    styleStale: report.style_stale === true,
  }
}

/** 一幕之内，前 n 句一共要播多久（含末句余韵）。 */
export function chapterDurationMs(chapter: StageChapter): number {
  return chapter.beats.reduce((total, item) => total + item.holdMs, 0) + CHAPTER_TAIL_MS
}

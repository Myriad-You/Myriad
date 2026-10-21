import type { StageMedia } from './stageMedia'
import { mediaMentionedIn, readStageMedia } from './stageMedia'

/**
 * 舞台播的剧本：几幕，每幕若干句台词。新报告直接带 `script`（后端分好章、句，
 * 句子可以指向一条事实）；旧报告只有 summary / insights，退回成「回响」「洞察」两幕。
 */

export interface StageFact {
  id: string
  label: string
  value: string
  /** 封面、头像之类，已经过站内图片代理。 */
  media?: string
}

/** 剧本里要本地化的文字。后端事实的 label 是写给模型看的英文，界面按 id 取文案。 */
export interface StageCopy {
  echo: string
  insight: string
  facts?: Record<string, string>
  /** 枚举型事实的取值（如贡献等级）按 id → 值 → 文字本地化。 */
  factValues?: Record<string, Record<string, string>>
  chapters?: Record<string, string>
  formatNumber?: (value: number) => string
}

export interface StageBeat {
  text: string
  fact?: StageFact
  /** 这句提到的作品的封面（事实自带的图排最前）。 */
  media?: StageMedia[]
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
  /** 这个平台带封面的作品，台词没点名时当书架摆出来。 */
  media: StageMedia[]
  /** 风格改过，报告还是旧风格写的。 */
  styleStale: boolean
}

const CONTROL_CHARS = /[\u0000-\u001F\u007F-\u009F]/g
const WHITESPACE = /\s+/g
// 只去成对的标记和行首标记：「C#」「my_repo」「5 * 3」里的符号不动。
const LINE_MARKERS = /^[ \t]{0,3}(?:#{1,6}|[>\-*+])[ \t]+/gm
const PAIRED_MARKUP = /(\*\*|__|~~|`)(?=\S)(.+?)(?<=\S)\1/g
const SINGLE_STARS = /(^|[\s(（])\*(?=\S)([^*\n]+)(?<=\S)\*(?=$|[\s).,!?;:，。！？）])/gm
// 旧报告没分句，只按句末标点切；逗号不切，免得一句话碎成几截。
const SENTENCE_END = /([。！？!?；;]|\.(?=\s|$))/g

/** 事实原样显示（「C#」「my_repo」都是正经值），只去控制字符、并空白。 */
export function cleanFactText(text: string): string {
  return text
    .replaceAll(CONTROL_CHARS, ' ')
    .replaceAll(WHITESPACE, ' ')
    .trim()
}

/** 台词：模型偶尔仍带 markdown，去成对的标记和行首标记，单个符号不动。 */
export function cleanStageText(text: string): string {
  return cleanFactText(
    text
      .replaceAll(LINE_MARKERS, '')
      .replaceAll(PAIRED_MARKUP, '$2')
      .replaceAll(SINGLE_STARS, '$1$2'),
  )
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

// 只认站内路径或 http(s)，别的（data:、javascript:）一律不显示。
// 站内绝对路径或 http(s)。「//host」「/\\host」浏览器都当成别的站，不放行。
const SAFE_MEDIA = /^(\/(?![/\\])|https?:\/\/)/i

function readFacts(raw: unknown, copy: StageCopy): Map<string, StageFact> {
  const facts = new Map<string, StageFact>()
  if (!Array.isArray(raw)) return facts
  for (const item of raw) {
    const record = asRecord(item)
    const id = asText(record?.id)
    const label = copy.facts?.[id] || cleanFactText(asText(record?.label))
    const rawValue = record?.value
    const value = cleanFactText(
      typeof rawValue === 'number' && Number.isFinite(rawValue) && copy.formatNumber
        ? copy.formatNumber(rawValue)
        : copy.factValues?.[id]?.[asText(rawValue)] ?? asText(rawValue),
    )
    const media = asText(record?.media).trim()
    if (id && label && value) {
      facts.set(id, SAFE_MEDIA.test(media) ? { id, label, value, media } : { id, label, value })
    }
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
  titles: StageCopy,
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

function attachMedia(target: StageBeat, media: readonly StageMedia[]) {
  const mentioned = mediaMentionedIn(target.text, media)
  const fromFact = target.fact?.media
    ? [{ title: target.fact.value, cover: target.fact.media, shape: 'square' as const }]
    : []
  const all = [...fromFact, ...mentioned.filter((item) => item.cover !== fromFact[0]?.cover)]
  if (all.length > 0) target.media = all
}

/** 把 `/api/reports/latest` 里的一条平台报告整理成舞台剧本。没东西可播时返回 null。 */
export function buildStageReport(
  raw: unknown,
  copy: StageCopy,
): StageReport | null {
  const report = asRecord(raw)
  const platform = asText(report?.platform)
  if (!report || !platform) return null
  const script = asRecord(report.script)
  const facts = readFacts(report.facts, copy)
  const fromScript = script ? chaptersFromScript(script, facts, copy.chapters ?? {}) : []
  const chapters = fromScript.length > 0 ? fromScript : chaptersFromLegacy(report, copy)
  if (chapters.length === 0) return null
  const headline = script ? cleanStageText(asText(script.headline)) : ''
  const media = readStageMedia(report.card_visuals, platform)
  for (const chapter of chapters) {
    for (const item of chapter.beats) attachMedia(item, media)
  }
  return {
    platform,
    headline: headline || undefined,
    chapters,
    media,
    styleStale: report.style_stale === true,
  }
}

/** 一幕之内，前 n 句一共要播多久（含末句余韵）。 */
export function chapterDurationMs(chapter: StageChapter): number {
  return chapter.beats.reduce((total, item) => total + item.holdMs, 0) + CHAPTER_TAIL_MS
}

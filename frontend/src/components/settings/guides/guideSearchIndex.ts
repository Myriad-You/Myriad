/**
 * 将选项指南目录展平为配置搜索索引条目。
 * 搜索命中后跳转到对应一级设置页（与 ConfigForm searchableContent 对齐）。
 */

import type { Locale } from '../../../i18n'
import type { SettingGuideEntry, SettingGuidesCatalog } from './types'
import { getSettingGuidesCatalog } from './catalog'

/** 指南分区 → ConfigForm 一级 section id */
export const GUIDE_CATALOG_TO_SECTION: Record<
  keyof SettingGuidesCatalog,
  string
> = {
  ui: 'basic',
  modules: 'modules',
  platforms: 'platforms',
  notifications: 'notifications',
  ai: 'ai',
  oauth: 'oauth',
  permissions: 'permissions',
  users: 'users',
  advanced: 'advanced',
  federation: 'federation',
  /** 更新器内联在关于页 */
  updater: 'about',
  about: 'about',
  /** Tapp 详情页不在 /config 内；索引阶段跳过 */
  tapp: '',
}

export interface GuideSearchEntry {
  type: 'guide'
  section: string
  /** 列表主标题：取自 what 首句 */
  title: string
  /** 副文案：frontend 或 notes */
  description: string
  /** 精简关键词（英文词 / 有意义中文块，不含滑动窗洪水） */
  keywords: string[]
  /** 全文小写（匹配 + 摘要） */
  haystack: string
  /** 目录路径，如 permissions.agentPreset */
  guidePath: string
}

function entryFields(entry: SettingGuideEntry): string[] {
  return [entry.what, entry.chain, entry.frontend, entry.notes].filter(
    (s): s is string => typeof s === 'string' && s.trim().length > 0,
  )
}

/** 截断标题：去掉编号前缀，取首句或前 max 字 */
export function guideEntryTitle(what: string, max = 42): string {
  const cleaned = what
    .replace(/^[①②③④⑤⑥⑦⑧⑨⑩\d]+[).、\s]*/u, '')
    .trim()
  const first = cleaned.split(/[。！？\n]/u)[0]?.trim() || cleaned
  if (first.length <= max) return first
  return `${first.slice(0, max - 1)}…`
}

/**
 * 精简分词：按标点/空白切，保留 ≥2 的片段。
 * 不再做全量 CJK 2–4 字滑动窗（噪音大、误匹配多）。
 * 全文匹配依赖 haystack.includes。
 */
export function tokenizeForSearch(text: string): string[] {
  const lower = text.toLowerCase()
  const parts = lower
    .split(/[^\p{L}\p{N}+#./:_-]+/u)
    .map((s) => s.trim())
    .filter((s) => s.length >= 2)
  // 中文整句里再抽 2–3 字「词块」仅从已切分的中文段，限制数量
  const extra: string[] = []
  for (const p of parts) {
    if (!/^[\u4E00-\u9FFF\u3040-\u30FF]+$/u.test(p)) continue
    if (p.length <= 4) continue
    // 段首 2–3 字常是主题词
    extra.push(p.slice(0, 2), p.slice(0, 3))
    if (p.length >= 4) extra.push(p.slice(0, 4))
  }
  return [...new Set([...parts, ...extra])]
}

export function buildGuideSearchIndex(locale: Locale): GuideSearchEntry[] {
  const catalog = getSettingGuidesCatalog(locale)
  const out: GuideSearchEntry[] = []

  for (const [area, group] of Object.entries(catalog) as Array<
    [
      keyof SettingGuidesCatalog,
      SettingGuidesCatalog[keyof SettingGuidesCatalog],
    ]
  >) {
    const section = GUIDE_CATALOG_TO_SECTION[area]
    if (!section || !group || typeof group !== 'object') continue

    for (const [key, entry] of Object.entries(group) as Array<
      [string, SettingGuideEntry]
    >) {
      if (!entry?.what) continue
      const fields = entryFields(entry)
      const blob = fields.join('\n')
      const haystack = blob.toLowerCase().replace(/\s+/g, ' ').trim()
      const tokens = tokenizeForSearch(blob)
      // path 片段也加入 keywords（如 agentPreset）
      tokens.push(key.toLowerCase(), area.toLowerCase())

      out.push({
        type: 'guide',
        section,
        title: guideEntryTitle(entry.what),
        description:
          entry.frontend?.split('\n')[0]?.trim() ||
          entry.notes?.split('\n')[0]?.trim() ||
          entry.what,
        keywords: [...new Set(tokens)],
        haystack,
        guidePath: `${area}.${key}`,
      })
    }
  }

  return out
}

/**
 * 将某一级 section 下指南中的「精简关键词」并入分区 keywords。
 */
export function guideKeywordsForSection(
  locale: Locale,
  sectionId: string,
): string[] {
  const entries = buildGuideSearchIndex(locale).filter(
    (e) => e.section === sectionId,
  )
  const set = new Set<string>()
  for (const e of entries) {
    // 只取较短、信息密度高的词，避免把整段 haystack 塞进 section
    for (const k of e.keywords) {
      if (k.length >= 2 && k.length <= 16) set.add(k)
    }
  }
  return Array.from(set)
}

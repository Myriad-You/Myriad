/** 分类预置值走库里的「友情链接 / 我」，不走界面语言。 */

import type { BrewSource, SourceType, UpdateSourceRequest } from '../../../../types/brew'
import { brewCategoryParts, PRESET_CATEGORY_DB_VALUES } from '../../constants'

export type SubscriptionMode = 'disabled' | 'normal' | 'brewlia'

export const EDIT_INTERVALS = [15, 30, 60, 120, 360, 720, 1440] as const

export function subscriptionModeOf(
  source: Pick<BrewSource, 'enabled' | 'source_type'>,
): SubscriptionMode {
  if (!source.enabled) return 'disabled'
  if (source.source_type === 'brewlia') return 'brewlia'
  return 'normal'
}

export function canAddCategory(selected: readonly string[]): boolean {
  if (selected.length === 0) return true
  if (selected.length >= 2) return false
  return selected.some((cat) => PRESET_CATEGORY_DB_VALUES.includes(cat))
}

export function joinCategories(
  selected: readonly string[],
  draft = '',
): string | undefined {
  const next = Iterator.from(selected).toArray()
  const extra = draft.trim()
  if (extra && !next.includes(extra) && next.length < 2) next.push(extra)
  return next.length > 0 ? next.join(', ') : undefined
}

export function categoriesOf(source: Pick<BrewSource, 'category'>): string[] {
  return brewCategoryParts(source.category)
}

export function resolveEditSourcePayload(input: {
  source: Pick<BrewSource, 'source_type' | 'feed_type'>
  name: string
  selectedCategories: readonly string[]
  newCategory?: string
  updateInterval: number
  subscriptionMode: SubscriptionMode
  customIcon: string | null
  themeColor: string
  styleTags: readonly string[]
  adminOnly: boolean
}): UpdateSourceRequest {
  const isLink = input.source.source_type === 'link'
  const isNote = input.source.source_type === 'note'
  const isRssHub = input.source.feed_type === 'rsshub'

  let enabled: boolean | undefined
  let sourceType: SourceType | undefined

  if (!isLink && !isNote) {
    enabled = input.subscriptionMode !== 'disabled'
    if (input.subscriptionMode === 'brewlia') { sourceType = 'brewlia'
}
    else if (input.subscriptionMode === 'normal') {
      sourceType = isRssHub ? 'rsshub' : 'rss'
    }
  }

  const payload: UpdateSourceRequest = {
    name: input.name.trim() || undefined,
    category: joinCategories(input.selectedCategories, input.newCategory),
    theme_color: input.themeColor,
    ai_style_tags: Iterator.from(input.styleTags).toArray(),
    admin_only: input.adminOnly,
  }
  if (!isLink && !isNote) {
    payload.update_interval = input.updateInterval
  }
  if (enabled !== undefined) payload.enabled = enabled
  if (sourceType) payload.source_type = sourceType
  if (input.customIcon !== null) payload.icon = input.customIcon
  return payload
}

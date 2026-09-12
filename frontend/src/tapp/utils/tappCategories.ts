import type { TappCategory, TappManifest, WidgetCategory } from '../types'

/** 用途分类的稳定 ID。运行形态与发布阶段不属于此字段。 */
export const TAPP_CATEGORIES = [
  'ai',
  'data',
  'developer',
  'game',
  'media',
  'productivity',
  'social',
  'utility',
] as const satisfies readonly TappCategory[]

export const TAPP_WIDGET_CATEGORIES = TAPP_CATEGORIES

export function parseTappWidgetCategory(
  category: string | null | undefined,
): WidgetCategory | null {
  if (!category) return null
  const id = category.trim().toLowerCase()
  return (TAPP_WIDGET_CATEGORIES as readonly string[]).includes(id)
    ? (id as WidgetCategory)
    : null
}

export const TAPP_CATEGORY_I18N_KEYS = {
  ai: 'categoryAI',
  data: 'categoryData',
  developer: 'categoryDeveloper',
  game: 'categoryGame',
  media: 'categoryMedia',
  productivity: 'categoryProductivity',
  social: 'categorySocial',
  utility: 'categoryUtility',
} as const satisfies Record<TappCategory, string>

const CATEGORY_ALIASES: Record<string, TappCategory> = {
  ai: 'ai',
  data: 'data',
  'data-extension': 'data',
  platform: 'data',
  visualization: 'data',
  developer: 'developer',
  development: 'developer',
  dev: 'developer',
  game: 'game',
  games: 'game',
  entertainment: 'media',
  media: 'media',
  music: 'media',
  productivity: 'productivity',
  communication: 'social',
  social: 'social',
  demo: 'utility',
  page: 'utility',
  test: 'utility',
  tool: 'utility',
  tools: 'utility',
  utilities: 'utility',
  utility: 'utility',
  widget: 'utility',
}

export function normalizeTappCategory(
  category: string | null | undefined,
): TappCategory {
  if (!category) return 'utility'
  return CATEGORY_ALIASES[category.trim().toLowerCase()] ?? 'utility'
}

export function resolveTappCategory(
  manifest: Pick<TappManifest, 'category' | 'permissions'>,
): TappCategory {
  if (manifest.category) return normalizeTappCategory(manifest.category)

  const permissions = manifest.permissions ?? []
  if (permissions.some((permission) => permission.startsWith('ai:'))) {
    return 'ai'
  }
  if (permissions.some((permission) => permission.startsWith('media:'))) {
    return 'media'
  }
  if (permissions.some((permission) => permission.startsWith('platform:'))) {
    return 'data'
  }
  return 'utility'
}

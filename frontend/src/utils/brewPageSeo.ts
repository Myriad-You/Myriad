/**
 * Brew 文章级 SEO（仅站主「自有」内容）
 *
 * 自有判定：订阅源 category 含固定值「我」（见 brew/constants）。
 * 友情链接、外部 RSS、RSSHub 聚合等一律 noindex，避免把别人的文章拿来做站内 SEO。
 */

import type { BrewItem, BrewSource } from '../types/brew'
import type { PageSeoInput } from './siteMetadata'
import {
  brewOwnItemPath,
  isOwnBrewSource,
} from '../components/brew/constants'
import { buildModulePageSeo } from './modulePageSeo'
import { formatPageTitle } from './siteMetadata'

export { brewOwnItemPath, isOwnBrewSource }

function plainTextSnippet(
  htmlOrText: string | null | undefined,
  maxLen = 160,
): string | undefined {
  if (!htmlOrText) return undefined
  const plain = htmlOrText
    .replace(/<[^>]*>/g, ' ')
    .replace(/&nbsp;/g, ' ')
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&quot;/g, '"')
    .replace(/\s+/g, ' ')
    .trim()
  if (!plain) return undefined
  if (plain.length <= maxLen) return plain
  return `${plain.slice(0, maxLen - 1).trimEnd()}…`
}

function pickItemImage(item: BrewItem): string | undefined {
  const raw = item.image?.trim()
  if (!raw || raw.startsWith('data:')) return undefined
  return raw
}

/** 列表/分类态：与 sitemap 中 /brew 一致 */
export function buildBrewListPageSeo(opts: {
  listLabel: string
  listDescription?: string
  moduleOpenToAll: boolean
}): PageSeoInput {
  return buildModulePageSeo({
    label: opts.listLabel,
    description: opts.listDescription,
    path: '/brew',
    moduleOpenToAll: opts.moduleOpenToAll,
  })
}

/**
 * 阅读器打开某篇文章时的 SEO。
 * - 自有 + 模块公开 → 可收录，canonical `/brew/item/{id}`
 * - 非自有 → 仍可更新 title 方便用户，但 noindex，canonical 回退 /brew
 */
export function buildBrewItemPageSeo(opts: {
  item: BrewItem
  source: BrewSource | null | undefined
  moduleOpenToAll: boolean
}): PageSeoInput {
  const { item, source, moduleOpenToAll } = opts
  const own = isOwnBrewSource(source)
  const title = formatPageTitle(item.title || 'Brew')
  const description =
    plainTextSnippet(item.summary) ||
    plainTextSnippet(item.content) ||
    undefined

  if (own && moduleOpenToAll) {
    return {
      title,
      description,
      image: pickItemImage(item),
      path: brewOwnItemPath(item.id),
      noindex: false,
    }
  }

  // 非自有 / 模块未对游客开放：绝不拿别人的文章做站内收录
  return {
    title,
    description,
    path: '/brew',
    noindex: true,
  }
}

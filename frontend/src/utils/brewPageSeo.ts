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
    .replaceAll(/<[^>]*>/g, ' ')
    .replaceAll('&nbsp;', ' ')
    .replaceAll('&amp;', '&')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll(/\s+/g, ' ')
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

  return {
    title,
    description,
    path: '/brew',
    noindex: true,
  }
}

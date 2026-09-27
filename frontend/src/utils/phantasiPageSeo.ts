import type { PhantasiViewMode } from '../components/phantasi/logic/board'
import type { PhantasiItem, PhantasiSource } from '../types/phantasi'
import type { PageSeoInput } from './siteMetadata'
import { isOwnPhantasiSource } from '../components/phantasi/constants'
import {
  isJournalSyndicationPath,
  JOURNAL_ROOT,
  journalItemPath,
  seoJournalFollow,
  seoListNoindex,
} from '../components/phantasi/logic/journalRoutes'
import { buildModulePageSeo } from './modulePageSeo'
import { formatPageTitle } from './siteMetadata'

function plainTextSnippet(
  htmlOrText: string | null | undefined,
  maxLen = 160,
): string | undefined {
  if (!htmlOrText) return undefined
  const plain = htmlOrText
    .replaceAll(/<[^>]*>/g, ' ')
    .replaceAll('&nbsp;', ' ')
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll('&amp;', '&')
    .replaceAll(/\s+/g, ' ')
    .trim()
  if (!plain) return undefined
  if (plain.length <= maxLen) return plain
  return `${plain.slice(0, maxLen - 1).trimEnd()}…`
}

function pickItemImage(item: PhantasiItem): string | undefined {
  const raw = item.image?.trim()
  if (!raw || raw.startsWith('data:')) return undefined
  return raw
}

export function buildPhantasiListPageSeo(opts: {
  listLabel: string
  listDescription?: string
  path: string
  moduleOpenToAll: boolean
  viewMode?: PhantasiViewMode
}): PageSeoInput {
  const seo = buildModulePageSeo({
    label: opts.listLabel,
    description: opts.listDescription,
    path: opts.path,
    moduleOpenToAll: opts.moduleOpenToAll,
  })
  const hide =
    (opts.viewMode && seoListNoindex(opts.viewMode)) ||
    isJournalSyndicationPath(opts.path)
  if (hide) {
    return {
      ...seo,
      noindex: true,
      follow: seoJournalFollow(opts.path, opts.viewMode ?? 'sources'),
    }
  }
  return seo
}

export function buildPhantasiItemPageSeo(opts: {
  item: PhantasiItem
  source: PhantasiSource | null | undefined
  moduleOpenToAll: boolean
  listPath?: string
}): PageSeoInput {
  const { item, source, moduleOpenToAll } = opts
  const own = isOwnPhantasiSource(source)
  const title = formatPageTitle(item.title || 'Journal')
  const description =
    plainTextSnippet(item.summary) ||
    plainTextSnippet(item.content) ||
    undefined

  if (own && moduleOpenToAll) {
    return {
      title,
      description,
      image: pickItemImage(item),
      path: journalItemPath(item.id),
      noindex: false,
    }
  }

  return {
    title,
    path: opts.listPath ?? JOURNAL_ROOT,
    noindex: true,
    follow: true,
  }
}

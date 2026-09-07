import type { PageContent } from '../../../contexts/PageContentContext'

/** Authored summary only. Page body is a fact, not Lite's summary. */
export function pagePerceptionCopy(
  page: PageContent,
  route: string,
): { title: string; summary: string; hasBody: boolean; author: string } {
  const title = page.title?.trim() || route
  const authored = page.summary?.trim().slice(0, 400) ?? ''
  const body = page.plainText?.trim() ?? ''
  const author = page.author?.trim() ?? ''
  const autoPrefix = authored.replace(/\.\.\.$/, '').trimEnd()
  const summaryIsBodyPrefix = Boolean(autoPrefix && body.startsWith(autoPrefix))
  return {
    title,
    summary: !summaryIsBodyPrefix && authored ? authored : title,
    hasBody: Boolean(authored || body),
    author,
  }
}

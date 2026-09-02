import type { PageContent } from '../../../contexts/PageContentContext'

/** Authored summary only. Page body is a fact, not Lite's summary. */
export function pagePerceptionCopy(
  page: PageContent,
  route: string,
): { title: string; summary: string; hasBody: boolean } {
  const title = page.title?.trim() || route
  const authored = page.summary?.trim().slice(0, 400) ?? ''
  return {
    title,
    summary: authored || title,
    hasBody: Boolean(authored || page.plainText?.trim()),
  }
}

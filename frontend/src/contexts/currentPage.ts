import type { PageContent } from './PageContentContext'

let page: PageContent | null = null

export function getCurrentPageContent(): PageContent | null {
  return page
}

export function setCurrentPageContent(next: PageContent | null): void {
  page = next
}

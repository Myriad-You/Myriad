import type { PageContent } from './PageContentContext'

let page: PageContent | null = null
const listeners = new Set<() => void>()

export function getCurrentPageContent(): PageContent | null {
  return page
}

export function setCurrentPageContent(next: PageContent | null): void {
  page = next
  for (const listener of listeners) listener()
}

export function subscribeCurrentPageContent(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

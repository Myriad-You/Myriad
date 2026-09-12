import type { PageContent } from './PageContentContext'
import { authSubject } from '../utils/authSubject'

let page: PageContent | null = null
const listeners = new Set<() => void>()
let owner = authSubject.signal

authSubject.subscribe(() => {
  page = null
  owner = authSubject.signal
  for (const listener of listeners) listener()
})

export function getCurrentPageContent(): PageContent | null {
  return owner.aborted ? null : page
}

export function setCurrentPageContent(next: PageContent | null, subject = authSubject.signal): void {
  if (subject.aborted) return
  owner = subject
  page = next
  for (const listener of listeners) listener()
}

/** Capture when a publisher is created, not when its asynchronous result arrives. */
export function currentPagePublisher(subject = authSubject.signal): (next: PageContent | null) => void {
  return next => setCurrentPageContent(next, subject)
}

export function subscribeCurrentPageContent(listener: () => void): () => void {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

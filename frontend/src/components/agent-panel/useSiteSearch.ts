import type { SiteSearchResults } from '../../services/siteSearchApi'
import { useEffect, useState, useSyncExternalStore } from 'react'
import { useDebounce } from '../../hooks/useDebounce'
import { searchSite } from '../../services/siteSearchApi'
import { authSubject } from '../../utils/authSubject'

const subscribeSubject = (listener: () => void) => authSubject.subscribe(listener)
const subjectRevision = () => authSubject.revision

/** One search that came back: the word, who asked, and what they may see. */
export interface FoundSearch {
  query: string
  /** `authSubject.revision` when it was asked. Another sign-in state sees other modules. */
  subject: number
  results: SiteSearchResults
}

/** How long typing has to pause before the field is searched. */
export const SITE_SEARCH_PAUSE_MS = 200

/** Whether there is anything to show. An empty search shows nothing at all. */
export function hasSiteResults(results: SiteSearchResults | null): results is SiteSearchResults {
  return !!results && (results.posts.length > 0 || results.library.length > 0)
}

/** What the field found, and whether it was found for what the field says now. */
export interface SiteSearch {
  results: SiteSearchResults | null
  /**
   * False while the field has changed since these results came back (the
   * pause before the next search, and the search itself). They stay on screen
   * so the list does not blink on every key, but Enter must not open them.
   */
  current: boolean
}

/**
 * The site's content matching what is in the field, as it is typed. Each new
 * search cancels the one before, so a slow answer never replaces a newer one.
 * Signing in or out drops what was found and searches again as the new visitor:
 * what one sign-in state may see is not shown to the next.
 */
export function useSiteSearch(text: string, enabled: boolean): SiteSearch {
  const query = useDebounce(enabled ? text.trim() : '', SITE_SEARCH_PAUSE_MS)
  const subject = useSyncExternalStore(subscribeSubject, subjectRevision, subjectRevision)
  const [found, setFound] = useState<FoundSearch | null>(null)

  useEffect(() => {
    // What the visitor before found is dropped, not just hidden.
    setFound((prev) => (prev && prev.subject === subject ? prev : null))
    if (!query) {
      setFound(null)
      return
    }
    const controller = new AbortController()
    // An answer still on its way when the visitor signs in or out was asked as
    // the one before; it is cancelled with that state.
    const signal = AbortSignal.any([controller.signal, authSubject.signal])
    const stale = () => signal.aborted || authSubject.revision !== subject
    searchSite(query, signal)
      .then((results) => {
        if (!stale()) setFound({ query, subject, results })
      })
      .catch(() => {
        // Search is a help beside the field; failing it leaves the field as it is.
        if (!stale()) setFound(null)
      })
    return () => controller.abort()
  }, [query, subject])

  return siteSearchFor(found, enabled ? text : '', subject)
}

/** What to show for `text`, given the last search that came back. */
export function siteSearchFor(
  found: FoundSearch | null,
  text: string,
  subject: number,
): SiteSearch {
  const wanted = text.trim()
  if (!wanted || !found || found.subject !== subject) return { results: null, current: false }
  return { results: found.results, current: found.query === wanted }
}

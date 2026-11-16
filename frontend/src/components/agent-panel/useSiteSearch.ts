import type { SiteSearchResults } from '../../services/siteSearchApi'
import { useEffect, useState } from 'react'
import { useDebounce } from '../../hooks/useDebounce'
import { searchSite } from '../../services/siteSearchApi'

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
 */
export function useSiteSearch(text: string, enabled: boolean): SiteSearch {
  const query = useDebounce(enabled ? text.trim() : '', SITE_SEARCH_PAUSE_MS)
  const [found, setFound] = useState<{ query: string; results: SiteSearchResults } | null>(null)

  useEffect(() => {
    if (!query) {
      setFound(null)
      return
    }
    const controller = new AbortController()
    searchSite(query, controller.signal)
      .then((results) => {
        if (!controller.signal.aborted) setFound({ query, results })
      })
      .catch(() => {
        // Search is a help beside the field; failing it leaves the field as it is.
        if (!controller.signal.aborted) setFound(null)
      })
    return () => controller.abort()
  }, [query])

  return siteSearchFor(found, enabled ? text : '')
}

/** What to show for `text`, given the last search that came back. */
export function siteSearchFor(
  found: { query: string; results: SiteSearchResults } | null,
  text: string,
): SiteSearch {
  const wanted = text.trim()
  if (!wanted || !found) return { results: null, current: false }
  return { results: found.results, current: found.query === wanted }
}

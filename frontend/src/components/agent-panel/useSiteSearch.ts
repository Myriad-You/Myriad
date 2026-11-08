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

/**
 * The site's content matching what is in the field, as it is typed. Each new
 * search cancels the one before, so a slow answer never replaces a newer one.
 */
export function useSiteSearch(text: string, enabled: boolean): SiteSearchResults | null {
  const query = useDebounce(enabled ? text.trim() : '', SITE_SEARCH_PAUSE_MS)
  const [results, setResults] = useState<SiteSearchResults | null>(null)

  useEffect(() => {
    if (!query) {
      setResults(null)
      return
    }
    const controller = new AbortController()
    searchSite(query, controller.signal)
      .then((next) => {
        if (!controller.signal.aborted) setResults(next)
      })
      .catch(() => {
        // Search is a help beside the field; failing it leaves the field as it is.
        if (!controller.signal.aborted) setResults(null)
      })
    return () => controller.abort()
  }, [query])

  return enabled && text.trim() ? results : null
}

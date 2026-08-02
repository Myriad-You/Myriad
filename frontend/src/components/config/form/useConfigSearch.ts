import type { Locale } from '../../../i18n'
import type { ConfigSearchableItem } from '../../settings/guides/configSearch'
import type { ConfigSearchI18n } from './buildSearchableContent'
import type { Config } from './types'
import { useMemo, useState } from 'react'
import { useDebounce } from '../../../hooks/useDebounce'
import {

  rankConfigSearch,
} from '../../settings/guides/configSearch'
import {
  buildSearchableContent,

} from './buildSearchableContent'

export function useConfigSearch(
  config: Config | null,
  t: ConfigSearchI18n,
  locale: Locale,
  isAdmin = true,
) {
  const [searchQuery, setSearchQuery] = useState('')
  const debouncedSearchQuery = useDebounce(searchQuery, 300)

  const searchableContent = useMemo(
    (): ConfigSearchableItem[] =>
      buildSearchableContent(config, t, locale, { isAdmin }),
    [config, t, locale, isAdmin],
  )

  const filteredContent = useMemo(() => {
    if (!debouncedSearchQuery.trim()) {
      return [] as ReturnType<typeof rankConfigSearch>
    }
    return rankConfigSearch(searchableContent, debouncedSearchQuery, {
      maxResults: 36,
      maxGuidesPerSection: 4,
    })
  }, [debouncedSearchQuery, searchableContent])

  return {
    searchQuery,
    setSearchQuery,
    filteredContent,
  }
}

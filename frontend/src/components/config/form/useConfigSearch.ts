import type { Locale } from '../../../i18n'
import type { ConfigSearchableItem } from '../../settings/guides/configSearch'
import type { ConfigSearchI18n } from './buildSearchableContent'
import type { Config } from './types'
import { useEffect, useMemo, useState } from 'react'
import { useDebounce } from '../../../hooks/useDebounce'
import { loadSettingGuidesCatalog } from '../../settings/guides/catalog'
import { rankConfigSearch } from '../../settings/guides/configSearch'
import { buildSearchableContent } from './buildSearchableContent'

export function useConfigSearch(
  config: Config | null,
  t: ConfigSearchI18n,
  locale: Locale,
  isAdmin = true,
) {
  const [searchQuery, setSearchQuery] = useState('')
  const [catalogEpoch, setCatalogEpoch] = useState(0)
  const debouncedSearchQuery = useDebounce(searchQuery, 300)

  useEffect(() => {
    let cancelled = false
    void loadSettingGuidesCatalog(locale).then(() => {
      if (!cancelled) setCatalogEpoch((n) => n + 1)
    })
    return () => {
      cancelled = true
    }
  }, [locale])

  const searchableContent = useMemo(
    (): ConfigSearchableItem[] =>
      buildSearchableContent(config, t, locale, { isAdmin }),
    [config, t, locale, isAdmin, catalogEpoch],
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

import type { SecondaryNavItem } from '../contexts/NavigationContext'
import { useCallback } from 'react'
import { useLocation, useNavigate } from 'react-router-dom'
import { useSecondaryNav } from '../contexts/NavigationContext'

export const LIBRARY_FILTERS = [
  'all', 'game', 'video', 'music', 'anime', 'tv_series', 'book',
] as const

export type LibraryFilter = typeof LIBRARY_FILTERS[number]

function parseFilter(value: string | null): LibraryFilter {
  return LIBRARY_FILTERS.find(filter => filter === value) ?? 'all'
}

/** 分类以 URL 为准，导航岛和资料库共用同一份选中状态。 */
export function useLibraryNavigation(items: SecondaryNavItem[], expandHint: string) {
  const location = useLocation()
  const navigate = useNavigate()
  const filter = parseFilter(new URLSearchParams(location.search).get('type'))
  const onChange = useCallback((id: string) => {
    const next = new URLSearchParams(location.search)
    const type = parseFilter(id)
    if (type === 'all') next.delete('type')
    else next.set('type', type)
    const search = next.size ? `?${next}` : ''
    if (search !== location.search) {
      void navigate({ pathname: location.pathname, search, hash: location.hash })
    }
  }, [location.pathname, location.search, location.hash, navigate])

  const nav = useSecondaryNav({
    routePath: '/library',
    items,
    defaultActiveId: 'all',
    activeId: filter,
    onChange,
    expandHint,
  })
  return { ...nav, filter }
}

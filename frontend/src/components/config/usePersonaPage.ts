import { useCallback, useEffect, useRef, useState } from 'react'
import { SETTINGS_DURATION_MS } from '../settings'
import { refreshConfigTourSurface } from '../tour/tourLogic'
import { saveConfigNavPersisted } from './form/configNavPersistence'

export type AiSubpage = 'merope' | 'merope-setup'

function readSubpage(): AiSubpage | null {
  if (typeof window === 'undefined') return null
  const page = new URLSearchParams(window.location.search).get('page')
  return page === 'merope' || page === 'merope-setup' ? page : null
}

function writeSubpage(page: AiSubpage | null) {
  if (typeof window === 'undefined') return
  const url = new URL(window.location.href)
  if (page) url.searchParams.set('page', page)
  else url.searchParams.delete('page')
  window.history.pushState(window.history.state, '', url.toString())
}

export function useAiSubpage(
  onNavigate?: (page: AiSubpage | null) => void,
) {
  const [page, setPage] = useState<AiSubpage | null>(readSubpage)
  const [navDir, setNavDir] = useState<'none' | 'forward' | 'back'>('none')
  const pageRef = useRef(page)
  const onNavigateRef = useRef(onNavigate)
  pageRef.current = page
  onNavigateRef.current = onNavigate

  const reveal = useCallback((next: AiSubpage | null) => {
    const prev = pageRef.current
    window.scrollTo({ top: 0, behavior: 'auto' })
    saveConfigNavPersisted({ scrollY: 0 })
    setNavDir(next && !prev ? 'forward' : !next && prev ? 'back' : 'forward')
    setPage(next)
    onNavigateRef.current?.(next)
    refreshConfigTourSurface()
  }, [])

  useEffect(() => {
    const apply = () => {
      const next = readSubpage()
      if (next === pageRef.current) return
      reveal(next)
    }
    window.addEventListener('popstate', apply)
    return () => window.removeEventListener('popstate', apply)
  }, [reveal])

  useEffect(() => {
    if (navDir === 'none') return undefined
    const id = window.setTimeout(
      setNavDir,
      SETTINGS_DURATION_MS.slow + 60,
      'none',
    )
    return () => window.clearTimeout(id)
  }, [navDir, page])

  const openPage = useCallback(
    (next: AiSubpage) => {
      writeSubpage(next)
      reveal(next)
    },
    [reveal],
  )

  const closePage = useCallback(() => {
    writeSubpage(null)
    reveal(null)
  }, [reveal])

  return {
    page,
    navDir: navDir === 'none' ? undefined : navDir,
    openPage,
    closePage,
  }
}

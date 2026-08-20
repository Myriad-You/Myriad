import { useCallback, useEffect, useRef, useState } from 'react'
import { SETTINGS_DURATION_MS } from '../settings'
import { saveConfigNavPersisted } from './form/configNavPersistence'

const PERSONA_PAGE = 'persona'

function isPersonaPageOpen(): boolean {
  if (typeof window === 'undefined') return false
  return new URLSearchParams(window.location.search).get('page') === PERSONA_PAGE
}

function setPersonaPageOpen(open: boolean) {
  if (typeof window === 'undefined') return
  const url = new URL(window.location.href)
  if (open) url.searchParams.set('page', PERSONA_PAGE)
  else url.searchParams.delete('page')
  window.history.pushState(window.history.state, '', url.toString())
}

/**
 * AI 一级 ↔ 设定引导：URL `page=persona`、回顶、sm-pane 方向。
 * 回顶与换设置分类同一套（scrollTo(0) + 清掉滚动快照）。
 */
export function usePersonaPage(onNavigate?: (open: boolean) => void) {
  const [open, setOpen] = useState(isPersonaPageOpen)
  const [navDir, setNavDir] = useState<'none' | 'forward' | 'back'>('none')
  const openRef = useRef(open)
  const onNavigateRef = useRef(onNavigate)
  openRef.current = open
  onNavigateRef.current = onNavigate

  const reveal = useCallback((nextOpen: boolean) => {
    window.scrollTo({ top: 0, behavior: 'auto' })
    saveConfigNavPersisted({ scrollY: 0 })
    setNavDir(nextOpen ? 'forward' : 'back')
    setOpen(nextOpen)
    onNavigateRef.current?.(nextOpen)
  }, [])

  useEffect(() => {
    const apply = () => {
      const nextOpen = isPersonaPageOpen()
      if (nextOpen === openRef.current) return
      reveal(nextOpen)
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
  }, [navDir, open])

  const openPage = useCallback(() => {
    setPersonaPageOpen(true)
    reveal(true)
  }, [reveal])

  const closePage = useCallback(() => {
    setPersonaPageOpen(false)
    reveal(false)
  }, [reveal])

  return {
    open,
    navDir: navDir === 'none' ? undefined : navDir,
    openPage,
    closePage,
  }
}

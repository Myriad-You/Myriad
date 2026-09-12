import type { LayoutKey } from '../types'
import { useCallback, useEffect, useRef, useState } from 'react'

export interface UseReaderChromeOptions {
  articleRef: React.RefObject<HTMLElement | null>
  layout: LayoutKey
  closeAllTooltips: () => void
}

export interface UseReaderChromeReturn {
  showPanels: boolean
  setShowPanels: (show: boolean) => void
  showMobileControls: boolean
  setShowMobileControls: (show: boolean) => void
  isHoveringControlsRef: React.RefObject<boolean>
  resetHideTimer: (delay?: number) => void
}

export function useReaderChrome({
  articleRef,
  layout,
  closeAllTooltips,
}: UseReaderChromeOptions): UseReaderChromeReturn {
  const [showPanels, setShowPanels] = useState(true)
  const [showMobileControls, setShowMobileControls] = useState(false)
  const hideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const lastMouseMoveRef = useRef<number>(0)
  const isScrollingRef = useRef(false)
  const cooldownRef = useRef(false) // 冷却期内不立刻再显示。
  const lastScrollTopRef = useRef(0)
  const isHoveringControlsRef = useRef(false)

  const resetHideTimer = useCallback(
    (delay = 4000) => {
      if (hideTimerRef.current) {
        clearTimeout(hideTimerRef.current)
      }
      hideTimerRef.current = setTimeout(() => {
        if (isHoveringControlsRef.current) {
          resetHideTimer(delay)
          return
        }
        setShowPanels(false)
        closeAllTooltips()
        cooldownRef.current = true
        setTimeout(() => {
          cooldownRef.current = false
        }, 800)
      }, delay)
    },
    [closeAllTooltips],
  )

  const showPanelsIfAllowed = useCallback(() => {
    if (cooldownRef.current || isScrollingRef.current) return
    setShowPanels(true)
    resetHideTimer()
  }, [resetHideTimer])

  useEffect(() => {
    const article = articleRef.current
    if (!article) return

    let scrollEndTimer: ReturnType<typeof setTimeout> | null = null

    const handleScroll = () => {
      const currentScrollTop = article.scrollTop
      const isScrollingUp = currentScrollTop < lastScrollTopRef.current
      lastScrollTopRef.current = currentScrollTop

      if (scrollEndTimer) clearTimeout(scrollEndTimer)

      if (isScrollingUp && currentScrollTop > 10) {
        isScrollingRef.current = false
        setShowPanels(true)
        resetHideTimer(2000)
      } else if (!isScrollingUp) {
        if (!isScrollingRef.current && !isHoveringControlsRef.current) {
          isScrollingRef.current = true
          setShowPanels(false)
          closeAllTooltips()
        }
      }

      scrollEndTimer = setTimeout(() => {
        isScrollingRef.current = false
      }, 150)
    }

    article.addEventListener('scroll', handleScroll, { passive: true })
    return () => {
      article.removeEventListener('scroll', handleScroll)
      if (scrollEndTimer) clearTimeout(scrollEndTimer)
    }
  }, [resetHideTimer, closeAllTooltips])

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      const now = Date.now()
      const windowWidth = window.innerWidth

      const layoutMaxWidth = layout === 'wide' ? 896 : 768
      const contentWidth = Math.min(layoutMaxWidth, windowWidth - 48)
      const contentLeft = (windowWidth - contentWidth) / 2
      const contentRight = contentLeft + contentWidth

      const controlZoneWidth = 80
      const isInLeftControlZone =
        e.clientX >= contentLeft - controlZoneWidth && e.clientX <= contentLeft
      const isInRightControlZone =
        e.clientX >= contentRight &&
        e.clientX <= contentRight + controlZoneWidth
      const isInControlZone = isInLeftControlZone || isInRightControlZone

      isHoveringControlsRef.current = isInControlZone

      if (isInControlZone) {
        if (cooldownRef.current) return
        isScrollingRef.current = false
        setShowPanels(true)
        if (hideTimerRef.current) {
          clearTimeout(hideTimerRef.current)
          hideTimerRef.current = null
        }
      } else {
        if (now - lastMouseMoveRef.current < 500) return
        lastMouseMoveRef.current = now
        showPanelsIfAllowed()
      }
    }

    const handleMouseLeave = () => {
      isHoveringControlsRef.current = false
      if (showPanels) {
        resetHideTimer(2000)
      }
    }

    window.addEventListener('mousemove', handleMouseMove, { passive: true })
    document.addEventListener('mouseleave', handleMouseLeave)
    return () => {
      window.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseleave', handleMouseLeave)
      if (hideTimerRef.current) clearTimeout(hideTimerRef.current)
    }
  }, [showPanelsIfAllowed, resetHideTimer, layout, showPanels])

  useEffect(() => {
    resetHideTimer()
    return () => {
      if (hideTimerRef.current) clearTimeout(hideTimerRef.current)
    }
  }, [resetHideTimer])

  return {
    showPanels,
    setShowPanels,
    showMobileControls,
    setShowMobileControls,
    isHoveringControlsRef,
    resetHideTimer,
  }
}

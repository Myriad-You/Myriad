/**
 * 阅读器沉浸控制栏自动显示 / 隐藏
 */

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
  const cooldownRef = useRef(false) // 冷却期，防止刚隐藏就显示
  const lastScrollTopRef = useRef(0) // 上次滚动位置，用于判断滚动方向
  const isHoveringControlsRef = useRef(false) // 鼠标是否在控制栏区域

  // 自动隐藏控制栏
  const resetHideTimer = useCallback(
    (delay = 4000) => {
      if (hideTimerRef.current) {
        clearTimeout(hideTimerRef.current)
      }
      hideTimerRef.current = setTimeout(() => {
        // 如果鼠标在控制栏区域，不隐藏
        if (isHoveringControlsRef.current) {
          resetHideTimer(delay)
          return
        }
        setShowPanels(false)
        // 关闭所有附属的 tooltip
        closeAllTooltips()
        // 进入冷却期
        cooldownRef.current = true
        setTimeout(() => {
          cooldownRef.current = false
        }, 800)
      }, delay)
    },
    [closeAllTooltips],
  )

  // 显示控制栏
  const showPanelsIfAllowed = useCallback(() => {
    if (cooldownRef.current || isScrollingRef.current) return
    setShowPanels(true)
    resetHideTimer()
  }, [resetHideTimer])

  // 滚动时：向上滚动显示控制栏2秒，向下滚动隐藏
  useEffect(() => {
    const article = articleRef.current
    if (!article) return

    let scrollEndTimer: ReturnType<typeof setTimeout> | null = null

    const handleScroll = () => {
      const currentScrollTop = article.scrollTop
      const isScrollingUp = currentScrollTop < lastScrollTopRef.current
      lastScrollTopRef.current = currentScrollTop

      // 清除之前的定时器
      if (scrollEndTimer) clearTimeout(scrollEndTimer)

      // 向上滚动（往之前内容滑动）时显示控制栏
      if (isScrollingUp && currentScrollTop > 10) {
        isScrollingRef.current = false
        setShowPanels(true)
        resetHideTimer(2000) // 显示2秒后自动隐藏
      } else if (!isScrollingUp) {
        // 向下滚动时隐藏（如果鼠标不在控制栏区域）
        if (!isScrollingRef.current && !isHoveringControlsRef.current) {
          isScrollingRef.current = true
          setShowPanels(false)
          // 关闭所有附属的 tooltip
          closeAllTooltips()
        }
      }

      // 停止滚动后的处理
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

  // 鼠标移动时显示（节流处理 + 控制栏区域检测）
  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      const now = Date.now()
      const windowWidth = window.innerWidth

      // 根据当前布局计算内容区宽度
      // narrow: max-w-3xl = 768px, wide: max-w-4xl = 896px
      const layoutMaxWidth = layout === 'wide' ? 896 : 768
      const contentWidth = Math.min(layoutMaxWidth, windowWidth - 48) // 减去 px-6 左右内边距
      const contentLeft = (windowWidth - contentWidth) / 2
      const contentRight = contentLeft + contentWidth

      // 控制栏区域：内容区两侧各 80px 范围内（控制栏宽度约 60px + margin）
      const controlZoneWidth = 80
      const isInLeftControlZone =
        e.clientX >= contentLeft - controlZoneWidth && e.clientX <= contentLeft
      const isInRightControlZone =
        e.clientX >= contentRight &&
        e.clientX <= contentRight + controlZoneWidth
      const isInControlZone = isInLeftControlZone || isInRightControlZone

      // 更新悬停状态
      isHoveringControlsRef.current = isInControlZone

      // 控制栏区域立即响应，其他区域节流
      if (isInControlZone) {
        // 控制栏区域：直接显示，不节流，不自动隐藏
        if (cooldownRef.current) return
        isScrollingRef.current = false // 允许覆盖滚动隐藏
        setShowPanels(true)
        // 清除隐藏定时器，鼠标在控制栏区域时不隐藏
        if (hideTimerRef.current) {
          clearTimeout(hideTimerRef.current)
          hideTimerRef.current = null
        }
      } else {
        // 非控制栏区域：节流 500ms
        if (now - lastMouseMoveRef.current < 500) return
        lastMouseMoveRef.current = now
        showPanelsIfAllowed()
      }
    }

    // 鼠标离开控制栏区域时重新设置隐藏定时器
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

  // 初始化隐藏计时器
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

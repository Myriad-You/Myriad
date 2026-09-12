import type { LockedDragMetrics } from '../utils/customScrollbarMetrics'

import { useCallback, useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router-dom'
import { observeResize } from '../hooks/animation'
import {
  useSharedResize,
  useSharedScroll,
} from '../hooks/useSharedEventListener'
import {
  computeThumbLayout,
  grabOffsetFromThumbPointer,
  MIN_THUMB_HEIGHT,
  scrollTopFromThumb,
  thumbTopFromPointer,
  TRACK_HEIGHT_PERCENT,
} from '../utils/customScrollbarMetrics'

function getIsMobile(): boolean {
  if (typeof window === 'undefined') return true

  const isSmallScreen = window.matchMedia('(max-width: 767px)').matches
  const isTouchDevice = window.matchMedia(
    '(hover: none) and (pointer: coarse)',
  ).matches

  return isSmallScreen || isTouchDevice
}

function isLibraryCanvasActive(): boolean {
  if (typeof document === 'undefined') return false
  return document.documentElement.dataset.libraryCanvas === 'active'
}

function applyPageScroll(top: number) {
  document.documentElement.scrollTop = top
  document.body.scrollTop = top
}

function setScrollbarDragging(active: boolean) {
  const root = document.documentElement
  if (active) root.dataset.scrollbarDragging = 'true'
  else delete root.dataset.scrollbarDragging
}

const IS_MOBILE_DEVICE = typeof window !== 'undefined' ? getIsMobile() : true

export default function CustomScrollbar() {
  if (IS_MOBILE_DEVICE) {
    return null
  }

  const [isMobile, setIsMobile] = useState(getIsMobile)

  useEffect(() => {
    if (typeof window === 'undefined') return

    const mediaQuery = window.matchMedia('(max-width: 767px)')
    const handleChange = (e: MediaQueryListEvent) => {
      setIsMobile(e.matches)
    }

    mediaQuery.addEventListener('change', handleChange)

    return () => {
      mediaQuery.removeEventListener('change', handleChange)
    }
  }, [])

  if (isMobile) {
    return null
  }

  return <CustomScrollbarInner />
}

function CustomScrollbarInner() {
  const location = useLocation()
  const [isDragging, setIsDragging] = useState(false)
  const [isVisible, setIsVisible] = useState(false)
  const [isScrolling, setIsScrolling] = useState(false)
  const [isHovering, setIsHovering] = useState(false)
  const [isLibraryCanvas, setIsLibraryCanvas] = useState(isLibraryCanvasActive)

  const hideTimerRef = useRef<number | null>(null)
  const scrollingTimerRef = useRef<number | null>(null)
  const trackRef = useRef<HTMLDivElement>(null)
  const thumbRef = useRef<HTMLDivElement>(null)
  const pulseRef = useRef<HTMLDivElement>(null)
  const isDraggingRef = useRef(false)
  const dragMetricsRef = useRef<LockedDragMetrics | null>(null)
  const savedScrollBehaviorRef = useRef({ html: '', body: '' })
  const lastScrollTopRef = useRef(0)
  const isRouteTransitioningRef = useRef(false) // 路由切换中禁止更新。
  const routeTransitionTimeRef = useRef(0)
  const cachedDocumentHeightRef = useRef(0)
  const lastHeightCheckRef = useRef(0)

  const paintThumb = useCallback((thumbTop: number, thumbHeight: number) => {
    const thumb = thumbRef.current
    if (!thumb) return
    thumb.style.height = `${thumbHeight}px`
    thumb.style.top = `${thumbTop}px`
    const pulse = pulseRef.current
    if (pulse) {
      pulse.style.top = `${thumbTop + thumbHeight / 2}px`
    }
  }, [])

  const readDocumentHeight = useCallback((force = false) => {
    const now = Date.now()
    if (
      force ||
      cachedDocumentHeightRef.current === 0 ||
      now - lastHeightCheckRef.current > 500
    ) {
      cachedDocumentHeightRef.current = document.documentElement.scrollHeight
      lastHeightCheckRef.current = now
    }
    return cachedDocumentHeightRef.current
  }, [])

  const updateThumb = useCallback(() => {
    if (!thumbRef.current || isDraggingRef.current) return

    // 路由切换中禁止更新。
    if (isRouteTransitioningRef.current) return

    const layout = computeThumbLayout({
      windowHeight: window.innerHeight,
      documentHeight: readDocumentHeight(),
      scrollTop: window.scrollY,
    })

    if (layout.scrollableHeight <= 0) {
      setIsVisible(false)
      return
    }

    const timeSinceRouteTransition = Date.now() - routeTransitionTimeRef.current
    if (timeSinceRouteTransition >= 1000 && timeSinceRouteTransition < 1700) {
      // 路由过渡已在外部设置，这里不要改 transition。
    } else {
      thumbRef.current.style.transition = 'none'
    }

    paintThumb(layout.thumbTop, layout.thumbHeight)
    lastScrollTopRef.current = window.scrollY
  }, [paintThumb, readDocumentHeight])

  const handleUpdate = useCallback(() => {
    if (isDraggingRef.current) return
    updateThumb()
  }, [updateThumb])

  useSharedScroll(handleUpdate)
  useSharedResize(handleUpdate)

  useEffect(() => {
    const syncCanvas = () => {
      const active = isLibraryCanvasActive()
      setIsLibraryCanvas(active)
      if (active && isDraggingRef.current) {
        isDraggingRef.current = false
        dragMetricsRef.current = null
        document.documentElement.style.scrollBehavior =
          savedScrollBehaviorRef.current.html
        document.body.style.scrollBehavior = savedScrollBehaviorRef.current.body
        setScrollbarDragging(false)
        setIsDragging(false)
      }
    }
    window.addEventListener('libraryCanvasModeChanged', syncCanvas)
    syncCanvas()
    return () => {
      window.removeEventListener('libraryCanvasModeChanged', syncCanvas)
    }
  }, [])

  useEffect(() => {
    let throttleTimer: number | null = null
    let initialRaf: number | null = null
    let initialFollowUpTimer: number | null = null

    const handleUpdateThrottled = () => {
      if (throttleTimer !== null) return
      throttleTimer = window.setTimeout(() => {
        handleUpdate()
        throttleTimer = null
      }, 100)
    }

    initialRaf = requestAnimationFrame(() => {
      updateThumb()
      initialFollowUpTimer = window.setTimeout(updateThumb, 100)
    })

    const unobserve = observeResize(
      document.documentElement,
      handleUpdateThrottled,
    )

    return () => {
      unobserve()
      if (throttleTimer !== null) clearTimeout(throttleTimer)
      if (initialRaf !== null) cancelAnimationFrame(initialRaf)
      if (initialFollowUpTimer !== null) clearTimeout(initialFollowUpTimer)
    }
  }, [updateThumb, handleUpdate])

  useEffect(() => {
    if (!thumbRef.current) return

    let updateDelay: ReturnType<typeof setTimeout> | null = null
    let transitionTimer: ReturnType<typeof setTimeout> | null = null
    let thumbRaf = 0

    routeTransitionTimeRef.current = Date.now()

    isRouteTransitioningRef.current = true

    const initialDelay = setTimeout(() => {
      if (!thumbRef.current) return

      thumbRef.current.style.transition =
        'top 0.5s cubic-bezier(0.4, 0, 0.2, 1), height 0.5s cubic-bezier(0.4, 0, 0.2, 1)'

      updateDelay = setTimeout(() => {
        isRouteTransitioningRef.current = false
        cachedDocumentHeightRef.current = 0
        thumbRaf = requestAnimationFrame(() => {
          thumbRaf = 0
          updateThumb()
        })
      }, 150)

      transitionTimer = setTimeout(() => {
        if (thumbRef.current) {
          thumbRef.current.style.transition = 'none'
        }
      }, 700)
    }, 1000)

    return () => {
      clearTimeout(initialDelay)
      if (updateDelay !== null) clearTimeout(updateDelay)
      if (transitionTimer !== null) clearTimeout(transitionTimer)
      if (thumbRaf) cancelAnimationFrame(thumbRaf)
      isRouteTransitioningRef.current = false
    }
  }, [location.pathname, updateThumb])

  useEffect(() => {
    const handleScroll = () => {
      setIsVisible(true)
      setIsScrolling(true)

      if (hideTimerRef.current) clearTimeout(hideTimerRef.current)
      if (scrollingTimerRef.current) clearTimeout(scrollingTimerRef.current)

      scrollingTimerRef.current = window.setTimeout(() => {
        setIsScrolling(false)
      }, 100)

      // hover/拖拽时不隐藏。
      hideTimerRef.current = window.setTimeout(() => {
        if (!isDraggingRef.current && !isHovering) {
          setIsVisible(false)
        }
      }, 1500)
    }

    window.addEventListener('scroll', handleScroll, { passive: true })

    const initialCheck = () => {
      const scrollableHeight =
        document.documentElement.scrollHeight - window.innerHeight
      if (scrollableHeight > 0) {
        setIsVisible(true)
        hideTimerRef.current = window.setTimeout(() => {
          if (!isDraggingRef.current && !isHovering) {
            setIsVisible(false)
          }
        }, 2000)
      }
    }

    initialCheck()

    return () => {
      window.removeEventListener('scroll', handleScroll)
      if (hideTimerRef.current) clearTimeout(hideTimerRef.current)
      if (scrollingTimerRef.current) clearTimeout(scrollingTimerRef.current)
    }
  }, [isHovering])

  const endDrag = useCallback(
    (event?: React.PointerEvent | PointerEvent) => {
      const drag = dragMetricsRef.current
      if (!drag) return
      if (event && event.pointerId !== drag.pointerId) return

      const track = trackRef.current
      if (track && event && track.hasPointerCapture(event.pointerId)) {
        track.releasePointerCapture(event.pointerId)
      }

      dragMetricsRef.current = null
      isDraggingRef.current = false
      document.documentElement.style.scrollBehavior =
        savedScrollBehaviorRef.current.html
      document.body.style.scrollBehavior = savedScrollBehaviorRef.current.body
      setScrollbarDragging(false)
      document.body.style.userSelect = ''
      document.body.style.cursor = ''
      setIsDragging(false)

      // 不要从 window.scrollY 重算 thumb；smooth scroll 会滞后并闪回。
      cachedDocumentHeightRef.current = 0
    },
    [],
  )

  const moveDrag = useCallback(
    (event: React.PointerEvent | PointerEvent) => {
      const drag = dragMetricsRef.current
      if (!drag || event.pointerId !== drag.pointerId) return
      event.preventDefault()

      const thumbTop = thumbTopFromPointer(event.clientY, drag)
      const scrollableHeight = Math.max(
        0,
        document.documentElement.scrollHeight - window.innerHeight,
      )
      const nextScrollTop = scrollTopFromThumb(
        thumbTop,
        drag.availableTrackHeight,
        scrollableHeight,
      )
      applyPageScroll(nextScrollTop)
      lastScrollTopRef.current = nextScrollTop
      paintThumb(thumbTop, drag.thumbHeight)
    },
    [paintThumb],
  )

  const beginDrag = useCallback(
    (event: React.PointerEvent<HTMLDivElement>, mode: 'thumb' | 'track') => {
      if (event.button !== 0) return
      const thumb = thumbRef.current
      const track = trackRef.current
      if (!thumb || !track) return

      event.preventDefault()
      event.stopPropagation()

      const layout = computeThumbLayout({
        windowHeight: window.innerHeight,
        documentHeight: document.documentElement.scrollHeight,
        scrollTop: window.scrollY,
      })
      if (layout.scrollableHeight <= 0) return

      savedScrollBehaviorRef.current = {
        html: document.documentElement.style.scrollBehavior,
        body: document.body.style.scrollBehavior,
      }
      document.documentElement.style.scrollBehavior = 'auto'
      document.body.style.scrollBehavior = 'auto'
      setScrollbarDragging(true)

      const trackRect = track.getBoundingClientRect()
      const thumbRect = thumb.getBoundingClientRect()
      const availableTrackHeight = Math.max(1, trackRect.height - thumbRect.height)

      let grabOffsetY = grabOffsetFromThumbPointer(
        event.clientY,
        thumbRect.top,
        thumbRect.height,
      )

      if (mode === 'track') {
        grabOffsetY = thumbRect.height / 2
        const thumbTop = thumbTopFromPointer(event.clientY, {
          trackTop: trackRect.top,
          grabOffsetY,
          availableTrackHeight,
        })
        const nextScrollTop = scrollTopFromThumb(
          thumbTop,
          availableTrackHeight,
          layout.scrollableHeight,
        )
        applyPageScroll(nextScrollTop)
        lastScrollTopRef.current = nextScrollTop
        paintThumb(thumbTop, thumbRect.height)
      }

      dragMetricsRef.current = {
        pointerId: event.pointerId,
        grabOffsetY,
        trackTop: trackRect.top,
        availableTrackHeight,
        thumbHeight: thumbRect.height,
        scrollableHeight: layout.scrollableHeight,
      }
      isDraggingRef.current = true
      cachedDocumentHeightRef.current =
        layout.scrollableHeight + window.innerHeight
      lastHeightCheckRef.current = Date.now()
      setIsDragging(true)
      setIsVisible(true)
      document.body.style.userSelect = 'none'
      document.body.style.cursor = 'grabbing'
      thumb.style.transition = 'none'

      if (!track.hasPointerCapture(event.pointerId)) {
        track.setPointerCapture(event.pointerId)
      }
    },
    [paintThumb],
  )

  const handleTrackPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    const thumb = thumbRef.current
    const onThumb = thumb != null && thumb.contains(event.target as Node)
    beginDrag(event, onThumb ? 'thumb' : 'track')
  }

  if (typeof window === 'undefined' || typeof document === 'undefined') {
    return null
  }

  if (isLibraryCanvas) return null

  const windowHeight = window.innerHeight
  const TRACK_HEIGHT = windowHeight * TRACK_HEIGHT_PERCENT

  const scrollableHeight =
    document.documentElement.scrollHeight - window.innerHeight
  if (scrollableHeight <= 0) return null

  return (
    <>
      <div
        ref={trackRef}
        className="fixed right-2 z-9999 hidden w-6 md:block"
        onMouseEnter={() => setIsHovering(true)}
        onMouseLeave={() => {
          if (!isDraggingRef.current) setIsHovering(false)
        }}
        onPointerDown={handleTrackPointerDown}
        onPointerMove={moveDrag}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onLostPointerCapture={endDrag}
        style={{
          height: `${TRACK_HEIGHT}px`,
          top: '50%',
          touchAction: 'none',
          transform: isVisible
            ? 'translateY(-50%) translateX(0px) scale(1)'
            : 'translateY(-50%) translateX(60px) scale(0.8)',
          opacity: isVisible ? 1 : 0,
          transition: isVisible
            ? 'opacity 0.4s cubic-bezier(0.34, 1.56, 0.64, 1), transform 0.5s cubic-bezier(0.34, 1.56, 0.64, 1)'
            : 'opacity 0.3s cubic-bezier(0.4, 0, 0.2, 1), transform 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
          pointerEvents: isVisible || isDragging ? 'auto' : 'none',
        }}
      >
        <div
          className="absolute inset-y-0 left-1/2 w-2.5 -translate-x-1/2 rounded-full"
          style={{
            backgroundColor: `color-mix(in srgb, var(--color-primary) ${
              isDragging
                ? '20%'
                : isScrolling
                  ? '15%'
                  : isHovering
                    ? '12%'
                    : '8%'
            }, transparent)`,
            boxShadow: isDragging
              ? `inset 0 0 24px color-mix(in srgb, var(--color-primary) 15%, transparent)`
              : isScrolling
                ? `inset 0 0 14px color-mix(in srgb, var(--color-primary) 8%, transparent)`
                : 'none',
            transform: `scaleX(${isDragging ? 1.15 : isScrolling ? 1.08 : isHovering ? 1.05 : 1})`,
            transition: 'all 0.22s cubic-bezier(0.34, 1.56, 0.64, 1)',
          }}
        />

        <div
          ref={thumbRef}
          className="absolute left-1/2 w-2.5 -translate-x-1/2 rounded-full cursor-grab active:cursor-grabbing"
          role="scrollbar"
          aria-orientation="vertical"
          style={{
            top: '0px',
            height: `${MIN_THUMB_HEIGHT}px`,
            touchAction: 'none',
            backgroundColor:
              'color-mix(in srgb, var(--color-primary) 70%, transparent)',
            opacity: 0.95,
            boxShadow: `0 0 14px color-mix(in srgb, var(--color-primary) 65%, transparent),
                       0 3px 10px color-mix(in srgb, var(--color-primary) 45%, transparent)`,
            transition: 'none',
            willChange: isDragging ? 'top' : 'auto',
          }}
        />

        {isDragging && (
          <div
            ref={pulseRef}
            className="pointer-events-none absolute left-1/2 -translate-x-1/2 -translate-y-1/2"
            style={{
              top: `${(Number.parseFloat(thumbRef.current?.style.top || '0') || 0) + (Number.parseFloat(thumbRef.current?.style.height || '0') || 0) / 2}px`,
              width: '16px',
              height: '16px',
              borderRadius: '50%',
              backgroundColor: `color-mix(in srgb, var(--color-primary) 30%, transparent)`,
              animation:
                'pulse-ring 1.5s cubic-bezier(0.4, 0, 0.6, 1) infinite',
            }}
          />
        )}
      </div>

      <style>
        {`
        html[data-scrollbar-dragging='true'] {
          scroll-behavior: auto !important;
          overflow-anchor: none;
        }
        @keyframes pulse-ring {
          0%, 100% {
            transform: translate(-50%, -50%) scale(1);
            opacity: 0.5;
          }
          50% {
            transform: translate(-50%, -50%) scale(1.8);
            opacity: 0;
          }
        }
      `}
      </style>
    </>
  )
}

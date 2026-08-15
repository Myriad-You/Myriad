/**
 * 自定义滚动条组件 - 彻底优化版
 * 替代浏览器原生滚动条，避免布局偏移
 * 特点：
 * - 固定30%高度轨道，居中显示
 * - 使用壁纸色（--color-primary）
 * - 流畅的进入/退出/滚动动画
 * - 指针捕获 + 锁定几何，拖拽时滑块贴住指针
 * - 仅在桌面端显示，移动端完全不加载
 * - 右侧 1rem 定位
 * - SPA 路由切换时平滑过渡尺寸
 * - 资料库画布模式隐藏（视口由画布自己平移）
 */

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
  type LockedDragMetrics,
} from '../utils/customScrollbarMetrics'

// 检测是否为移动端 - 使用多重检测确保准确性
function getIsMobile(): boolean {
  if (typeof window === 'undefined') return true // SSR 时视为移动端，不渲染

  // 多重检测: 屏幕宽度 + 触摸设备
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
  // html is the real scroller (`overflow: hidden auto`). Write both
  // anyway — scrollingElement is body in some engines.
  document.documentElement.scrollTop = top
  document.body.scrollTop = top
}

function setScrollbarDragging(active: boolean) {
  const root = document.documentElement
  if (active) root.dataset.scrollbarDragging = 'true'
  else delete root.dataset.scrollbarDragging
}

// 在模块加载时立即检测,避免任何延迟
const IS_MOBILE_DEVICE = typeof window !== 'undefined' ? getIsMobile() : true

/**
 * 主导出组件 - 移动端守卫
 * ⚠️ 关键优化: 在移动端完全不渲染,避免注册任何 Observer 和事件监听器
 * 这是防止移动端崩溃的第一道防线
 */
export default function CustomScrollbar() {
  // 关键: 使用模块级常量,避免首次渲染延迟
  // 如果是移动端,直接返回 null,不执行任何逻辑
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

  // 二次检查: 响应式变化时也要检测
  if (isMobile) {
    return null
  }

  return <CustomScrollbarInner />
}

/**
 * 内部滚动条组件 - 只在桌面端渲染
 */
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
  const isRouteTransitioningRef = useRef(false) // 路由切换中，禁止所有更新
  const routeTransitionTimeRef = useRef(0) // 记录路由切换开始时间
  const cachedDocumentHeightRef = useRef(0) // 缓存文档高度，减少重排
  const lastHeightCheckRef = useRef(0) // 上次检查高度的时间

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

  // 计算并更新 Thumb 的位置和高度
  const updateThumb = useCallback(() => {
    if (!thumbRef.current || isDraggingRef.current) return

    // 路由切换期间，禁止所有更新
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
      // 路由切换的过渡动画已经在外部设置，这里直接更新即可
    } else {
      thumbRef.current.style.transition = 'none'
    }

    paintThumb(layout.thumbTop, layout.thumbHeight)
    lastScrollTopRef.current = window.scrollY
  }, [paintThumb, readDocumentHeight])

  // RAF 节流的更新处理器 - 使用 useCallback 确保引用稳定
  const handleUpdate = useCallback(() => {
    if (isDraggingRef.current) return
    updateThumb()
  }, [updateThumb])

  // 使用共享的 scroll 和 resize 监听器
  // 关键优化: 使用共享事件监听器，减少重复注册
  // RAF 节流 (~60fps) 由共享监听器内部处理
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

  // ResizeObserver 用于监听文档高度变化
  // 移除了 MutationObserver - 这是造成移动端崩溃的主要原因
  useEffect(() => {
    let throttleTimer: number | null = null
    let initialRaf: number | null = null
    let initialFollowUpTimer: number | null = null

    const handleUpdateThrottled = () => {
      if (throttleTimer !== null) return
      throttleTimer = window.setTimeout(() => {
        handleUpdate()
        throttleTimer = null
      }, 100) // 100ms 节流
    }

    // 立即执行初始更新，确保 thumb 可见
    initialRaf = requestAnimationFrame(() => {
      updateThumb()
      // 再次更新以确保准确（处理初次渲染后的 DOM 变化）
      initialFollowUpTimer = window.setTimeout(updateThumb, 100)
    })

    // 使用共享 ResizeObserver 监听文档高度变化
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

  // 监听路由变化，触发平滑过渡
  useEffect(() => {
    if (!thumbRef.current) return

    let updateDelay: ReturnType<typeof setTimeout> | null = null
    let transitionTimer: ReturnType<typeof setTimeout> | null = null

    // 记录路由切换时间
    routeTransitionTimeRef.current = Date.now()

    // 立即设置标志，禁止所有更新
    isRouteTransitioningRef.current = true

    // 等待 DOM 重排和页面内容加载完成
    const initialDelay = setTimeout(() => {
      if (!thumbRef.current) return

      // 先启用过渡动画
      thumbRef.current.style.transition =
        'top 0.5s cubic-bezier(0.4, 0, 0.2, 1), height 0.5s cubic-bezier(0.4, 0, 0.2, 1)'

      // 再等待一段时间后触发更新到新页面的尺寸
      updateDelay = setTimeout(() => {
        // 解除更新禁止，并触发一次更新
        isRouteTransitioningRef.current = false
        // 路由变化时重置高度缓存
        cachedDocumentHeightRef.current = 0
        requestAnimationFrame(() => {
          updateThumb()
        })
      }, 150) // 再延迟 150ms 确保过渡已经设置好

      // 700ms 后移除过渡，恢复正常
      transitionTimer = setTimeout(() => {
        if (thumbRef.current) {
          thumbRef.current.style.transition = 'none'
        }
      }, 700)
    }, 1000) // 等待 1000ms 让 DOM 完全重排和页面动画完成

    return () => {
      clearTimeout(initialDelay)
      if (updateDelay !== null) clearTimeout(updateDelay)
      if (transitionTimer !== null) clearTimeout(transitionTimer)
      // 清理时也要恢复标志
      isRouteTransitioningRef.current = false
    }
  }, [location.pathname, updateThumb])

  // 显示/隐藏滚动条 + 滚动状态检测
  useEffect(() => {
    const handleScroll = () => {
      setIsVisible(true)
      setIsScrolling(true)

      // 清除之前的计时器
      if (hideTimerRef.current) clearTimeout(hideTimerRef.current)
      if (scrollingTimerRef.current) clearTimeout(scrollingTimerRef.current)

      // 100ms 检测滚动停止（更灵敏）
      scrollingTimerRef.current = window.setTimeout(() => {
        setIsScrolling(false)
      }, 100)

      // Hover 或拖拽时不隐藏，否则 1.5 秒后隐藏
      hideTimerRef.current = window.setTimeout(() => {
        if (!isDraggingRef.current && !isHovering) {
          setIsVisible(false)
        }
      }, 1500)
    }

    window.addEventListener('scroll', handleScroll, { passive: true })

    // 初始显示
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

      // Thumb is already where the pointer left it. Do not rematerialize
      // from window.scrollY — with `scroll-behavior: smooth` that value
      // lags and the thumb flashes back to the old spot.
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
      // Live page height: library infinite-load grows the document. Keep
      // the thumb under the pointer and map that position to current range
      // so release does not snap the thumb back.
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

  // SSR 安全检查 - 在服务端渲染时不渲染此组件
  if (typeof window === 'undefined' || typeof document === 'undefined') {
    return null
  }

  if (isLibraryCanvas) return null

  // 计算轨道尺寸
  const windowHeight = window.innerHeight
  const TRACK_HEIGHT = windowHeight * TRACK_HEIGHT_PERCENT

  // 检查是否有可滚动内容
  const scrollableHeight =
    document.documentElement.scrollHeight - window.innerHeight
  if (scrollableHeight <= 0) return null

  return (
    <>
      {/* 滚动条轨道容器：视觉条仍约 10px，命中区加宽避免资料库画布抢手势 */}
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
          // 进入：从右侧滑入到原位 + 从小变大 - 带回弹效果
          // 退出：从原位滑出到右侧 + 从大变小 - 平滑退出
          transform: isVisible
            ? 'translateY(-50%) translateX(0px) scale(1)'
            : 'translateY(-50%) translateX(60px) scale(0.8)',
          opacity: isVisible ? 1 : 0,
          // 进入使用回弹曲线，退出使用平滑曲线
          transition: isVisible
            ? 'opacity 0.4s cubic-bezier(0.34, 1.56, 0.64, 1), transform 0.5s cubic-bezier(0.34, 1.56, 0.64, 1)'
            : 'opacity 0.3s cubic-bezier(0.4, 0, 0.2, 1), transform 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
          pointerEvents: isVisible || isDragging ? 'auto' : 'none',
        }}
      >
        {/* 轨道背景 */}
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
            backdropFilter:
              typeof document !== 'undefined' &&
              document.documentElement.dataset.perfMode === 'exlight'
                ? 'none'
                : isDragging || isScrolling
                  ? 'blur(10px)'
                  : 'blur(6px)',
            boxShadow: isDragging
              ? `inset 0 0 24px color-mix(in srgb, var(--color-primary) 15%, transparent)`
              : isScrolling
                ? `inset 0 0 14px color-mix(in srgb, var(--color-primary) 8%, transparent)`
                : 'none',
            transform: `scaleX(${isDragging ? 1.15 : isScrolling ? 1.08 : isHovering ? 1.05 : 1})`,
            // 增强回弹效果
            transition: 'all 0.22s cubic-bezier(0.34, 1.56, 0.64, 1)',
          }}
        />

        {/* Thumb滑块 - 由 DOM 控制位置和高度，但提供初始值避免闪烁 */}
        <div
          ref={thumbRef}
          className="absolute left-1/2 w-2.5 -translate-x-1/2 rounded-full cursor-grab active:cursor-grabbing"
          role="scrollbar"
          aria-orientation="vertical"
          style={{
            // 设置初始值，防止 thumb 在 updateThumb 执行前不可见
            top: '0px',
            height: `${MIN_THUMB_HEIGHT}px`,
            touchAction: 'none',
            backgroundColor:
              'color-mix(in srgb, var(--color-primary) 70%, transparent)',
            opacity: 0.95,
            boxShadow: `0 0 14px color-mix(in srgb, var(--color-primary) 65%, transparent),
                       0 3px 10px color-mix(in srgb, var(--color-primary) 45%, transparent)`,
            transition: 'none',
            backdropFilter:
              typeof document !== 'undefined' &&
              document.documentElement.dataset.perfMode === 'exlight'
                ? 'none'
                : 'blur(4px)',
            willChange: isDragging ? 'top' : 'auto',
          }}
        />

        {/* 滚动进度指示器 - 微妙的脉冲效果 */}
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

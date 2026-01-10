/**
 * 自定义滚动条组件 - 彻底优化版
 * 替代浏览器原生滚动条，避免布局偏移
 * 特点：
 * - 固定30%高度轨道，居中显示
 * - 使用壁纸色（--color-primary）
 * - 流畅的进入/退出/滚动动画
 * - 超跟手的拖拽体验
 * - 仅在桌面端显示，移动端完全不加载
 * - 右侧 1rem 定位
 * - SPA 路由切换时平滑过渡尺寸
 */

import { useCallback, useEffect, useRef, useState } from 'react'
import { useLocation } from 'react-router-dom'
import { observeResize } from '../hooks/animation'
import { useSharedResize, useSharedScroll } from '../hooks/useSharedEventListener'

// 检测是否为移动端 - 使用多重检测确保准确性
function getIsMobile(): boolean {
  if (typeof window === 'undefined')
    return true // SSR 时视为移动端，不渲染

  // 多重检测: 屏幕宽度 + 触摸设备
  const isSmallScreen = window.matchMedia('(max-width: 767px)').matches
  const isTouchDevice = window.matchMedia('(hover: none) and (pointer: coarse)').matches

  return isSmallScreen || isTouchDevice
}

// 在模块加载时立即检测,避免任何延迟
const IS_MOBILE_DEVICE = typeof window !== 'undefined' ? getIsMobile() : true

/**
 * 主导出组件 - 移动端守卫
 * ⚠️ 关键优化: 在移动端完全不渲染,避免注册任何 Observer 和事件监听器
 * 这是防止移动端崩溃的第一道防线
 */
export default function CustomScrollbar() {
  // ⚠️ 关键: 使用模块级常量,避免首次渲染延迟
  // 如果是移动端,直接返回 null,不执行任何逻辑
  if (IS_MOBILE_DEVICE) {
    return null
  }

  const [isMobile, setIsMobile] = useState(getIsMobile)

  useEffect(() => {
    if (typeof window === 'undefined')
      return

    const mediaQuery = window.matchMedia('(max-width: 767px)')
    const handleChange = (e: MediaQueryListEvent) => {
      setIsMobile(e.matches)
    }

    if (mediaQuery.addEventListener) {
      mediaQuery.addEventListener('change', handleChange)
    }
    else {
      mediaQuery.addListener(handleChange)
    }

    return () => {
      if (mediaQuery.removeEventListener) {
        mediaQuery.removeEventListener('change', handleChange)
      }
      else {
        mediaQuery.removeListener(handleChange)
      }
    }
  }, [])

  // 二次检查: 响应式变化时也要检测
  if (isMobile) {
    return null
  }

  return <CustomScrollbarInner />
}

// ⚠️ 关键修复: 将常量移到组件外部，避免每次渲染重新创建
// 这是导致无限重渲染的根本原因！
const TRACK_HEIGHT_PERCENT = 0.3
const MIN_THUMB_HEIGHT = 40 // 最小 Thumb 高度（px）

/**
 * 内部滚动条组件 - 只在桌面端渲染
 */
function CustomScrollbarInner() {
  const location = useLocation()
  const [isDragging, setIsDragging] = useState(false)
  const [isVisible, setIsVisible] = useState(false)
  const [isScrolling, setIsScrolling] = useState(false)
  const [isHovering, setIsHovering] = useState(false)

  const dragStartRef = useRef({ scrollY: 0, clientY: 0 })
  const hideTimerRef = useRef<number | null>(null)
  const scrollingTimerRef = useRef<number | null>(null)
  const thumbRef = useRef<HTMLDivElement>(null)
  const isDraggingRef = useRef(false)
  const lastScrollTopRef = useRef(0)
  const dragEndTimeRef = useRef(0) // 记录拖动结束时间
  const isRouteTransitioningRef = useRef(false) // 路由切换中，禁止所有更新
  const routeTransitionTimeRef = useRef(0) // 记录路由切换开始时间
  const cachedDocumentHeightRef = useRef(0) // 🔧 缓存文档高度，减少重排
  const lastHeightCheckRef = useRef(0) // 上次检查高度的时间

  // 计算并更新 Thumb 的位置和高度
  const updateThumb = useCallback(() => {
    if (!thumbRef.current || isDraggingRef.current)
      return

    // 路由切换期间，禁止所有更新
    if (isRouteTransitioningRef.current)
      return

    const now = Date.now()
    const timeSinceDragEnd = now - dragEndTimeRef.current
    const timeSinceRouteTransition = now - routeTransitionTimeRef.current

    // 拖动结束后 1000ms 内，忽略所有更新（除非是路由切换后的更新）
    if (timeSinceDragEnd < 1000 && timeSinceRouteTransition > 2000) {
      return
    }

    const windowHeight = window.innerHeight

    // 🔧 优化：缓存 scrollHeight，每 500ms 最多更新一次
    // 这减少了大量的强制布局计算
    if (now - lastHeightCheckRef.current > 500 || cachedDocumentHeightRef.current === 0) {
      cachedDocumentHeightRef.current = document.documentElement.scrollHeight
      lastHeightCheckRef.current = now
    }
    const documentHeight = cachedDocumentHeightRef.current

    const scrollTop = window.scrollY
    const scrollableHeight = documentHeight - windowHeight

    if (scrollableHeight <= 0) {
      setIsVisible(false)
      return
    }

    // 计算轨道高度
    const trackHeight = windowHeight * TRACK_HEIGHT_PERCENT

    // 根据内容比例动态计算 Thumb 高度
    const viewportRatio = windowHeight / documentHeight
    const thumbHeight = Math.max(MIN_THUMB_HEIGHT, trackHeight * viewportRatio)

    // 计算可用轨道空间
    const availableTrackHeight = trackHeight - thumbHeight

    // 计算滚动百分比和 Thumb 位置
    const percentage = Math.max(0, Math.min(1, scrollTop / scrollableHeight))
    const thumbTop = percentage * availableTrackHeight

    // 路由切换后的平滑过渡（优先级最高）
    if (timeSinceRouteTransition >= 1000 && timeSinceRouteTransition < 1700) {
      // 路由切换的过渡动画已经在外部设置，这里直接更新即可
      // 不需要设置 transition，避免覆盖
    }
    // 拖动结束后的修正动画
    else if (timeSinceDragEnd >= 1000 && timeSinceDragEnd < 1600) {
      // 先设置过渡动画
      thumbRef.current.style.transition = 'top 0.6s cubic-bezier(0.4, 0, 0.2, 1), height 0.6s cubic-bezier(0.4, 0, 0.2, 1)'
    }
    else {
      thumbRef.current.style.transition = 'none'
    }

    // 同时更新高度和位置
    thumbRef.current.style.height = `${thumbHeight}px`
    thumbRef.current.style.top = `${thumbTop}px`

    // 记录当前 scrollTop
    lastScrollTopRef.current = scrollTop
  }, []) // ⚠️ 修复: 空依赖数组，因为常量已移到组件外部

  // RAF 节流的更新处理器 - 使用 useCallback 确保引用稳定
  const handleUpdate = useCallback(() => {
    if (isDraggingRef.current)
      return
    updateThumb()
  }, [updateThumb])

  // 使用共享的 scroll 和 resize 监听器
  // ⚠️ 关键优化: 使用共享事件监听器，减少重复注册
  // RAF 节流 (~60fps) 由共享监听器内部处理
  useSharedScroll(handleUpdate)
  useSharedResize(handleUpdate)

  // ResizeObserver 用于监听文档高度变化
  // ⚠️ 移除了 MutationObserver - 这是造成移动端崩溃的主要原因
  useEffect(() => {
    let throttleTimer: number | null = null

    const handleUpdateThrottled = () => {
      if (throttleTimer !== null)
        return
      throttleTimer = window.setTimeout(() => {
        handleUpdate()
        throttleTimer = null
      }, 100) // 100ms 节流
    }

    // 立即执行初始更新，确保 thumb 可见
    requestAnimationFrame(() => {
      updateThumb()
      // 再次更新以确保准确（处理初次渲染后的 DOM 变化）
      setTimeout(() => updateThumb(), 100)
    })

    // 使用共享 ResizeObserver 监听文档高度变化
    const unobserve = observeResize(document.documentElement, handleUpdateThrottled)

    return () => {
      unobserve()
      if (throttleTimer !== null)
        clearTimeout(throttleTimer)
    }
  }, [updateThumb, handleUpdate])

  // 监听路由变化，触发平滑过渡
  useEffect(() => {
    if (!thumbRef.current)
      return

    // 记录路由切换时间
    routeTransitionTimeRef.current = Date.now()

    // 立即设置标志，禁止所有更新
    isRouteTransitioningRef.current = true

    // 等待 DOM 重排和页面内容加载完成
    const initialDelay = setTimeout(() => {
      if (!thumbRef.current)
        return

      // 先启用过渡动画
      thumbRef.current.style.transition = 'top 0.5s cubic-bezier(0.4, 0, 0.2, 1), height 0.5s cubic-bezier(0.4, 0, 0.2, 1)'

      // 再等待一段时间后触发更新到新页面的尺寸
      const updateDelay = setTimeout(() => {
        // 解除更新禁止，并触发一次更新
        isRouteTransitioningRef.current = false
        // 🔧 路由变化时重置高度缓存
        cachedDocumentHeightRef.current = 0
        requestAnimationFrame(() => {
          updateThumb()
        })
      }, 150) // 再延迟 150ms 确保过渡已经设置好

      // 700ms 后移除过渡，恢复正常
      const transitionTimer = setTimeout(() => {
        if (thumbRef.current) {
          thumbRef.current.style.transition = 'none'
        }
      }, 700)

      return () => {
        clearTimeout(updateDelay)
        clearTimeout(transitionTimer)
      }
    }, 1000) // 等待 1000ms 让 DOM 完全重排和页面动画完成

    return () => {
      clearTimeout(initialDelay)
      // 清理时也要恢复标志
      isRouteTransitioningRef.current = false
    }
  }, [location.pathname, updateThumb])

  // 显示/隐藏滚动条 + 滚动状态检测
  useEffect(() => {
    let lastScrollTime = Date.now()

    const handleScroll = () => {
      const now = Date.now()
      lastScrollTime = now

      setIsVisible(true)
      setIsScrolling(true)

      // 清除之前的计时器
      if (hideTimerRef.current)
        clearTimeout(hideTimerRef.current)
      if (scrollingTimerRef.current)
        clearTimeout(scrollingTimerRef.current)

      // 100ms 检测滚动停止（更灵敏）
      scrollingTimerRef.current = window.setTimeout(() => {
        setIsScrolling(false)
      }, 100)

      // Hover 或拖拽时不隐藏，否则 1.5 秒后隐藏
      hideTimerRef.current = window.setTimeout(() => {
        if (!isDragging && !isHovering) {
          setIsVisible(false)
        }
      }, 1500)
    }

    window.addEventListener('scroll', handleScroll, { passive: true })

    // 初始显示
    const initialCheck = () => {
      const scrollableHeight = document.documentElement.scrollHeight - window.innerHeight
      if (scrollableHeight > 0) {
        setIsVisible(true)
        hideTimerRef.current = window.setTimeout(() => {
          if (!isDragging && !isHovering) {
            setIsVisible(false)
          }
        }, 2000)
      }
    }

    initialCheck()

    return () => {
      window.removeEventListener('scroll', handleScroll)
      if (hideTimerRef.current)
        clearTimeout(hideTimerRef.current)
      if (scrollingTimerRef.current)
        clearTimeout(scrollingTimerRef.current)
    }
  }, [isDragging, isHovering])

  // 处理拖拽开始
  const handleMouseDown = (e: React.MouseEvent) => {
    e.preventDefault()

    // 立即设置 ref 标志
    isDraggingRef.current = true

    // 保存拖动起始信息
    dragStartRef.current = {
      scrollY: window.scrollY,
      clientY: e.clientY,
    }

    setIsDragging(true)
    document.body.style.userSelect = 'none'
  }

  // 处理拖拽 - 完全同步，无延迟
  useEffect(() => {
    if (!isDragging)
      return

    const handleMouseMove = (e: MouseEvent) => {
      if (!thumbRef.current)
        return

      // 获取当前尺寸
      const windowHeight = window.innerHeight
      const documentHeight = document.documentElement.scrollHeight
      const scrollableHeight = documentHeight - windowHeight
      const trackHeight = windowHeight * TRACK_HEIGHT_PERCENT

      // 获取当前 Thumb 高度（动态计算的）
      const currentThumbHeight = Number.parseFloat(thumbRef.current.style.height || '0')
      const availableTrackHeight = trackHeight - currentThumbHeight

      // 计算鼠标移动距离
      const deltaY = e.clientY - dragStartRef.current.clientY

      // 计算新的滚动位置
      const scrollRatio = deltaY / availableTrackHeight
      const newScrollY = dragStartRef.current.scrollY + scrollRatio * scrollableHeight
      const clampedScrollY = Math.max(0, Math.min(newScrollY, scrollableHeight))

      // 同步更新滚动位置
      document.documentElement.scrollTop = clampedScrollY
      document.body.scrollTop = clampedScrollY

      // 记录这次设置的 scrollTop
      lastScrollTopRef.current = clampedScrollY

      // 计算并更新 Thumb 位置
      const percentage = clampedScrollY / scrollableHeight
      const thumbTop = percentage * availableTrackHeight
      thumbRef.current.style.top = `${thumbTop}px`
    }

    const handleMouseUp = () => {
      // 清除光标样式
      document.body.style.userSelect = ''
      document.body.style.cursor = ''

      // 记录拖动结束时间
      dragEndTimeRef.current = Date.now()

      // 清除拖动状态
      setIsDragging(false)
      isDraggingRef.current = false
    }

    // 全局拖拽光标
    document.body.style.cursor = 'grabbing'
    document.body.style.userSelect = 'none'

    document.addEventListener('mousemove', handleMouseMove, { passive: false })
    document.addEventListener('mouseup', handleMouseUp)
    document.addEventListener('mouseleave', handleMouseUp)

    return () => {
      document.removeEventListener('mousemove', handleMouseMove)
      document.removeEventListener('mouseup', handleMouseUp)
      document.removeEventListener('mouseleave', handleMouseUp)
      document.body.style.cursor = ''
      document.body.style.userSelect = ''
    }
  }, [isDragging, updateThumb])

  // 点击轨道跳转
  const handleTrackClick = (e: React.MouseEvent) => {
    if (e.target !== e.currentTarget)
      return // 只响应轨道点击，不响应thumb点击

    const rect = e.currentTarget.getBoundingClientRect()
    const clickY = e.clientY - rect.top
    const trackHeight = rect.height
    const percentage = clickY / trackHeight

    const windowHeight = window.innerHeight
    const documentHeight = document.documentElement.scrollHeight
    const scrollableHeight = documentHeight - windowHeight

    window.scrollTo({
      top: percentage * scrollableHeight,
      behavior: 'smooth',
    })
  }

  // SSR 安全检查 - 在服务端渲染时不渲染此组件
  if (typeof window === 'undefined' || typeof document === 'undefined') {
    return null
  }

  // 计算轨道尺寸
  const windowHeight = window.innerHeight
  const TRACK_HEIGHT = windowHeight * TRACK_HEIGHT_PERCENT

  // 检查是否有可滚动内容
  const scrollableHeight = document.documentElement.scrollHeight - window.innerHeight
  if (scrollableHeight <= 0)
    return null

  return (
    <>
      {/* 滚动条轨道容器 */}
      <div
        className="fixed right-4 w-[10px] z-[9999] hidden md:block pointer-events-none"
        onMouseEnter={() => setIsHovering(true)}
        onMouseLeave={() => setIsHovering(false)}
        style={{
          height: `${TRACK_HEIGHT}px`,
          top: '50%',
          // 进入：从右侧 (60px) 滑入到原位 (0) + 从小变大 - 带回弹效果
          // 退出：从原位 (0) 滑出到右侧 (60px) + 从大变小 - 平滑退出
          transform: isVisible
            ? 'translateY(-50%) translateX(0px) scale(1)'
            : 'translateY(-50%) translateX(60px) scale(0.8)',
          opacity: isVisible ? 1 : 0,
          // 进入使用回弹曲线，退出使用平滑曲线
          transition: isVisible
            ? 'opacity 0.4s cubic-bezier(0.34, 1.56, 0.64, 1), transform 0.5s cubic-bezier(0.34, 1.56, 0.64, 1)'
            : 'opacity 0.3s cubic-bezier(0.4, 0, 0.2, 1), transform 0.3s cubic-bezier(0.4, 0, 0.2, 1)',
          pointerEvents: isVisible ? 'auto' : 'none',
        }}
      >
        {/* 轨道背景 */}
        <div
          className="absolute inset-0 rounded-full"
          style={{
            backgroundColor: `color-mix(in srgb, var(--color-primary) ${
              isDragging ? '20%' : isScrolling ? '15%' : isHovering ? '12%' : '8%'
            }, transparent)`,
            backdropFilter: isDragging || isScrolling ? 'blur(10px)' : 'blur(6px)',
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
          className="absolute left-0 right-0 rounded-full cursor-grab active:cursor-grabbing pointer-events-auto"
          style={{
            // 设置初始值，防止 thumb 在 updateThumb 执行前不可见
            top: '0px',
            height: `${MIN_THUMB_HEIGHT}px`,
            backgroundColor: 'color-mix(in srgb, var(--color-primary) 70%, transparent)',
            opacity: 0.95,
            boxShadow: `0 0 14px color-mix(in srgb, var(--color-primary) 65%, transparent),
                       0 3px 10px color-mix(in srgb, var(--color-primary) 45%, transparent)`,
            transform: 'scaleX(1) scaleY(1)',
            transition: 'none',
            backdropFilter: 'blur(4px)',
            willChange: isDragging ? 'top' : 'auto',
          }}
          onMouseDown={handleMouseDown}
        />

        {/* 滚动进度指示器 - 微妙的脉冲效果 */}
        {isDragging && thumbRef.current && (
          <div
            className="absolute left-1/2 -translate-x-1/2 pointer-events-none"
            style={{
              top: `calc(${thumbRef.current.style.top || '0px'} + ${Number.parseFloat(thumbRef.current.style.height || '0') / 2}px)`,
              width: '16px',
              height: '16px',
              borderRadius: '50%',
              backgroundColor: `color-mix(in srgb, var(--color-primary) 30%, transparent)`,
              animation: 'pulse-ring 1.5s cubic-bezier(0.4, 0, 0.6, 1) infinite',
            }}
          />
        )}
      </div>

      <style>
        {`
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

/**
 * 导航岛自动隐藏 Hook
 * 管理滚动隐藏、鼠标边缘唤回、无操作超时、响应式断点等行为
 *
 * 布局（底栏 / 侧轨）与 utils/navLayout 一致，勿用纯 width≥768：
 * 平板触控 768–1023 为 mobile 底栏，若写 desktop 的 translateY 会盖住 CSS。
 *
 * 底栏↔侧轨切换时由 NavigationIsland 做 crossfade（data-nav-switch）；
 * 此期间不写 inline transform/opacity，避免与淡出/淡入抢控制权。
 */

import type { NavLayout } from '../utils/navLayout'
import { useEffect } from 'react'
import {
  getNavLayoutSnapshot,
  NAV_CHROME_SETTLED_EVENT,

  subscribeNavLayout,
} from '../utils/navLayout'

const INACTIVITY_DELAY = 5000
const SCROLL_THRESHOLD = 50
const PAGE_TOP_THRESHOLD = 100
const EDGE_THRESHOLD = 100

const TRANSFORM_SHOW_DESKTOP = 'translateY(-50%)'
const TRANSFORM_SHOW_MOBILE = 'translateX(-50%)'
const TRANSFORM_HIDE_DESKTOP = 'translateY(-50%) translateX(-20px)'
const TRANSFORM_HIDE_MOBILE = 'translateX(-50%) translateY(20px)'

const TRANSITION_VISIBILITY = 'opacity 0.3s ease, transform 0.3s ease'
/** Layout morph: never interpolate translateX(-50%) ↔ translateY(-50%). */
const TRANSITION_OPACITY_ONLY = 'opacity 0.3s ease'

function isChromeSwitching(nav: HTMLElement): boolean {
  const phase = nav.dataset.navSwitch
  return phase === 'out' || phase === 'in'
}

export function useNavAutoHide(selector = '.nav-container') {
  useEffect(() => {
    const navContainer = document.querySelector(selector) as HTMLElement
    if (!navContainer) return

    // ===== 状态 =====
    let lastScrollY = window.scrollY
    let rafId = 0
    let inactivityTimeoutId = 0
    let isHovering = false
    let isNavVisible = true
    let hiddenByScroll = false
    let cachedLayout: NavLayout = getNavLayoutSnapshot()
    let cachedWindowHeight = window.innerHeight

    navContainer.style.transition = TRANSITION_VISIBILITY

    const applyVisibility = (visible: boolean) => {
      // Crossfade owns opacity/transform while data-nav-switch is set.
      if (isChromeSwitching(navContainer)) return

      const desktop = cachedLayout === 'desktop'
      const transform = visible
        ? desktop
          ? TRANSFORM_SHOW_DESKTOP
          : TRANSFORM_SHOW_MOBILE
        : desktop
          ? TRANSFORM_HIDE_DESKTOP
          : TRANSFORM_HIDE_MOBILE

      navContainer.style.opacity = visible ? '1' : '0'
      navContainer.style.transform = transform
      navContainer.style.pointerEvents = visible ? 'auto' : 'none'
    }

    /** After layout morph: snap transform without animating old→new axes. */
    const snapVisibilityForLayout = (visible: boolean) => {
      if (isChromeSwitching(navContainer)) return
      navContainer.style.transition = TRANSITION_OPACITY_ONLY
      applyVisibility(visible)
      // Restore hide/show transform transition on next frame
      requestAnimationFrame(() => {
        if (!isChromeSwitching(navContainer)) {
          navContainer.style.transition = TRANSITION_VISIBILITY
        }
      })
    }

    // ===== 核心显示/隐藏 =====
    const showNav = () => {
      if (isNavVisible) return
      isNavVisible = true
      hiddenByScroll = false
      applyVisibility(true)
    }

    const hideNav = () => {
      if (!isNavVisible || isHovering) return
      isNavVisible = false
      applyVisibility(false)
    }

    const hideNavByScroll = () => {
      if (!isNavVisible || isHovering) return
      isNavVisible = false
      hiddenByScroll = true
      applyVisibility(false)
    }

    // ===== 无操作计时器 =====
    const clearInactivityTimer = () => {
      if (inactivityTimeoutId) {
        clearTimeout(inactivityTimeoutId)
        inactivityTimeoutId = 0
      }
    }

    const startInactivityTimer = () => {
      clearInactivityTimer()
      if (isNavVisible && !isChromeSwitching(navContainer)) {
        inactivityTimeoutId = window.setTimeout(hideNav, INACTIVITY_DELAY)
      }
    }

    // ===== 滚动处理 =====
    const processScroll = () => {
      if (isChromeSwitching(navContainer)) {
        rafId = 0
        return
      }
      const currentScrollY = window.scrollY
      const delta = currentScrollY - lastScrollY
      const isDown = delta > 0

      if (
        isDown &&
        delta > SCROLL_THRESHOLD &&
        currentScrollY > PAGE_TOP_THRESHOLD
      ) {
        clearInactivityTimer()
        hideNavByScroll()
      } else if (!isDown || currentScrollY < PAGE_TOP_THRESHOLD) {
        showNav()
        startInactivityTimer()
      }

      lastScrollY = currentScrollY
      rafId = 0
    }

    const handleScroll = () => {
      if (!rafId) {
        rafId = requestAnimationFrame(processScroll)
      }
    }

    // ===== 鼠标移动处理 =====
    let pendingMouseMove: MouseEvent | null = null
    let mouseRafId = 0

    const processMouseMove = () => {
      if (!pendingMouseMove) return
      const e = pendingMouseMove
      pendingMouseMove = null
      mouseRafId = 0
      if (isChromeSwitching(navContainer)) return

      // 侧轨：靠左缘唤回；底栏：靠底缘唤回
      const isNearEdge =
        cachedLayout === 'desktop'
          ? e.clientX < EDGE_THRESHOLD
          : e.clientY > cachedWindowHeight - EDGE_THRESHOLD

      if (isNearEdge) {
        showNav()
        startInactivityTimer()
      }
    }

    const handleMouseMove = (e: MouseEvent) => {
      pendingMouseMove = e
      if (!mouseRafId) {
        mouseRafId = requestAnimationFrame(processMouseMove)
      }
    }

    // ===== 交互处理 =====
    const handleInteraction = () => {
      if (isChromeSwitching(navContainer)) return
      if (!hiddenByScroll) {
        showNav()
        startInactivityTimer()
      }
    }

    // ===== 导航岛悬停 =====
    const handleNavEnter = () => {
      if (isChromeSwitching(navContainer)) return
      isHovering = true
      clearInactivityTimer()
      showNav()
    }

    const handleNavLeave = () => {
      isHovering = false
      startInactivityTimer()
    }

    // Desired layout changed (store). Chrome may still be crossfading — only
    // cache the token; transform snap happens on NAV_CHROME_SETTLED_EVENT.
    const handleLayoutDesire = () => {
      cachedLayout = getNavLayoutSnapshot()
    }

    const handleChromeSettled = () => {
      cachedLayout = getNavLayoutSnapshot()
      isNavVisible = true
      hiddenByScroll = false
      clearInactivityTimer()
      // Clear any residual inline from before switch, then snap show pose.
      navContainer.style.removeProperty('opacity')
      navContainer.style.removeProperty('transform')
      navContainer.style.removeProperty('pointer-events')
      snapVisibilityForLayout(true)
      startInactivityTimer()
    }

    const handleResize = () => {
      cachedWindowHeight = window.innerHeight
    }

    // ===== 初始化 & 事件注册 =====
    applyVisibility(true)

    const controller = new AbortController()
    const { signal } = controller
    const passive = { passive: true, signal }

    window.addEventListener('scroll', handleScroll, passive)
    window.addEventListener('mousemove', handleMouseMove, passive)
    window.addEventListener('keydown', handleInteraction, passive)
    window.addEventListener('click', handleInteraction, passive)
    window.addEventListener('touchstart', handleInteraction, passive)
    window.addEventListener('resize', handleResize, passive)
    navContainer.addEventListener('mouseenter', handleNavEnter, { signal })
    navContainer.addEventListener('mouseleave', handleNavLeave, { signal })
    navContainer.addEventListener(NAV_CHROME_SETTLED_EVENT, handleChromeSettled, {
      signal,
    })

    const unsubscribeLayout = subscribeNavLayout(handleLayoutDesire)
    startInactivityTimer()

    return () => {
      controller.abort()
      unsubscribeLayout()
      if (rafId) cancelAnimationFrame(rafId)
      if (mouseRafId) cancelAnimationFrame(mouseRafId)
      clearInactivityTimer()
    }
  }, [selector])
}

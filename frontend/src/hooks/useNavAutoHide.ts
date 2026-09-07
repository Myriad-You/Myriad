/**
 * 导航岛自动隐藏 Hook
 * 管理滚动隐藏、鼠标边缘唤回、无操作超时、响应式断点等行为
 *
 * 布局（底栏 / 侧轨）与 utils/navLayout 一致，勿用纯 width≥768：
 * 平板触控 768–1023 为 mobile 底栏，若写 desktop 的 translateY 会盖住 CSS。
 *
 * 底栏↔侧轨切换时由 NavigationIsland 做 crossfade（data-nav-switch）；
 * 此期间不写 inline transform/opacity，避免与淡出/淡入抢控制权。
 *
 * 边缘唤回只认进入沿（离开后再进来，或推到热边）。岛本身就在邻近带里，
 * 带内的 mousemove 不能刷新无操作计时，否则空闲隐藏永远不会发生。
 */

import type { NavLayout } from '../utils/navLayout'
import { useEffect } from 'react'
import {
  isTourDomActive,
  TOUR_ACTIVE_EVENT,
} from '../components/tour/tourLogic'
import {
  edgeRevealShouldShow,
  isNearNavEdge,
  NAV_HOT_EDGE_THRESHOLD,
  navScrollDecision,
} from '../utils/navAutoHide'
import {
  getNavLayoutSnapshot,
  NAV_CHROME_SETTLED_EVENT,

  subscribeNavLayout,
} from '../utils/navLayout'

const INACTIVITY_DELAY = 5000

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

    // 状态
    const readScrollY = () =>
      document.scrollingElement?.scrollTop ?? window.scrollY ?? 0

    let lastScrollY = readScrollY()
    let rafId = 0
    let hoverSyncRaf = 0
    let inactivityTimeoutId = 0
    let isHovering = false
    let isNavVisible = true
    let hiddenByScroll = false
    let insideEdge = false
    let insideHotEdge = false
    let edgePrimed = false
    let cachedLayout: NavLayout = getNavLayoutSnapshot()
    let cachedWindowHeight = window.innerHeight
    const hoverCapable = window.matchMedia('(hover: hover)').matches

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

      // Idle-hide used to leave an opacity:0 island with backdrop-filter on.
      // That backdrop root samples #wallpaper (filter + parallax transform) and
      // can freeze those updates. Drop the glass before the fade/slide.
      navContainer.dataset.navIdle = visible ? 'shown' : 'hidden'
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

    // 核心显示/隐藏
    const showNav = () => {
      if (isNavVisible) return
      isNavVisible = true
      hiddenByScroll = false
      applyVisibility(true)
    }

    const syncHoverFromDom = () => {
      // Touch :hover sticks after tap; only fine pointers pause idle hide.
      isHovering = hoverCapable && navContainer.matches(':hover')
    }

    const isPointerOverNav = () =>
      isHovering && navContainer.matches(':hover')

    const hideNav = (byScroll = false) => {
      if (isTourDomActive()) return
      if (!isNavVisible) return
      // Touch can fire pointerenter without a matching leave; don't let a
      // sticky flag block idle hide if the pointer is not actually over us.
      if (isPointerOverNav()) return
      isHovering = false
      isNavVisible = false
      hiddenByScroll = byScroll
      applyVisibility(false)
    }

    // 无操作计时器
    const clearInactivityTimer = () => {
      if (inactivityTimeoutId) {
        clearTimeout(inactivityTimeoutId)
        inactivityTimeoutId = 0
      }
    }

    const startInactivityTimer = () => {
      clearInactivityTimer()
      if (isTourDomActive()) return
      if (isNavVisible && !isChromeSwitching(navContainer)) {
        inactivityTimeoutId = window.setTimeout(hideNav, INACTIVITY_DELAY)
      }
    }

    // 滚动处理
    const processScroll = () => {
      if (isChromeSwitching(navContainer)) {
        rafId = 0
        return
      }
      const currentScrollY = readScrollY()
      const decision = navScrollDecision(currentScrollY, lastScrollY)
      if (decision === 'hide') {
        clearInactivityTimer()
        hideNav(true)
      } else if (decision === 'show') {
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

    // 鼠标移动处理
    let pendingMouseMove: MouseEvent | null = null
    let mouseRafId = 0

    const processMouseMove = () => {
      if (!pendingMouseMove) return
      const e = pendingMouseMove
      pendingMouseMove = null
      mouseRafId = 0
      if (isChromeSwitching(navContainer)) return

      const reveal = edgeRevealShouldShow({
        visible: isNavVisible,
        primed: edgePrimed,
        wasInsideProximity: insideEdge,
        isInsideProximity: isNearNavEdge(
          cachedLayout,
          e.clientX,
          e.clientY,
          cachedWindowHeight,
        ),
        wasInsideHot: insideHotEdge,
        isInsideHot: isNearNavEdge(
          cachedLayout,
          e.clientX,
          e.clientY,
          cachedWindowHeight,
          NAV_HOT_EDGE_THRESHOLD,
        ),
      })
      edgePrimed = reveal.primed
      insideEdge = reveal.insideProximity
      insideHotEdge = reveal.insideHot
      // Proximity must not refresh the idle timer while the island is already
      // visible — the rail sits inside that band.
      if (reveal.show) {
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

    // 交互处理
    const handleInteraction = () => {
      if (isChromeSwitching(navContainer)) return
      if (hiddenByScroll) return
      showNav()
      if (!isHovering) startInactivityTimer()
    }

    // Pause idle hide only for real hover. Touch synthesizes enter without
    // leave, which used to pin isHovering and disable auto-hide entirely.
    const isHoverPointer = (e: PointerEvent) =>
      e.pointerType === 'mouse' || e.pointerType === 'pen'

    const handleNavEnter = (e: PointerEvent) => {
      if (!isHoverPointer(e) || isChromeSwitching(navContainer)) return
      isHovering = true
      clearInactivityTimer()
      showNav()
    }

    const handleNavLeave = (e: PointerEvent) => {
      if (!isHoverPointer(e)) return
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
      navContainer.removeAttribute('data-nav-idle')
      snapVisibilityForLayout(true)
      // pointer-events just came back; :hover / pointerenter may lag one frame.
      if (hoverSyncRaf) cancelAnimationFrame(hoverSyncRaf)
      hoverSyncRaf = requestAnimationFrame(() => {
        hoverSyncRaf = 0
        syncHoverFromDom()
        if (!isHovering) startInactivityTimer()
      })
    }

    const handleResize = () => {
      cachedWindowHeight = window.innerHeight
    }

    const handleTourActive = () => {
      if (isTourDomActive()) {
        // Edit mode (and other immersive chrome) keeps the island hidden.
        // Do not snap it back just because a tour started.
        if (navContainer.classList.contains('immersive')) {
          return
        }
        // Snap to the shown pose. A 300ms transform transition would leave
        // getBoundingClientRect mid-slide, and the tour hole/card would lock
        // onto the idle-hide offset (translateX(-20px) on desktop).
        isNavVisible = true
        hiddenByScroll = false
        clearInactivityTimer()
        if (!isChromeSwitching(navContainer)) {
          navContainer.style.transition = 'none'
          applyVisibility(true)
        }
        return
      }
      if (!isChromeSwitching(navContainer)) {
        navContainer.style.transition = TRANSITION_VISIBILITY
      }
      if (!isHovering) startInactivityTimer()
    }

    // 初始化 & 事件注册
    applyVisibility(true)

    const controller = new AbortController()
    const { signal } = controller
    const passive = { passive: true, signal }

    window.addEventListener('scroll', handleScroll, passive)
    document.scrollingElement?.addEventListener('scroll', handleScroll, {
      passive: true,
      signal,
    })
    window.addEventListener('mousemove', handleMouseMove, passive)
    window.addEventListener('keydown', handleInteraction, passive)
    window.addEventListener('click', handleInteraction, passive)
    window.addEventListener('touchstart', handleInteraction, passive)
    window.addEventListener('resize', handleResize, passive)
    window.addEventListener(TOUR_ACTIVE_EVENT, handleTourActive, { signal })
    navContainer.addEventListener('pointerenter', handleNavEnter, { signal })
    navContainer.addEventListener('pointerleave', handleNavLeave, { signal })
    navContainer.addEventListener(NAV_CHROME_SETTLED_EVENT, handleChromeSettled, {
      signal,
    })

    const unsubscribeLayout = subscribeNavLayout(handleLayoutDesire)
    // pointerenter does not fire if the pointer is already over the island.
    syncHoverFromDom()
    if (!isHovering) startInactivityTimer()

    return () => {
      controller.abort()
      unsubscribeLayout()
      if (rafId) cancelAnimationFrame(rafId)
      if (mouseRafId) cancelAnimationFrame(mouseRafId)
      if (hoverSyncRaf) cancelAnimationFrame(hoverSyncRaf)
      clearInactivityTimer()
    }
  }, [selector])
}

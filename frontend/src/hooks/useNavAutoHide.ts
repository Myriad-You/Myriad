/** 布局与 navLayout 一致，勿用纯 width≥768：平板触控 768–1023 是 mobile 底栏。 */
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

/** 布局变形时不要在 translateX(-50%) 与 translateY(-50%) 之间插值。 */
const TRANSITION_OPACITY_ONLY = 'opacity 0.3s ease'

function isChromeSwitching(nav: HTMLElement): boolean {
  const phase = nav.dataset.navSwitch
  return phase === 'out' || phase === 'in'
}

export function useNavAutoHide(selector = '.nav-container') {
  useEffect(() => {
    const navContainer = document.querySelector(selector) as HTMLElement
    if (!navContainer) return

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
      // Crossfade 期间不写 opacity/transform。
      if (isChromeSwitching(navContainer)) return

      const desktop = cachedLayout === 'desktop'
      const transform = visible
        ? desktop
          ? TRANSFORM_SHOW_DESKTOP
          : TRANSFORM_SHOW_MOBILE
        : desktop
          ? TRANSFORM_HIDE_DESKTOP
          : TRANSFORM_HIDE_MOBILE

      // 隐藏前去掉玻璃：opacity:0 仍带着 backdrop-filter 会冻住 #wallpaper 采样。
      navContainer.dataset.navIdle = visible ? 'shown' : 'hidden'
      navContainer.style.opacity = visible ? '1' : '0'
      navContainer.style.transform = transform
      navContainer.style.pointerEvents = visible ? 'auto' : 'none'
    }

    /** 布局变形后立刻 snap transform，不要播旧轴→新轴。 */
    const snapVisibilityForLayout = (visible: boolean) => {
      if (isChromeSwitching(navContainer)) return
      navContainer.style.transition = TRANSITION_OPACITY_ONLY
      applyVisibility(visible)

      requestAnimationFrame(() => {
        if (!isChromeSwitching(navContainer)) {
          navContainer.style.transition = TRANSITION_VISIBILITY
        }
      })
    }

    const showNav = () => {
      if (isNavVisible) return
      isNavVisible = true
      hiddenByScroll = false
      applyVisibility(true)
    }

    const syncHoverFromDom = () => {
      // 触摸 :hover 会粘住；仅 hover:hover 才暂停空闲隐藏。
      isHovering = hoverCapable && navContainer.matches(':hover')
    }

    const isPointerOverNav = () =>
      isHovering && navContainer.matches(':hover')

    const hideNav = (byScroll = false) => {
      if (isTourDomActive()) return
      if (!isNavVisible) return

      // 触摸可能只有 enter 没有 leave；指针不在岛上时不要挡住空闲隐藏。
      if (isPointerOverNav()) return
      isHovering = false
      isNavVisible = false
      hiddenByScroll = byScroll
      applyVisibility(false)
    }

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

      // 岛已可见时邻近带不得刷新空闲计时——侧轨就在带内。
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

    const handleInteraction = () => {
      if (isChromeSwitching(navContainer)) return
      if (hiddenByScroll) return
      showNav()
      if (!isHovering) startInactivityTimer()
    }

    // 只把真 hover 当暂停；触摸合成的 enter 不配对 leave。
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

    // 只缓存 layout token；transform snap 等 NAV_CHROME_SETTLED_EVENT。
    const handleLayoutDesire = () => {
      cachedLayout = getNavLayoutSnapshot()
    }

    const handleChromeSettled = () => {
      cachedLayout = getNavLayoutSnapshot()
      isNavVisible = true
      hiddenByScroll = false
      clearInactivityTimer()

      // 清掉切换残留的 inline，再 snap 到显示姿态。
      navContainer.style.removeProperty('opacity')
      navContainer.style.removeProperty('transform')
      navContainer.style.removeProperty('pointer-events')
      navContainer.removeAttribute('data-nav-idle')
      snapVisibilityForLayout(true)

      if (hoverSyncRaf) cancelAnimationFrame(hoverSyncRaf)
      // pointer-events 刚恢复；:hover / pointerenter 可能晚一帧。
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
        // 沉浸 chrome 保持隐藏；教程开始不要把它 snap 回来。
        if (navContainer.classList.contains('immersive')) {
          return
        }

        isNavVisible = true
        hiddenByScroll = false
        clearInactivityTimer()
        if (!isChromeSwitching(navContainer)) {
          // snap 显示姿态，避免 300ms 过渡让教程洞对准 idle-hide 位移。
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
    // pointerenter 在指针已在岛上时不会再触发。

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

import type { RefObject } from 'react'
import { useLayoutEffect, useRef } from 'react'
import { agentPanelStaggerSteps } from './agentPanelStage'
import {
  applyConversationExit,
  clampConversationScroll,
  CONVERSATION_FADE_PX,
  CONVERSATION_FLING_TAU,
  CONVERSATION_FOLLOW_TAU,
  CONVERSATION_LOAD_MORE_PX,
  CONVERSATION_NEAR_BOTTOM_PX,
  conversationExitKey,
  conversationExitStyle,
  conversationHoldExitOnClose,
  conversationMaxScroll,
  conversationShellLimit,
  conversationViewHeight,
  decayVelocity,
  rubberband,
  sampleVelocity,
  smoothToward,
  stillCoasting,
  wheelDeltaY,
} from './conversationPan'

interface Card {
  el: HTMLElement
  box: HTMLElement
  top: number
  height: number
  key: string
}

export function useConversationPan(
  viewportRef: RefObject<HTMLElement | null>,
  trackRef: RefObject<HTMLElement | null>,
  enabled: boolean,
  resetKey: unknown,
  cardSelector = '.agent-panel-message',
  onNearStart?: () => void,
): void {
  const nearStartRef = useRef(onNearStart)
  nearStartRef.current = onNearStart

  useLayoutEffect(() => {
    if (!enabled) return
    const viewport = viewportRef.current
    const track = trackRef.current
    if (!viewport || !track) return

    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const closestAnchor = viewport.closest('.agent-panel-overlay-anchor')
    const anchor = closestAnchor instanceof HTMLElement ? closestAnchor : null
    const closestSlot = viewport.closest('.agent-panel-messages-slot')
    const slot = closestSlot instanceof HTMLElement ? closestSlot : null
    const composer = anchor?.querySelector('.agent-panel-composer')
    const rail = anchor?.querySelector('.agent-panel-tag-rail')
    const moveMs = (() => {
      const raw = anchor
        ? getComputedStyle(anchor).getPropertyValue('--agent-move')
        : ''
      const parsed = Number.parseFloat(raw)
      return Number.isFinite(parsed) && parsed > 0 ? parsed : 480
    })()
    let current = 0
    let target = 0
    let velocity = 0
    let nearBottom = true
    let dragging = false
    let touchY = 0
    let frame = 0
    let exitFrame = 0
    let last = 0
    let trackH = 0
    let viewH = 0
    let followUntil = 0
    let cards: Card[] = []
    const samples: Array<{ t: number; x: number }> = []

    const maxScroll = () => conversationMaxScroll(trackH, viewH)

    const recache = () => {
      const nextH = track.offsetHeight
      if (trackH > 0 && nextH > trackH && !nearBottom) {
        const delta = nextH - trackH
        current += delta
        target += delta
      }
      trackH = nextH
      cards = Iterator.from(
        track.querySelectorAll<HTMLElement>(cardSelector),
      )
        .map((el) => {
          const box =
            (el.closest('.agent-panel-presence') as HTMLElement | null) ?? el
          return {
            el,
            box,
            top: box.offsetTop,
            height: box.offsetHeight,
            key: '',
          }
        })
        .toArray()
    }

    const measure = () => {
      if (anchor) {
        const style = getComputedStyle(anchor)
        const gap = Number.parseFloat(style.rowGap) || 12
        const reserved =
          (composer instanceof HTMLElement ? composer.offsetHeight : 0) +
          (rail instanceof HTMLElement ? rail.offsetHeight : 0) +
          gap
        const shell = conversationShellLimit(
          Number.parseFloat(style.maxHeight),
          window.innerHeight,
        )
        viewH = conversationViewHeight(trackH, Math.max(48, shell - reserved))
      } else {
        viewH = viewport.clientHeight
      }
      if (viewH < 32) {
        viewH = conversationViewHeight(
          trackH,
          conversationShellLimit(0, window.innerHeight),
        )
      }
    }

    const clearExit = () => {
      for (const card of cards) {
        applyConversationExit(card.el, { exit: 0, shift: 0, hidden: false })
        card.box.style.removeProperty('--agent-exit-stagger')
        card.key = ''
      }
    }

    const writeExitStagger = () => {
      const ranked = cards
        .filter((card) => card.el.style.visibility !== 'hidden')
        .toSorted((a, b) => b.top + b.height - (a.top + a.height))
      ranked.forEach((card, index) => {
        card.box.style.setProperty(
          '--agent-exit-stagger',
          String(agentPanelStaggerSteps(index + 1)),
        )
      })
      if (anchor) {
        anchor.style.setProperty(
          '--agent-stagger-wave',
          String(agentPanelStaggerSteps(ranked.length)),
        )
      }
    }

    const writeExit = () => {
      if (viewH < 32) return
      const leaving =
        anchor?.dataset.phase === 'closing' || slot?.dataset.exiting === 'true'
      if (leaving) {
        for (const card of cards) {
          const style = conversationExitStyle(
            card.top - current,
            card.top + card.height - current,
            0,
            viewH,
            CONVERSATION_FADE_PX,
          )
          if (conversationHoldExitOnClose(style)) {
            const key = conversationExitKey(style)
            if (key !== card.key) {
              card.key = key
              applyConversationExit(card.el, style)
            }
            continue
          }
          applyConversationExit(card.el, { exit: 0, shift: 0, hidden: false })
          card.key = 'r'
        }
        writeExitStagger()
        return
      }
      for (const card of cards) {
        const style = conversationExitStyle(
          card.top - current,
          card.top + card.height - current,
          0,
          viewH,
          CONVERSATION_FADE_PX,
        )
        const key = conversationExitKey(style)
        if (key === card.key) continue
        card.key = key
        applyConversationExit(card.el, style)
      }
    }

    const writeCap = (max: number) => {
      const flag = max > 0 ? 'true' : 'false'
      if (viewport.dataset.capped !== flag) viewport.dataset.capped = flag
      if (anchor && anchor.dataset.capped !== flag) {
        anchor.dataset.capped = flag
      }
    }

    const writeTransform = () => {
      const max = maxScroll()
      writeCap(max)
      track.style.transform = max > 0 ? `translate3d(0, ${-current}px, 0)` : ''
      // pinned-bottom current can still be < 72; that is not "scrolled to top"
      if (max > 0 && !nearBottom && current <= CONVERSATION_LOAD_MORE_PX) {
        nearStartRef.current?.()
      }
    }

    const scheduleExit = () => {
      if (exitFrame) return
      exitFrame = requestAnimationFrame(() => {
        exitFrame = 0
        writeExit()
      })
    }

    const write = () => {
      writeTransform()
      writeExit()
    }

    const stop = () => {
      if (frame) cancelAnimationFrame(frame)
      if (exitFrame) cancelAnimationFrame(exitFrame)
      frame = 0
      exitFrame = 0
      last = 0
      track.style.willChange = ''
    }

    const tick = (now: number) => {
      const dt = last ? Math.min(0.032, (now - last) / 1000) : 1 / 60
      last = now
      const following = now < followUntil
      if (following) measure()
      const max = maxScroll()

      if (!dragging && velocity !== 0) {
        target += velocity * dt
        velocity = decayVelocity(velocity, dt, CONVERSATION_FLING_TAU)
        if (target < 0 || target > max) {
          target = clampConversationScroll(target, max)
          velocity = 0
        }
      } else if (!dragging) {
        velocity = 0
      }

      if (nearBottom && !dragging) target = max
      if (!dragging) target = clampConversationScroll(target, max)
      current =
        reduce || (following && nearBottom && !dragging)
          ? target
          : smoothToward(current, target, dt, CONVERSATION_FOLLOW_TAU)
      write()

      if (
        stillCoasting(current, target, velocity, dragging) ||
        now < followUntil
      ) {
        frame = requestAnimationFrame(tick)
      } else {
        current = target
        write()
        stop()
      }
    }

    const kick = () => {
      if (maxScroll() > 0 || followUntil > 0)
        track.style.willChange = 'transform'
      if (!frame) frame = requestAnimationFrame(tick)
    }

    const followComposer = () => {
      if (blocked()) return
      followUntil = performance.now() + moveMs + 32
      measure()
      if (nearBottom) target = maxScroll()
      kick()
    }

    const blocked = () =>
      anchor?.dataset.phase === 'closing' || slot?.dataset.exiting === 'true'

    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey || blocked()) return
      measure()
      const max = maxScroll()
      if (max <= 0) return
      event.preventDefault()
      velocity = 0
      target = clampConversationScroll(
        target + wheelDeltaY(event.deltaY, event.deltaMode),
        max,
      )
      current = target
      nearBottom = max - target <= CONVERSATION_NEAR_BOTTOM_PX
      track.style.willChange = 'transform'
      writeTransform()
      scheduleExit()
    }

    const onTouchStart = (event: TouchEvent) => {
      if (event.touches.length !== 1 || blocked()) return
      dragging = true
      velocity = 0
      touchY = event.touches[0].clientY
      samples.length = 0
      samples.push({ t: event.timeStamp, x: target })
      last = 0
      track.style.willChange = 'transform'
    }

    const onTouchMove = (event: TouchEvent) => {
      if (!dragging || event.touches.length !== 1 || blocked()) return
      measure()
      const max = maxScroll()
      if (max <= 0) return
      const y = event.touches[0].clientY
      const dy = touchY - y
      touchY = y
      if (dy === 0) return
      event.preventDefault()
      target = rubberband(target + dy, max, viewH)
      current = target
      nearBottom =
        max - clampConversationScroll(target, max) <=
        CONVERSATION_NEAR_BOTTOM_PX
      samples.push({ t: event.timeStamp, x: target })
      if (samples.length > 12) samples.shift()
      writeTransform()
      scheduleExit()
    }

    const onTouchEnd = (event: TouchEvent) => {
      if (!dragging) return
      dragging = false
      if (blocked()) return
      measure()
      const max = maxScroll()
      velocity = sampleVelocity(samples, event.timeStamp)
      samples.length = 0
      if (target < 0 || target > max) {
        target = clampConversationScroll(target, max)
        velocity = 0
      }
      nearBottom = max - target <= CONVERSATION_NEAR_BOTTOM_PX
      kick()
    }

    recache()
    measure()
    if (nearBottom) target = maxScroll()
    current = target
    write()

    const resize = new ResizeObserver(() => {
      if (blocked()) return
      const nextH = track.offsetHeight
      if (nextH !== trackH || cards.length === 0) recache()
      const prevView = viewH
      const prevMax = maxScroll()
      const wasPinned =
        nearBottom && Math.abs(current - prevMax) <= CONVERSATION_NEAR_BOTTOM_PX
      measure()
      const max = maxScroll()
      if (nearBottom) target = max
      // pin to bottom; sliding from the top would hide in-view cards
      if (nearBottom && (wasPinned || current <= CONVERSATION_NEAR_BOTTOM_PX)) {
        current = target
        write()
        if (viewH !== prevView) followComposer()
        return
      }
      writeCap(max)
      if (
        dragging ||
        velocity !== 0 ||
        Math.abs(target - current) > CONVERSATION_NEAR_BOTTOM_PX
      ) {
        kick()
        return
      }
      if (viewH !== prevView || max !== prevMax) writeExit()
    })
    resize.observe(track)
    resize.observe(viewport)
    if (anchor) resize.observe(anchor)
    if (composer instanceof HTMLElement) resize.observe(composer)
    const phaseWatch = new MutationObserver(() => write())
    if (anchor) {
      phaseWatch.observe(anchor, {
        attributes: true,
        attributeFilter: ['data-phase'],
      })
    }
    const modeWatch = new MutationObserver(followComposer)
    if (composer instanceof HTMLElement) {
      modeWatch.observe(composer, {
        attributes: true,
        attributeFilter: ['data-mode'],
      })
    }
    if (anchor) {
      modeWatch.observe(anchor, {
        attributes: true,
        attributeFilter: ['data-mode'],
      })
    }
    if (slot) {
      phaseWatch.observe(slot, {
        attributes: true,
        attributeFilter: ['data-exiting'],
      })
    }
    const onVis = () => {
      if (document.hidden) stop()
      else if (stillCoasting(current, target, velocity, dragging)) kick()
    }
    document.addEventListener('visibilitychange', onVis)
    viewport.addEventListener('wheel', onWheel, { passive: false })
    viewport.addEventListener('touchstart', onTouchStart, { passive: true })
    viewport.addEventListener('touchmove', onTouchMove, { passive: false })
    viewport.addEventListener('touchend', onTouchEnd)
    viewport.addEventListener('touchcancel', onTouchEnd)

    return () => {
      stop()
      resize.disconnect()
      phaseWatch.disconnect()
      modeWatch.disconnect()
      document.removeEventListener('visibilitychange', onVis)
      viewport.removeEventListener('wheel', onWheel)
      viewport.removeEventListener('touchstart', onTouchStart)
      viewport.removeEventListener('touchmove', onTouchMove)
      viewport.removeEventListener('touchend', onTouchEnd)
      viewport.removeEventListener('touchcancel', onTouchEnd)
      track.style.transform = ''
      delete viewport.dataset.capped
      if (anchor) {
        delete anchor.dataset.capped
        anchor.style.removeProperty('--agent-stagger-wave')
      }
      clearExit()
    }
  }, [cardSelector, enabled, resetKey, trackRef, viewportRef])
}

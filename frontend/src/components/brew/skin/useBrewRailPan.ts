/** 轨道只做 transform，不设 overflow、不切遮罩。前一张从左边溢出，不退场。 */

import type { RefObject } from 'react'
import type { ConversationExitStyle } from '../../agent-panel/conversationPan'

import { useLayoutEffect, useRef } from 'react'
import {
  applyConversationExit,
  clampConversationScroll,
  CONVERSATION_FADE_PX,
  conversationExitKey,
  conversationExitStyle,

  rubberband,
  sampleVelocity,
  smoothToward,
  wheelDeltaY,
} from '../../agent-panel/conversationPan'
import {
  isDiscreteWheel,
  nearestRailSlot,
  neighborRailSlot,
  RAIL_OVERFLOW_LEFT_PX,
  RAIL_SEAT_PX,
  RAIL_WHEEL_SETTLE_MS,
  railLeadIndex,
  railMaxScroll,
  railOverflowLeft,
  railSeatScroll,
  railSeatSlots,
  railSettleTau,
  railSlotOffsets,
  scrollFromTrackTransform,
  settleRailSlot,
} from './railPan'

export { railLeadIndex } from './railPan'

function applyRailExit(el: HTMLElement, style: ConversationExitStyle): void {
  applyConversationExit(el, style)
  if (!style.hidden) el.style.pointerEvents = ''
  if (style.hidden || style.exit <= 0.01) {
    el.style.removeProperty('--exit-x')
    return
  }
  el.style.setProperty('--exit-x', `${style.shift}px`)
}

export interface BrewRailApi {
  align: (id: number, immediate?: boolean) => void
}

function wheelDelta(event: WheelEvent): number {
  const dominant =
    Math.abs(event.deltaX) > Math.abs(event.deltaY)
      ? event.deltaX
      : event.deltaY
  return wheelDeltaY(dominant, event.deltaMode)
}

export function useBrewRailPan(
  viewportRef: RefObject<HTMLElement | null>,
  trackRef: RefObject<HTMLElement | null>,
  enabled: boolean,
  resetKey: unknown,
  cardSelector: string,
  onLeadChange?: (id: number) => void,
  apiRef?: RefObject<BrewRailApi | null>,
  overflowLeft = false,
): void {
  const onLeadChangeRef = useRef(onLeadChange)
  onLeadChangeRef.current = onLeadChange

  useLayoutEffect(() => {
    if (!enabled) return
    const viewport = viewportRef.current
    const track = trackRef.current
    if (!viewport || !track) return

    const reduce = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    let current = 0
    let target = 0
    let dragging = false
    let seating = false
    let touchX = 0
    let frame = 0
    let exitFrame = 0
    let settleTimer = 0
    let last = 0
    let wheelStarted = 0
    let wheelAcc = 0
    let wheelVel = 0
    let home = 0
    let trackW = 0
    let viewW = 0
    let cards: Array<{
      el: HTMLElement
      left: number
      width: number
      key: string
    }> = []
    const samples: Array<{ t: number; x: number }> = []
    let lastLead = ''

    const overflowPx = overflowLeft ? RAIL_OVERFLOW_LEFT_PX : 0
    const slotsOf = () => railSeatSlots(railSlotOffsets(cards), overflowPx)
    const maxScroll = () => railMaxScroll(railSlotOffsets(cards), overflowPx)

    const slotAt = (scroll: number) =>
      nearestRailSlot(scroll, slotsOf(), maxScroll())

    const restOnSlot = (scroll: number) =>
      Math.abs(scroll - slotAt(scroll)) <= 0.6

    const recache = () => {
      trackW = track.scrollWidth
      cards = Iterator.from(
        track.querySelectorAll<HTMLElement>(cardSelector),
      )
        .map((el) => ({
          el,
          left: el.offsetLeft,
          width: el.offsetWidth,
          key: '',
        }))
        .toArray()
    }

    const measure = () => {
      viewW = viewport.clientWidth
    }

    const clearExit = () => {
      for (const card of cards) {
        applyRailExit(card.el, { exit: 0, shift: 0, hidden: false })
        card.key = ''
      }
    }

    const writeExit = () => {
      if (viewW < 32) return
      const fade = CONVERSATION_FADE_PX
      for (const card of cards) {
        const left = card.left - current
        const right = left + card.width
        if (railOverflowLeft(left, overflowLeft) || (left >= fade && right <= viewW)) {
          if (card.key === 'r') continue
          card.key = 'r'
          applyRailExit(card.el, { exit: 0, shift: 0, hidden: false })
          continue
        }
        const style = conversationExitStyle(left, right, 0, viewW, fade)
        const key = conversationExitKey(style)
        if (key === card.key) continue
        card.key = key
        applyRailExit(card.el, style)
      }
    }

    const writeTransform = () => {
      const max = maxScroll()
      track.style.transform = max > 0 ? `translate3d(${-current}px, 0, 0)` : ''
      track.dataset.brewRailScroll = String(Math.round(current))
    }

    const leadElAt = (scroll: number) => {
      const index = railLeadIndex(cards, scroll, viewW)
      return index >= 0 ? (cards[index]?.el ?? null) : null
    }

    const notifyLead = (el: HTMLElement | null) => {
      const notify = onLeadChangeRef.current
      const id = el?.dataset.railId ?? ''
      if (!id || id === lastLead) return
      lastLead = id
      const parsed = Number(id)
      if (Number.isFinite(parsed)) notify?.(parsed)
    }

    const reportLead = () => {
      if (seating && Math.abs(current - target) > 1) return
      notifyLead(leadElAt(current))
    }

    const scheduleExit = () => {
      if (exitFrame) return
      exitFrame = requestAnimationFrame(() => {
        exitFrame = 0
        writeExit()
      })
    }

    const write = () => {
      const pending = track.dataset.brewRailRestore
      if (pending != null && pending !== '') {
        const next = Number(pending)
        delete track.dataset.brewRailRestore
        if (Number.isFinite(next)) {
          current = next
          target = next
        }
      }
      writeTransform()
      writeExit()
      reportLead()
    }

    const clearSettleTimer = () => {
      if (!settleTimer) return
      window.clearTimeout(settleTimer)
      settleTimer = 0
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
      const max = maxScroll()

      if (!dragging) target = clampConversationScroll(target, max)
      current = reduce
        ? target
        : smoothToward(
            current,
            target,
            dt,
            railSettleTau(target - current, seating),
          )
      write()

      const arrived =
        !dragging &&
        Math.abs(target - current) <= (seating ? RAIL_SEAT_PX : 0.35)
      if (!arrived) {
        frame = requestAnimationFrame(tick)
        return
      }
      current = target
      if (!dragging && !restOnSlot(current)) {
        seating = true
        target = settleRailSlot(current, wheelVel, slotsOf(), max, home)
        home = target
        notifyLead(leadElAt(target))
        write()
        if (Math.abs(target - current) > 0.35 && !reduce) {
          frame = requestAnimationFrame(tick)
          return
        }
        current = target
      }
      seating = false
      write()
      stop()
    }

    const kick = () => {
      if (maxScroll() > 0) track.style.willChange = 'transform'
      if (!frame) frame = requestAnimationFrame(tick)
    }

    const snapTo = (scroll: number) => {
      clearSettleTimer()
      seating = true
      target = clampConversationScroll(scroll, maxScroll())
      home = slotAt(target)
      notifyLead(leadElAt(target))
      if (reduce) {
        current = target
        seating = false
        write()
        return
      }
      kick()
    }

    const align = (id: number, immediate = false) => {
      recache()
      measure()
      const index = cards.findIndex((item) => Number(item.el.dataset.railId) === id)
      if (index < 0) return
      lastLead = String(id)
      const x = clampConversationScroll(
        railSeatScroll(cards, index, overflowPx),
        maxScroll(),
      )
      if (immediate) {
        current = x
        target = x
        home = slotAt(x)
        seating = false
        writeTransform()
        writeExit()
        return
      }
      snapTo(x)
    }

    if (apiRef) apiRef.current = { align }

    const onWheel = (event: WheelEvent) => {
      if (event.ctrlKey) return
      if (!cards.length) recache()
      measure()
      const max = maxScroll()
      if (max <= 0) return
      event.preventDefault()
      const delta = wheelDelta(event)
      if (delta === 0) return
      dragging = false

      if (isDiscreteWheel(event)) {
        wheelAcc = 0
        wheelStarted = 0
        wheelVel = 0
        const dir = Math.sign(delta) || 1
        snapTo(neighborRailSlot(target, dir, slotsOf(), max))
        return
      }

      const now = event.timeStamp
      const slots = slotsOf()

      if (!wheelStarted) {
        wheelStarted = now
        home = slotAt(current)
        wheelAcc = 0
      }
      wheelAcc += delta
      wheelVel = wheelAcc / Math.max((now - wheelStarted) / 1000, 0.016)

      const pos = clampConversationScroll(home + wheelAcc, max)
      const committed = settleRailSlot(pos, 0, slots, max, home)

      const armIdle = () => {
        clearSettleTimer()
        settleTimer = window.setTimeout(() => {
          settleTimer = 0
          measure()
          const end = settleRailSlot(
            home + wheelAcc,
            wheelVel,
            slotsOf(),
            maxScroll(),
            home,
          )
          wheelAcc = 0
          wheelStarted = 0
          wheelVel = 0
          if (Math.abs(end - current) > 0.6 || !restOnSlot(current)) {
            snapTo(end)
          }
        }, RAIL_WHEEL_SETTLE_MS)
      }

      if (Math.abs(committed - home) > 0.6) {
        seating = true
        if (Math.abs(committed - target) > 0.6) {
          target = committed
          notifyLead(leadElAt(target))
        }
        armIdle()
        kick()
        return
      }

      seating = false
      target = pos
      current = pos
      armIdle()
      write()
    }

    const onTouchStart = (event: TouchEvent) => {
      if (event.touches.length !== 1) return
      dragging = true
      seating = false
      home = slotAt(target)
      clearSettleTimer()
      touchX = event.touches[0].clientX
      samples.length = 0
      samples.push({ t: event.timeStamp, x: target })
      last = 0
      track.style.willChange = 'transform'
    }

    const onTouchMove = (event: TouchEvent) => {
      if (!dragging || event.touches.length !== 1) return
      measure()
      const max = maxScroll()
      if (max <= 0) return
      const x = event.touches[0].clientX
      const dx = touchX - x
      touchX = x
      if (dx === 0) return
      event.preventDefault()
      target = rubberband(target + dx, max, viewW)
      current = target
      samples.push({ t: event.timeStamp, x: target })
      if (samples.length > 12) samples.shift()
      writeTransform()
      scheduleExit()
      reportLead()
    }

    const onTouchEnd = (event: TouchEvent) => {
      if (!dragging) return
      dragging = false
      recache()
      measure()
      const max = maxScroll()
      const flung = sampleVelocity(samples, event.timeStamp)
      samples.length = 0
      const raw = target
      target = clampConversationScroll(target, max)
      snapTo(settleRailSlot(raw, flung, slotsOf(), max, home))
    }

    recache()
    measure()
    const stored = Number(track.dataset.brewRailScroll)
    const seated = Number.isFinite(stored)
      ? stored
      : scrollFromTrackTransform(track.style.transform)
    current = seated
    target = seated
    home = slotAt(seated)
    const lead = leadElAt(seated)
    if (lead?.dataset.railId) lastLead = lead.dataset.railId
    writeTransform()
    writeExit()

    const resize = new ResizeObserver(() => {
      const nextW = track.scrollWidth
      if (nextW !== trackW || cards.length === 0) recache()
      measure()
      const max = maxScroll()
      if (dragging) return
      target = settleRailSlot(target, 0, slotsOf(), max)
      if (Math.abs(target - current) > 0.35) {
        seating = true
        kick()
        return
      }
      current = target
      write()
    })
    resize.observe(track)
    resize.observe(viewport)
    const restore = new MutationObserver(() => write())
    restore.observe(track, {
      attributes: true,
      attributeFilter: ['data-brew-rail-restore'],
    })

    const onVis = () => {
      if (document.hidden) {
        stop()
        return
      }
      if (dragging || Math.abs(target - current) > RAIL_SEAT_PX) kick()
    }
    document.addEventListener('visibilitychange', onVis)
    viewport.addEventListener('wheel', onWheel, { passive: false })
    viewport.addEventListener('touchstart', onTouchStart, { passive: true })
    viewport.addEventListener('touchmove', onTouchMove, { passive: false })
    viewport.addEventListener('touchend', onTouchEnd)
    viewport.addEventListener('touchcancel', onTouchEnd)

    return () => {
      clearSettleTimer()
      stop()
      restore.disconnect()
      resize.disconnect()
      document.removeEventListener('visibilitychange', onVis)
      viewport.removeEventListener('wheel', onWheel)
      viewport.removeEventListener('touchstart', onTouchStart)
      viewport.removeEventListener('touchmove', onTouchMove)
      viewport.removeEventListener('touchend', onTouchEnd)
      viewport.removeEventListener('touchcancel', onTouchEnd)
      clearExit()
      if (apiRef) apiRef.current = null
    }
  }, [apiRef, cardSelector, enabled, overflowLeft, resetKey, trackRef, viewportRef])
}

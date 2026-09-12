/** 选中卡与文章轨同一左缘；前一张从左边伸出，不退场藏掉。 */

import {
  clampConversationScroll,
  CONVERSATION_FADE_PX,
} from '../../agent-panel/conversationPan'

/** 比对话轨略慢，才能看见下一张进来。 */
export const RAIL_FOLLOW_TAU = 0.04

export const RAIL_SNAP_FAR_TAU = 0.046
export const RAIL_SNAP_MID_TAU = 0.06
export const RAIL_SNAP_NEAR_TAU = 0.072

/** 坐进槽位时提前咬死，去掉指数衰减的长尾巴。 */
export const RAIL_SEAT_PX = 2.6

export const RAIL_WHEEL_SETTLE_MS = 96

export const RAIL_FLING_LOOKAHEAD_S = 0.22
export const RAIL_FLING_SLOT_PX_S = 360

/** 推过槽距这么多，松手就进下一张，不弹回。 */
export const RAIL_COMMIT_RATIO = 0.28

/** 再进一格必须几乎走过下一张，避免轻滑连跳。 */
export const RAIL_NEXT_RATIO = 0.85

/** 甩的预估位移要超过这么多槽距，才允许跳第二张。 */
export const RAIL_MULTI_SPAN = 1.52

export const RAIL_OVERFLOW_LEFT_PX = 0

/** 左边越出的卡保持绘制。 */
export function railOverflowLeft(left: number, enabled: boolean): boolean {
  return enabled && left < CONVERSATION_FADE_PX
}

export function railLeadIndex(
  cards: ReadonlyArray<{ left: number; width: number }>,
  current: number,
  viewW: number,
  fade = CONVERSATION_FADE_PX,
): number {
  let fallback = -1
  let best = 0
  for (let i = 0; i < cards.length; i++) {
    const left = cards[i].left - current
    const visible =
      Math.min(left + cards[i].width, viewW) - Math.max(left, 0)
    if (visible <= 0.5) continue
    if (visible >= Math.min(fade, cards[i].width) - 0.5) return i
    if (visible > best) {
      best = visible
      fallback = i
    }
  }
  return fallback
}

export function railSeatScroll(
  cards: ReadonlyArray<{ left: number }>,
  focusIndex: number,
  overflowLeft = 0,
): number {
  if (cards.length === 0 || focusIndex < 0) return 0
  const origin = cards[0]?.left ?? 0
  const raw = Math.max(0, (cards[focusIndex]?.left ?? origin) - origin)
  if (focusIndex <= 0 || overflowLeft <= 0) return raw
  return Math.max(0, raw - overflowLeft)
}

export function railMaxScroll(slots: readonly number[], overflowLeft = 0): number {
  if (slots.length <= 1) return 0
  return Math.max(0, (slots.at(-1) ?? 0) - Math.max(0, overflowLeft))
}

/** 吸入槽位跟座定同一套：第一张贴左缘，其后每张让出溢出。 */
export function railSeatSlots(
  slots: readonly number[],
  overflowLeft = 0,
): number[] {
  if (slots.length === 0) return [0]
  if (overflowLeft <= 0) return Iterator.from(slots).toArray()
  return slots.map((slot, i) => (i === 0 ? slot : Math.max(0, slot - overflowLeft)))
}

export function railSlotOffsets(
  cards: ReadonlyArray<{ left: number }>,
): number[] {
  if (cards.length === 0) return [0]
  const origin = cards[0].left
  const slots: number[] = []
  for (const card of cards) {
    const left = card.left - origin
    if (!slots.length || Math.abs(slots.at(-1)! - left) > 0.5) {
      slots.push(left)
    }
  }
  return slots
}

export function railFloorIndex(
  offset: number,
  slots: readonly number[],
): number {
  let index = 0
  for (let i = 1; i < slots.length; i++) {
    if (slots[i] <= offset + 0.5) index = i
    else break
  }
  return index
}

export function nearestRailSlot(
  offset: number,
  slots: readonly number[],
  max: number,
): number {
  const x = clampConversationScroll(offset, max)
  let best = clampConversationScroll(slots[0] ?? 0, max)
  let bestDist = Math.abs(best - x)
  for (let i = 1; i < slots.length; i++) {
    const slot = clampConversationScroll(slots[i], max)
    const dist = Math.abs(slot - x)
    if (dist < bestDist - 0.01) {
      best = slot
      bestDist = dist
    }
  }
  return best
}

export function railNearestIndex(
  offset: number,
  slots: readonly number[],
): number {
  let index = 0
  let best = Infinity
  for (let i = 0; i < slots.length; i++) {
    const dist = Math.abs((slots[i] ?? 0) - offset)
    if (dist < best - 0.01) {
      best = dist
      index = i
    }
  }
  return index
}

export function neighborRailSlot(
  offset: number,
  direction: number,
  slots: readonly number[],
  max: number,
): number {
  if (slots.length === 0 || max <= 0) return 0
  if (direction === 0) return nearestRailSlot(offset, slots, max)
  const points = slots.map((slot) => clampConversationScroll(slot, max))
  const index = railNearestIndex(offset, points)
  if (direction > 0) {
    return points[Math.min(points.length - 1, index + 1)] ?? 0
  }
  return points[Math.max(0, index - 1)] ?? 0
}

export function settleRailSlot(
  offset: number,
  velocity: number,
  slots: readonly number[],
  max: number,
  home?: number,
): number {
  if (slots.length <= 1 || max <= 0) return 0
  const points = slots.map((slot) => clampConversationScroll(slot, max))
  const x = clampConversationScroll(offset, max)
  const predicted = clampConversationScroll(
    offset + velocity * RAIL_FLING_LOOKAHEAD_S,
    max,
  )
  const homeX =
    home === undefined
      ? nearestRailSlot(x, points, max)
      : nearestRailSlot(home, points, max)
  const index = railNearestIndex(homeX, points)
  const curr = points[index] ?? 0
  const prev = points[Math.max(0, index - 1)] ?? curr
  const next = points[Math.min(points.length - 1, index + 1)] ?? curr

  if (velocity > RAIL_FLING_SLOT_PX_S) {
    if (next === curr) return curr
    const span = next - curr
    if (predicted - curr < span * RAIL_MULTI_SPAN) return next
    return nearestRailSlot(predicted, points, max)
  }
  if (velocity < -RAIL_FLING_SLOT_PX_S) {
    if (prev === curr) return curr
    const span = curr - prev
    if (curr - predicted < span * RAIL_MULTI_SPAN) return prev
    return nearestRailSlot(predicted, points, max)
  }

  let i = index
  if (x >= curr) {
    while (i < points.length - 1) {
      const span = (points[i + 1] ?? 0) - (points[i] ?? 0)
      const need = i === index ? span * RAIL_COMMIT_RATIO : span * RAIL_NEXT_RATIO
      if (span <= 0.5 || x - (points[i] ?? 0) < need) break
      i += 1
    }
  } else {
    while (i > 0) {
      const span = (points[i] ?? 0) - (points[i - 1] ?? 0)
      const need = i === index ? span * RAIL_COMMIT_RATIO : span * RAIL_NEXT_RATIO
      if (span <= 0.5 || (points[i] ?? 0) - x < need) break
      i -= 1
    }
  }
  return points[i] ?? curr
}

export function railSettleTau(distance: number, seating: boolean): number {
  if (!seating) return RAIL_FOLLOW_TAU
  const abs = Math.abs(distance)
  if (abs > 160) return RAIL_SNAP_FAR_TAU
  if (abs > 56) return RAIL_SNAP_MID_TAU
  return RAIL_SNAP_NEAR_TAU
}

export function isDiscreteWheel(event: WheelEvent): boolean {
  return event.deltaMode !== 0
}

/** 收回全屏时接着座定，不要从 0 再吸一次。 */
export function scrollFromTrackTransform(transform: string): number {
  const match = /translate3d\(\s*(-?[\d.]+)px/i.exec(transform)
  if (!match) return 0
  const x = Number(match[1])
  if (!Number.isFinite(x)) return 0
  return Math.max(0, -x)
}

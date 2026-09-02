/**
 * 对话区滚动：位移跟手，卡片整张进整张出，不设 overflow、不裁容器、也不切遮罩。
 *
 * 轨道只做 transform。短卡片一越边就整颗一起淡；长卡片还在读的时候保持实心
 * 胶囊（顶上那截完整画出去），只剩最后一截才整张化开。
 */

export const CONVERSATION_NEAR_BOTTOM_PX = 8
export const CONVERSATION_LOAD_MORE_PX = 72
export const CONVERSATION_FADE_PX = 96
export const CONVERSATION_SHIFT_PX = 16

/** 惯性跟手时间常数（秒）。滚轮/拖拽直接贴目标，这条只给甩出去之后。 */
export const CONVERSATION_FOLLOW_TAU = 0.028

/** 甩出去之后的衰减时间常数（秒） */
export const CONVERSATION_FLING_TAU = 0.22

const SETTLE_PX = 0.35
const FLING_MIN_PX_S = 90

export function conversationMaxScroll(
  trackHeight: number,
  viewportHeight: number,
): number {
  return Math.max(0, Math.ceil(trackHeight - viewportHeight))
}

/**
 * 看得见的窗口高度：外壳上限扣掉输入行之后剩下的那截。
 * 不能用量轨道自己的高度 —— 那永远等于内容，滚动算成 0，卡片会当成全在视口里，铺到页面上。
 * 外壳还没量到时返回 0，宁可先不画退场，也不能把整列当成窗口。
 */
export function conversationViewHeight(
  trackHeight: number,
  availableHeight: number,
): number {
  if (trackHeight <= 0) return 0
  if (availableHeight <= 1) return 0
  return Math.min(trackHeight, availableHeight)
}

/** 锚点 max-height 解析成像素。getComputedStyle 会把 dvh 算成 px。 */
export function conversationShellLimit(
  maxHeightPx: number,
  viewportPx: number,
): number {
  if (maxHeightPx > 32) return maxHeightPx
  return Math.max(48, viewportPx - 32)
}

export function clampConversationScroll(scroll: number, max: number): number {
  if (max <= 0) return 0
  if (scroll < 0) return 0
  if (scroll > max) return max
  return scroll
}

export function wheelDeltaY(deltaY: number, deltaMode: number): number {
  if (deltaMode === 1) return deltaY * 16
  if (deltaMode === 2) return deltaY * 800
  return deltaY
}

export function smoothToward(
  current: number,
  target: number,
  dt: number,
  tau = CONVERSATION_FOLLOW_TAU,
): number {
  if (tau <= 0 || dt <= 0) return target
  if (Math.abs(target - current) <= SETTLE_PX) return target
  return current + (target - current) * (1 - Math.exp(-dt / tau))
}

export function decayVelocity(
  velocity: number,
  dt: number,
  tau = CONVERSATION_FLING_TAU,
): number {
  if (tau <= 0 || dt <= 0) return 0
  const next = velocity * Math.exp(-dt / tau)
  return Math.abs(next) < FLING_MIN_PX_S ? 0 : next
}

export function sampleVelocity(
  samples: ReadonlyArray<{ t: number; x: number }>,
  now: number,
  windowMs = 90,
): number {
  const from = now - windowMs
  let start = 0
  while (start < samples.length && samples[start].t < from) start += 1
  if (samples.length - start < 2) return 0
  const a = samples[start]
  const b = samples[samples.length - 1]
  const dt = (b.t - a.t) / 1000
  if (dt <= 0) return 0
  return (b.x - a.x) / dt
}

export function rubberband(offset: number, max: number, range: number): number {
  if (max <= 0) return 0
  const dim = Math.max(48, range * 0.42)
  if (offset < 0) return offset / (1 + Math.abs(offset) / dim)
  if (offset > max) {
    const extra = offset - max
    return max + extra / (1 + extra / dim)
  }
  return offset
}

export function stillCoasting(
  current: number,
  target: number,
  velocity: number,
  dragging: boolean,
): boolean {
  if (dragging) return true
  if (Math.abs(target - current) > SETTLE_PX) return true
  return Math.abs(velocity) >= FLING_MIN_PX_S
}

export interface ConversationExitStyle {
  exit: number
  /** 整卡平移，负值往上离开顶边，正值往下离开底边 */
  shift: number
  hidden: boolean
}

const REST: ConversationExitStyle = { exit: 0, shift: 0, hidden: false }

function leavingTop(
  messageTop: number,
  messageBottom: number,
  viewportTop: number,
  viewportBottom: number,
): boolean {
  const mid = (viewportTop + viewportBottom) / 2
  return (messageTop + messageBottom) / 2 <= mid
}

/**
 * 轨道坐标里一张气泡怎么退场。viewportTop 一般为 0。
 *
 * 不切遮罩：卡片始终是完整胶囊。看得见的高度还够，就整张实心留着
 * （越出视口的那截照样画）；只剩 `fade` 那么一截，才整张一起淡、移。
 */
export function conversationExitStyle(
  messageTop: number,
  messageBottom: number,
  viewportTop: number,
  viewportBottom: number,
  fade = CONVERSATION_FADE_PX,
): ConversationExitStyle {
  const height = messageBottom - messageTop
  if (height <= 0.5) return REST

  const visible =
    Math.min(messageBottom, viewportBottom) - Math.max(messageTop, viewportTop)

  const shiftFor = (top: boolean) =>
    top ? -CONVERSATION_SHIFT_PX : CONVERSATION_SHIFT_PX

  if (visible <= 0.5) {
    return {
      exit: 1,
      shift: shiftFor(
        leavingTop(messageTop, messageBottom, viewportTop, viewportBottom),
      ),
      hidden: true,
    }
  }

  const band = Math.min(Math.max(fade, 1), height)
  if (visible >= band - 0.5) return REST

  const topLost = Math.max(0, viewportTop - messageTop)
  const bottomLost = Math.max(0, messageBottom - viewportBottom)
  return {
    exit: Math.min(1, 1 - visible / band),
    shift: shiftFor(topLost >= bottomLost),
    hidden: false,
  }
}

export function conversationExitKey(style: ConversationExitStyle): string {
  if (style.hidden) return 'h'
  // 不能用空串：重测时 key 会清成 ''，再写成 rest 会被当成没变，
  // visibility:hidden 就再也清不掉，历史首次加载会丢卡片。
  if (style.exit <= 0.01) return 'r'
  return `${style.exit.toFixed(2)}:${style.shift}`
}

/**
 * 关面板时：已经越出 2/3 窗口的淡出要冻住。
 * 若先拉回实心胶囊再交给 CSS 一张张收，顶上那张半截卡会闪一整张。
 * 它仍参与错开，而且在最上面，所以是最后一张。
 */
export function conversationHoldExitOnClose(
  style: ConversationExitStyle,
): boolean {
  return style.hidden || style.exit > 0.01
}

export function applyConversationExit(
  el: HTMLElement,
  style: ConversationExitStyle,
): void {
  if (style.hidden) {
    delete el.dataset.leaving
    el.style.removeProperty('--exit')
    el.style.removeProperty('--exit-y')
    el.style.visibility = 'hidden'
    el.style.pointerEvents = 'none'
    return
  }

  el.style.visibility = ''
  if (style.exit <= 0.01) {
    delete el.dataset.leaving
    el.style.removeProperty('--exit')
    el.style.removeProperty('--exit-y')
    el.style.pointerEvents = ''
    return
  }

  el.dataset.leaving = 'true'
  el.style.setProperty('--exit', style.exit.toFixed(3))
  el.style.setProperty('--exit-y', `${style.shift}px`)
  el.style.pointerEvents = style.exit > 0.82 ? 'none' : ''
}

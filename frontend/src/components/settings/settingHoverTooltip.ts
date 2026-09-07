/**
 * 设置页 hover tooltip 定位（SettingTitleHelp / ToggleSwitch 共用）。
 * 无 React / DOM 写入，可单测。
 */

export const HOVER_TOOLTIP_VIEWPORT_PAD = 8
/** 触发器与浮层的间距；与 ⓘ 说明一致 */
export const HOVER_TOOLTIP_GAP = 6

export type HoverTooltipPlacement = 'top' | 'bottom'

export interface HoverTooltipCoords {
  top: number
  left: number
  placement: HoverTooltipPlacement
}

export interface HoverTooltipTriggerBox {
  top: number
  left: number
  bottom: number
  width: number
}

/**
 * 相对触发器水平居中，空间不足时翻转上下，并夹在视口内。
 */
export function computeHoverTooltipPosition(
  trigger: HoverTooltipTriggerBox,
  tipW: number,
  tipH: number,
  preferred: HoverTooltipPlacement,
  viewport: { width: number; height: number },
): HoverTooltipCoords {
  const { width: vw, height: vh } = viewport
  const pad = HOVER_TOOLTIP_VIEWPORT_PAD
  const gap = HOVER_TOOLTIP_GAP

  let placement: HoverTooltipPlacement = preferred
  const spaceBelow = vh - trigger.bottom - pad
  const spaceAbove = trigger.top - pad

  if (
    placement === 'bottom' &&
    tipH + gap > spaceBelow &&
    spaceAbove > spaceBelow
  ) {
    placement = 'top'
  } else if (
    placement === 'top' &&
    tipH + gap > spaceAbove &&
    spaceBelow >= spaceAbove
  ) {
    placement = 'bottom'
  }

  let left = trigger.left + trigger.width / 2 - tipW / 2
  left = Math.max(pad, Math.min(left, vw - tipW - pad))

  const top =
    placement === 'bottom'
      ? trigger.bottom + gap
      : trigger.top - gap - tipH

  const clampedTop = Math.max(pad, Math.min(top, vh - tipH - pad))

  return { top: clampedTop, left, placement }
}

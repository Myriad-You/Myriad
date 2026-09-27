export const GUIDE_VIEWPORT_PAD = 10
export const GUIDE_GAP = 8
/** min visible edge (px); below = hidden */
export const GUIDE_MIN_VISIBLE_EDGE = 10

export type GuidePlacement = 'top' | 'left' | 'right' | 'bottom'

export interface GuideCoords {
  top: number
  left: number
  placement: GuidePlacement
}

export function clamp(n: number, min: number, max: number): number {
  return Math.max(min, Math.min(n, max))
}

export function clampPanelToViewport(
  top: number,
  left: number,
  panelW: number,
  panelH: number,
  viewportW: number,
  viewportH: number,
  pad: number = GUIDE_VIEWPORT_PAD,
): { top: number; left: number } {
  return {
    left: clamp(
      left,
      pad,
      Math.max(pad, viewportW - panelW - pad),
    ),
    top: clamp(
      top,
      pad,
      Math.max(pad, viewportH - panelH - pad),
    ),
  }
}

export function applyGuideDragDelta(
  origTop: number,
  origLeft: number,
  deltaX: number,
  deltaY: number,
  panelW: number,
  panelH: number,
  viewportW: number,
  viewportH: number,
  pad: number = GUIDE_VIEWPORT_PAD,
): { top: number; left: number } {
  return clampPanelToViewport(
    origTop + deltaY,
    origLeft + deltaX,
    panelW,
    panelH,
    viewportW,
    viewportH,
    pad,
  )
}

export function shouldAllowGuideClose(
  pinned: boolean,
  force = false,
): boolean {
  if (pinned && !force) return false
  return true
}

export function shouldToggleCloseGuide(
  isOpen: boolean,
  pinned: boolean,
): boolean {
  if (!isOpen) return false
  if (pinned) return false
  return true
}

export interface GuideRect {
  top: number
  left: number
  right: number
  bottom: number
  width: number
  height: number
}

export function computeGuidePosition(
  trigger: GuideRect,
  panelW: number,
  panelH: number,
  viewportW: number,
  viewportH: number,
  pad: number = GUIDE_VIEWPORT_PAD,
  gap: number = GUIDE_GAP,
): GuideCoords {
  const spaceAbove = trigger.top - pad
  const spaceBelow = viewportH - trigger.bottom - pad
  const spaceLeft = trigger.left - pad
  const spaceRight = viewportW - trigger.right - pad

  const needH = panelH + gap
  const needW = panelW + gap

  let placement: GuidePlacement
  if (needH <= spaceAbove) {
    placement = 'top'
  } else if (needW <= spaceLeft) {
    placement = 'left'
  } else if (needW <= spaceRight) {
    placement = 'right'
  } else if (needH <= spaceBelow) {
    placement = 'bottom'
  } else {
    const scores: Array<{ p: GuidePlacement; s: number }> = [
      { p: 'top', s: spaceAbove },
      { p: 'left', s: spaceLeft },
      { p: 'right', s: spaceRight },
      { p: 'bottom', s: spaceBelow },
    ]
    placement = scores.toSorted((a, b) => b.s - a.s)[0]!.p
  }

  let top = 0
  let left = 0

  switch (placement) {
    case 'top':
      top = trigger.top - gap - panelH
      left = trigger.left
      break
    case 'left':
      top = trigger.top
      left = trigger.left - gap - panelW
      break
    case 'right':
      top = trigger.top
      left = trigger.right + gap
      break
    case 'bottom':
      top = trigger.bottom + gap
      left = trigger.left
      break
  }

  left = clamp(left, pad, viewportW - panelW - pad)
  top = clamp(top, pad, viewportH - panelH - pad)

  return { top, left, placement }
}

export function isGuideAnchorVisible(
  rect: GuideRect,
  viewportW: number,
  viewportH: number,
  minEdge: number = GUIDE_MIN_VISIBLE_EDGE,
): boolean {
  const visibleH = Math.min(rect.bottom, viewportH) - Math.max(rect.top, 0)
  const visibleW = Math.min(rect.right, viewportW) - Math.max(rect.left, 0)
  return visibleH >= minEdge && visibleW >= minEdge
}

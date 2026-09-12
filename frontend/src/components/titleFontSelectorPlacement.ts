export const STYLE_PANEL_WIDTH = 320
export const STYLE_PANEL_FALLBACK_HEIGHT = 280
export const STYLE_PANEL_GAP = 8
export const STYLE_PANEL_MARGIN = 8

export type StylePanelPlacement = 'above' | 'below'

export interface StylePanelBox {
  top: number
  left: number
  right: number
  bottom: number
}

export interface StylePanelPosition {
  top: number
  left: number
  placement: StylePanelPlacement
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(min, value), Math.max(min, max))
}

function readViewport(
  fallback: { width: number; height: number },
  viewport?: { width: number; height: number },
): { width: number; height: number } {
  if (viewport) return viewport
  if (typeof window === 'undefined') return fallback
  return { width: window.innerWidth, height: window.innerHeight }
}

export function placeStylePanel(
  button: StylePanelBox,
  panelWidth: number,
  panelHeight: number,
  viewport?: { width: number; height: number },
): StylePanelPosition {
  const { width: vw, height: vh } = readViewport(
    { width: panelWidth, height: panelHeight },
    viewport,
  )
  const spaceBelow = vh - button.bottom - STYLE_PANEL_GAP - STYLE_PANEL_MARGIN
  const spaceAbove = button.top - STYLE_PANEL_GAP - STYLE_PANEL_MARGIN
  const placement: StylePanelPlacement =
    spaceBelow >= panelHeight || spaceBelow >= spaceAbove ? 'below' : 'above'

  let left = button.left
  if (left + panelWidth + STYLE_PANEL_MARGIN > vw) {
    left = button.right - panelWidth
  }
  left = clamp(left, STYLE_PANEL_MARGIN, vw - panelWidth - STYLE_PANEL_MARGIN)

  const top = clamp(
    placement === 'above'
      ? button.top - STYLE_PANEL_GAP - panelHeight
      : button.bottom + STYLE_PANEL_GAP,
    STYLE_PANEL_MARGIN,
    vh - panelHeight - STYLE_PANEL_MARGIN,
  )

  return { top, left, placement }
}

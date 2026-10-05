/** 悬浮框锚定的那段文字：水平中心与上下边，视口坐标。 */
export interface ReaderTooltipAnchor {
  x: number
  top: number
  bottom: number
}

export interface ReaderTooltipPlacement {
  left: number
  top: number
  side: 'above' | 'below'
  /** 箭头相对框左边的位置，始终指着锚点。 */
  arrowX: number
}

const ARROW_INSET = 12

/**
 * 框按实际尺寸整块放进视口：水平以锚点居中再夹到边距内；
 * 默认放在锚点上方，上面放不下就翻到下方，两边都放不下取空间大的一边。
 */
export function placeReaderTooltip(
  anchor: ReaderTooltipAnchor,
  size: { width: number; height: number },
  viewport: { width: number; height: number },
  gap: number,
  margin = 16,
): ReaderTooltipPlacement {
  const maxLeft = Math.max(margin, viewport.width - margin - size.width)
  const left = Math.min(maxLeft, Math.max(margin, anchor.x - size.width / 2))

  const aboveTop = anchor.top - gap - size.height
  const belowTop = anchor.bottom + gap
  const fitsAbove = aboveTop >= margin
  const fitsBelow = belowTop + size.height <= viewport.height - margin
  const roomAbove = anchor.top - margin
  const roomBelow = viewport.height - margin - anchor.bottom
  const side =
    fitsAbove || (!fitsBelow && roomAbove >= roomBelow) ? 'above' : 'below'

  const maxTop = Math.max(margin, viewport.height - margin - size.height)
  const top = Math.min(
    maxTop,
    Math.max(margin, side === 'above' ? aboveTop : belowTop),
  )
  const arrowX = Math.min(
    Math.max(ARROW_INSET, anchor.x - left),
    Math.max(ARROW_INSET, size.width - ARROW_INSET),
  )
  return { left, top, side, arrowX }
}

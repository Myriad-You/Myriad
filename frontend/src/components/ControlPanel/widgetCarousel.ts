export interface WidgetCarouselGate {
  isEditMode: boolean
  maxPage: number
  // 收起后仍挂载（content-visibility:hidden）；不接 panelVisible 的话轮播会在看不见时继续翻页。
  panelVisible: boolean
  // 触屏不要置 isHovering：mouseenter 会粘滞，轮播会停到点到区域外。
  isHovering: boolean
}

export function isHoverCapablePointer(pointerType: string): boolean {
  return pointerType === 'mouse'
}

// 只收窄不放宽：任一项不满足都停。
export function shouldAutoAdvanceWidgets(gate: WidgetCarouselGate): boolean {
  return (
    !gate.isEditMode &&
    gate.maxPage > 0 &&
    gate.panelVisible &&
    !gate.isHovering
  )
}

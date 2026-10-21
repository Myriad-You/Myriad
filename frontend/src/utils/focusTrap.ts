const FOCUSABLE = [
  'a[href]',
  'area[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  'iframe',
  '[contenteditable="true"]',
  '[tabindex]:not([tabindex="-1"])',
].join(',')

export function focusableWithin(root: HTMLElement): HTMLElement[] {
  return Array.from(root.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (element) =>
      !element.hasAttribute('data-focus-sentinel') &&
      !element.closest('[inert]') &&
      element.getClientRects().length > 0,
  )
}

/** Tab / Shift+Tab 在 root 里循环。返回 true 表示已接管这次按键。 */
export function trapTabKey(root: HTMLElement, event: KeyboardEvent): boolean {
  if (event.key !== 'Tab') return false
  const items = focusableWithin(root)
  if (items.length === 0) {
    event.preventDefault()
    root.focus()
    return true
  }
  const first = items[0]
  const last = items[items.length - 1]
  const current = document.activeElement
  if (event.shiftKey && (current === first || current === root)) {
    event.preventDefault()
    last.focus()
    return true
  }
  if (!event.shiftKey && (current === last || current === root)) {
    event.preventDefault()
    first.focus()
    return true
  }
  if (!(current instanceof Node) || !root.contains(current)) {
    event.preventDefault()
    first.focus()
    return true
  }
  return false
}

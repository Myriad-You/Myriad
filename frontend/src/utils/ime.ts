import type { KeyboardEvent as ReactKeyboardEvent } from 'react'

/**
 * Detect IME composition (CJK candidate confirmation, etc.).
 * Enter during composition must not trigger send/submit.
 */
export function isImeComposing(
  event: ReactKeyboardEvent | KeyboardEvent,
): boolean {
  if ('nativeEvent' in event) {
    const ne = event.nativeEvent
    if (ne.isComposing) return true
    // keyCode 229 = composition in progress (Safari / older browsers)
    if (ne.keyCode === 229) return true
    return false
  }
  if (event.isComposing) return true
  if (event.keyCode === 229) return true
  return false
}

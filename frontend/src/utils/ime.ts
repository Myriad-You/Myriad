import type { KeyboardEvent as ReactKeyboardEvent } from 'react'

/** Enter during composition must not submit. */
export function isImeComposing(
  event: ReactKeyboardEvent | KeyboardEvent,
): boolean {
  if ('nativeEvent' in event) {
    const ne = event.nativeEvent
    if (ne.isComposing) return true
    if (ne.keyCode === 229) return true
    return false
  }
  if (event.isComposing) return true
  if (event.keyCode === 229) return true
  return false
}

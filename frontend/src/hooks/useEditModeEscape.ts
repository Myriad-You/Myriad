import { useEffect, useRef } from 'react'

/** Exit edit on Esc. Dock Stage Manager consumes Esc first (capture). */
export function useEditModeEscape(enabled: boolean, onExit: () => void): void {
  const onExitRef = useRef(onExit)
  onExitRef.current = onExit

  useEffect(() => {
    if (!enabled) return
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== 'Escape' || event.defaultPrevented) return
      onExitRef.current()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [enabled])
}

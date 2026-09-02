/**
 * Immersive nav + Escape-to-exit for Tapp fixed shells.
 * Escape is optional (store had it; run did not — keep parity via enableEscape).
 */

import { useEffect } from 'react'
import { useImmersiveChrome } from '../../contexts/NavigationContext'

export function useTappFullscreenChrome(
  isFullscreen: boolean,
  setIsFullscreen: (value: boolean | ((prev: boolean) => boolean)) => void,
  options?: { enableEscape?: boolean },
): void {
  const enableEscape = options?.enableEscape ?? false

  useImmersiveChrome('tapp-fullscreen', isFullscreen)

  useEffect(() => {
    if (!enableEscape || !isFullscreen) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        setIsFullscreen(false)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [enableEscape, isFullscreen, setIsFullscreen])
}

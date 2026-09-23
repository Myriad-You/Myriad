import { useEffect } from 'react'

import { useWidgetTheme } from '../hooks/useWidgetTheme'
import { LENS_VIEWPORT_QUERY } from '../utils/liquidGlass/lensViewport'

/**
 * The lens engine (~35KB) can only run at desktop widths, so it is fetched
 * once the viewport qualifies — never on phones — and mounted from there.
 */
function useSurfaceLenses(): void {
  useEffect(() => {
    const viewport = matchMedia(LENS_VIEWPORT_QUERY)
    let cancelled = false
    let dispose: (() => void) | undefined
    const start = () => {
      if (!viewport.matches) return
      viewport.removeEventListener('change', start)
      void import('../utils/liquidGlass/surfaceLenses').then(({ mountSurfaceLenses }) => {
        if (!cancelled) dispose = mountSurfaceLenses()
      })
    }
    viewport.addEventListener('change', start)
    start()
    return () => {
      cancelled = true
      viewport.removeEventListener('change', start)
      dispose?.()
    }
  }, [])
}

export function SurfaceThemeApplier() {
  useWidgetTheme()
  useSurfaceLenses()
  return null
}

export default SurfaceThemeApplier

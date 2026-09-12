import { useEffect } from 'react'

import { useWidgetTheme } from '../hooks/useWidgetTheme'
import { mountSurfaceLenses } from '../utils/liquidGlass/surfaceLenses'

export function SurfaceThemeApplier() {
  useWidgetTheme()
  useEffect(() => mountSurfaceLenses(), [])
  return null
}

export default SurfaceThemeApplier

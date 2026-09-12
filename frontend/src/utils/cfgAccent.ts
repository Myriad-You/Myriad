/** Use var(--cfg-accent), not --color-primary. */

import { deriveAdaptiveTitleColor } from './readableColor'
import { subscribeToTheme } from './themeSubscriber'

const CSS_VAR = '--cfg-accent'

export function syncCfgAccentColor(): void {
  if (typeof document === 'undefined') return
  const isDark = document.documentElement.classList.contains('dark')
  const color = deriveAdaptiveTitleColor(isDark)
  document.documentElement.style.setProperty(CSS_VAR, color)
}

let started = false

export function ensureCfgAccentSync(): void {
  if (started || typeof document === 'undefined') return
  started = true
  syncCfgAccentColor()
  subscribeToTheme(() => {
    syncCfgAccentColor()
  })
}

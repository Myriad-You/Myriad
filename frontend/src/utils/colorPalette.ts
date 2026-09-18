import { syncCfgAccentColor } from './cfgAccent'

export interface ColorPalette {
  primary: string
  secondary: string
  accent: string
  light: string
  dark: string
}

export const DEFAULT_PALETTE: ColorPalette = Object.freeze({
  primary: '#6b7280',
  secondary: '#9ca3af',
  accent: '#4b5563',
  light: '#d1d5db',
  dark: '#374151',
})

export function isDefaultPalette(
  palette: ColorPalette | null | undefined,
): boolean {
  if (!palette) return true
  return (
    palette.primary === DEFAULT_PALETTE.primary &&
    palette.secondary === DEFAULT_PALETTE.secondary &&
    palette.accent === DEFAULT_PALETTE.accent &&
    palette.light === DEFAULT_PALETTE.light &&
    palette.dark === DEFAULT_PALETTE.dark
  )
}

export function applyColorPalette(palette: ColorPalette): void {
  const root = document.documentElement
  root.style.setProperty('--color-primary', palette.primary)
  root.style.setProperty('--color-secondary', palette.secondary)
  root.style.setProperty('--color-accent', palette.accent)
  root.style.setProperty('--color-light', palette.light)
  root.style.setProperty('--color-dark', palette.dark)
  syncCfgAccentColor()
}

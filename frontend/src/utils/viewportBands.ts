export type ViewportBand = 'phone' | 'tablet' | 'desktop'

/** Phone max; Tailwind md = min 768. */
export const VIEWPORT_PHONE_MAX = 767
export const VIEWPORT_TABLET_MIN = 768
export const VIEWPORT_TABLET_MAX = 1077
export const VIEWPORT_DESKTOP_MIN = 1078

export const HOME_GRID_COLS_PHONE = 4
export const HOME_GRID_COLS_TABLET = 8
export const HOME_GRID_COLS_DESKTOP = 16

/** Hard cut; no dead zone. */
export function resolveViewportBand(width: number): ViewportBand {
  if (!Number.isFinite(width) || width <= 0) return 'desktop'
  if (width <= VIEWPORT_PHONE_MAX) return 'phone'
  if (width <= VIEWPORT_TABLET_MAX) return 'tablet'
  return 'desktop'
}

export function homeGridColsForBand(band: ViewportBand): number {
  switch (band) {
    case 'phone':
      return HOME_GRID_COLS_PHONE
    case 'tablet':
      return HOME_GRID_COLS_TABLET
    default:
      return HOME_GRID_COLS_DESKTOP
  }
}

/** Hard cut; no hysteresis. */
export function resolveHomeGridColumns(
  width: number,
  previous: number = HOME_GRID_COLS_DESKTOP,
): number {
  if (!Number.isFinite(width) || width <= 0) {
    return previous === HOME_GRID_COLS_PHONE ||
      previous === HOME_GRID_COLS_TABLET ||
      previous === HOME_GRID_COLS_DESKTOP
      ? previous
      : HOME_GRID_COLS_DESKTOP
  }
  return homeGridColsForBand(resolveViewportBand(width))
}

export const VIEWPORT_MQ = {
  phone: `(max-width: ${VIEWPORT_PHONE_MAX}px)`,
  tablet: `(min-width: ${VIEWPORT_TABLET_MIN}px) and (max-width: ${VIEWPORT_TABLET_MAX}px)`,
  desktop: `(min-width: ${VIEWPORT_DESKTOP_MIN}px)`,
  notDesktop: `(max-width: ${VIEWPORT_TABLET_MAX}px)`,
  notPhone: `(min-width: ${VIEWPORT_TABLET_MIN}px)`,
} as const

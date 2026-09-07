/**
 * Dock library Stage Manager park (left of the grid).
 * Island box size is owned here (`libraryDockIslandBoxStyle`); CSS only paints.
 * Home edit hides the nav island (`useImmersiveChrome('home-edit-mode')`),
 * so the thumbnail sits on a left inset instead of clearing the rail.
 * Scene perspective is applied from `LIBRARY_DOCK_STAGE_PERSPECTIVE`.
 */

export const LIBRARY_DOCK_WIDTH_VW = 94
export const LIBRARY_DOCK_HEIGHT_VH = 70
export const LIBRARY_DOCK_MAX_WIDTH_REM = 64
export const LIBRARY_DOCK_MAX_HEIGHT_REM = 40
export const LIBRARY_DOCK_SIZE_FACTOR = 5 / 6
export const LIBRARY_DOCK_BOTTOM_REM = 5.25

/** Left inset while parked. Nav rail is hidden in edit mode. */
export const LIBRARY_DOCK_STAGE_INSET_REM = 1.5

/** Matches `.global-control-bar { top; right }`. */
export const CONTROL_PANEL_INSET_REM = 1
/** Matches `@media (width <= 640px) { .global-control-bar { top; right } }`. */
export const CONTROL_PANEL_MOBILE_MAX_PX = 640
export const CONTROL_PANEL_MOBILE_INSET_REM = 0.75
/** Matches `.control-bar-trigger.expanded { width }`. */
export const CONTROL_PANEL_EXPANDED_WIDTH_PX = 400
/** Matches `.control-bar-trigger.expanded { border-radius }`. */
export const CONTROL_PANEL_EXPANDED_RADIUS_REM = 1.5
/** Matches `GlobalControlPanel` height: `ceil(scrollHeight * 1.08)`. */
export const CONTROL_PANEL_HEIGHT_COMPENSATION = 1.08
/** Matches `.control-bar-trigger` collapsed chrome. */
export const CONTROL_PANEL_COLLAPSED_WIDTH_PX = 160
export const CONTROL_PANEL_COLLAPSED_HEIGHT_REM = 3
export const CONTROL_PANEL_COLLAPSED_RADIUS_REM = 2
/** Gap between the unparkable catalog and the expanded control panel. */
export const LIBRARY_BESIDE_PANEL_GAP_REM = 1
export const LIBRARY_BESIDE_PANEL_ANCHOR =
  '.global-control-bar .control-bar-trigger'

export const LIBRARY_DOCK_STAGE_SCALE = 0.3
export const LIBRARY_DOCK_STAGE_SCALE_HOVER = 0.33
/**
 * CSS +Y: clockwise from above, so the right edge comes toward the camera.
 * Keep this a single-axis tilt — extra X rotation reads as warped.
 */
export const LIBRARY_DOCK_STAGE_ROTATE_Y = 74
export const LIBRARY_DOCK_STAGE_ROTATE_X = 0
export const LIBRARY_DOCK_STAGE_BADGE_HANG_PX = 6
export const LIBRARY_DOCK_STAGE_PERSPECTIVE = 1800
/** Framer `originX` / `originY`: shrink and hinge from the left edge. */
export const LIBRARY_DOCK_STAGE_ORIGIN_X = 0
export const LIBRARY_DOCK_STAGE_ORIGIN_Y = 0.5

/**
 * Clicks on these roots must not park/restore the dock.
 * Home-owned chrome uses the data attr; nav / control bar are platform chrome.
 */
export const LIBRARY_DOCK_CHROME_ATTR = 'data-library-dock-chrome'
export const LIBRARY_DOCK_POINTER_CHROME = `[${LIBRARY_DOCK_CHROME_ATTR}], [data-sticker-pick], .nav-container, .global-control-bar, .tour-overlay`
export const LIBRARY_DOCK_RESTORE_BLOCK =
  '.widget-grid-item, button, input, select, textarea, a, [role="button"]'

export const LIBRARY_DOCK_PARK_EASE = [0.22, 1, 0.36, 1] as const
export const LIBRARY_DOCK_PARK_DURATION_S = 0.56
export const LIBRARY_DOCK_RESTORE_SUPPRESS_MS =
  LIBRARY_DOCK_PARK_DURATION_S * 1000 + 90
export const LIBRARY_DOCK_PARK_TRANSITION = {
  x: { duration: LIBRARY_DOCK_PARK_DURATION_S, ease: LIBRARY_DOCK_PARK_EASE },
  y: { duration: LIBRARY_DOCK_PARK_DURATION_S, ease: LIBRARY_DOCK_PARK_EASE },
  scale: { duration: LIBRARY_DOCK_PARK_DURATION_S, ease: LIBRARY_DOCK_PARK_EASE },
  rotateX: {
    duration: LIBRARY_DOCK_PARK_DURATION_S,
    ease: LIBRARY_DOCK_PARK_EASE,
  },
  rotateY: {
    duration: LIBRARY_DOCK_PARK_DURATION_S,
    ease: LIBRARY_DOCK_PARK_EASE,
  },
  z: { duration: LIBRARY_DOCK_PARK_DURATION_S, ease: LIBRARY_DOCK_PARK_EASE },
  opacity: { duration: 0.26, ease: [0.25, 0.1, 0.25, 1] },
} as const

/**
 * Motion’s `transformTemplate` values are already unit-suffixed
 * (`-100px`, `36deg`). Re-adding units makes the whole transform invalid.
 */
function asCss(
  value: number | string | undefined,
  unit: string,
  fallback: string,
): string {
  if (value === undefined || value === '') return fallback
  if (typeof value === 'number') return `${value}${unit}`
  if (unit && !value.endsWith(unit) && /^-?\d+(\.\d+)?$/.test(value)) {
    return `${value}${unit}`
  }
  return value
}

/**
 * CSS applies the transform list right-to-left. Framer’s default string is
 * `scale() rotateY()`, which rotates the full-size window and then shrinks
 * the projected result — that is the stretch. Put rotate left of scale so
 * the box is scaled first, then tilted. Parent `.widget-library-stage-scene`
 * already supplies perspective; do not add `perspective()` here.
 */
export function libraryDockStageTransform(latest: {
  x?: number | string
  y?: number | string
  z?: number | string
  scale?: number | string
  rotateY?: number | string
  rotateX?: number | string
}): string {
  const x = asCss(latest.x, 'px', '0px')
  const y = asCss(latest.y, 'px', '0px')
  const z = asCss(latest.z, 'px', '0px')
  const scale = latest.scale === undefined || latest.scale === '' ? 1 : latest.scale
  const rotateY = asCss(latest.rotateY, 'deg', '0deg')
  const rotateX = asCss(latest.rotateX, 'deg', '0deg')
  return `translate3d(${x}, ${y}, ${z}) rotateY(${rotateY}) rotateX(${rotateX}) scale(${scale})`
}

export function libraryDockIslandSize(
  viewportWidth: number,
  viewportHeight: number,
  rootFontSize = 16,
): { width: number; height: number } {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  return {
    width: Math.min(
      (LIBRARY_DOCK_WIDTH_VW / 100) * viewportWidth * LIBRARY_DOCK_SIZE_FACTOR,
      LIBRARY_DOCK_MAX_WIDTH_REM * rem * LIBRARY_DOCK_SIZE_FACTOR,
    ),
    height: Math.min(
      (LIBRARY_DOCK_HEIGHT_VH / 100) *
        viewportHeight *
        LIBRARY_DOCK_SIZE_FACTOR,
      LIBRARY_DOCK_MAX_HEIGHT_REM * rem * LIBRARY_DOCK_SIZE_FACTOR,
    ),
  }
}

export function fallbackControlPanelEdge(
  viewportWidth: number,
  rootFontSize = 16,
): { top: number; left: number } {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  const inset = CONTROL_PANEL_INSET_REM * rem
  return {
    top: inset,
    left: viewportWidth - inset - CONTROL_PANEL_EXPANDED_WIDTH_PX,
  }
}

export function compensatedControlPanelHeight(contentHeight: number): number {
  return Math.max(
    0,
    Math.ceil(contentHeight * CONTROL_PANEL_HEIGHT_COMPENSATION),
  )
}

export function controlPanelChromeInset(
  viewportWidth: number,
  rootFontSize = 16,
): number {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  const mobile = viewportWidth <= CONTROL_PANEL_MOBILE_MAX_PX
  return (mobile ? CONTROL_PANEL_MOBILE_INSET_REM : CONTROL_PANEL_INSET_REM) * rem
}

/** 展开终态外壳：右上 inset + 宽 400 / 窄屏铺满，不读 morph 中的 width。 */
export function expandedControlPanelMetrics(
  viewportWidth: number,
  rootFontSize = 16,
): { top: number; left: number; width: number } {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  const inset = controlPanelChromeInset(viewportWidth, rem)
  const width =
    viewportWidth <= CONTROL_PANEL_MOBILE_MAX_PX
      ? Math.max(0, viewportWidth - inset * 2)
      : CONTROL_PANEL_EXPANDED_WIDTH_PX
  return {
    top: inset,
    left: viewportWidth - inset - width,
    width,
  }
}

export function predictExpandedControlPanelBox(
  viewportWidth: number,
  contentHeight: number,
  rootFontSize = 16,
): { top: number; left: number; width: number; height: number } {
  return {
    ...expandedControlPanelMetrics(viewportWidth, rootFontSize),
    height: compensatedControlPanelHeight(contentHeight),
  }
}

/** 收缩终态控制岛：右上 inset + 160×3rem，不读收起 morph 中的尺寸。 */
export function predictCollapsedControlPanelBox(
  viewportWidth: number,
  rootFontSize = 16,
): { top: number; left: number; width: number; height: number } {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  const inset = controlPanelChromeInset(viewportWidth, rem)
  const width = CONTROL_PANEL_COLLAPSED_WIDTH_PX
  return {
    top: inset,
    left: viewportWidth - inset - width,
    width,
    height: CONTROL_PANEL_COLLAPSED_HEIGHT_REM * rem,
  }
}

/**
 * Unparkable catalog: sit in the slot left of the expanded control panel.
 * Home dock size is the preferred cap; the slot may shrink it.
 */
export function libraryBesidePanelBox(input: {
  preferred: { width: number; height: number }
  viewportWidth: number
  viewportHeight: number
  panel: { top: number; left: number } | null
  rootFontSize?: number
}): {
  width: number
  height: number
  top: number
  left: number
  bottom: 'auto'
  transformOrigin: string
} {
  const rem =
    input.rootFontSize && input.rootFontSize > 0 ? input.rootFontSize : 16
  const inset = LIBRARY_DOCK_STAGE_INSET_REM * rem
  const gap = LIBRARY_BESIDE_PANEL_GAP_REM * rem
  const panel =
    input.panel ?? fallbackControlPanelEdge(input.viewportWidth, rem)
  const availableRight = panel.left - gap
  const availableWidth = Math.max(0, availableRight - inset)
  const top = Math.max(0, panel.top)
  const availableHeight = Math.max(0, input.viewportHeight - gap - top)
  const width = Math.min(input.preferred.width, availableWidth)
  const height = Math.min(input.preferred.height, availableHeight)
  return {
    width,
    height,
    top,
    left: availableRight - width,
    bottom: 'auto',
    transformOrigin: `${LIBRARY_DOCK_STAGE_ORIGIN_X * 100}% ${LIBRARY_DOCK_STAGE_ORIGIN_Y * 100}%`,
  }
}

/** Inline box for `.widget-library-island` — CSS does not repeat these numbers. */
export function libraryDockIslandBoxStyle(
  island: { width: number; height: number },
  rootFontSize = 16,
): {
  width: number
  height: number
  bottom: number
  left: string
  transformOrigin: string
} {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  return {
    width: island.width,
    height: island.height,
    bottom: LIBRARY_DOCK_BOTTOM_REM * rem,
    left: '50%',
    transformOrigin: `${LIBRARY_DOCK_STAGE_ORIGIN_X * 100}% ${LIBRARY_DOCK_STAGE_ORIGIN_Y * 100}%`,
  }
}

/** Rest pose: centered, `left: 50%` + `translateX(-width/2)`, `bottom: 5.25rem`. */
export function predictRestoredLibraryDockBox(
  viewportWidth: number,
  viewportHeight: number,
  rootFontSize = 16,
): { top: number; left: number; width: number; height: number } {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  const island = libraryDockIslandSize(viewportWidth, viewportHeight, rem)
  const bottom = LIBRARY_DOCK_BOTTOM_REM * rem
  return {
    width: island.width,
    height: island.height,
    left: viewportWidth / 2 - island.width / 2,
    top: viewportHeight - bottom - island.height,
  }
}

/** Left edge of the parked thumbnail. */
export function libraryDockStageLeft(rootFontSize = 16): number {
  const rem = rootFontSize > 0 ? rootFontSize : 16
  return LIBRARY_DOCK_STAGE_INSET_REM * rem
}

export function libraryDockStageVisualSize(input: {
  islandWidth: number
  islandHeight: number
  scale?: number
  rotateYDeg?: number
  rotateXDeg?: number
}): { width: number; height: number } {
  const scale = input.scale ?? LIBRARY_DOCK_STAGE_SCALE
  const rotateY =
    ((input.rotateYDeg ?? LIBRARY_DOCK_STAGE_ROTATE_Y) * Math.PI) / 180
  const rotateX =
    ((input.rotateXDeg ?? LIBRARY_DOCK_STAGE_ROTATE_X) * Math.PI) / 180
  return {
    width: input.islandWidth * scale * Math.abs(Math.cos(rotateY)),
    height: input.islandHeight * scale * Math.abs(Math.cos(rotateX)),
  }
}

/**
 * Motion `x` / `y` on the dock island (CSS `left: 50%; bottom: 5.25rem`).
 * Unstaged rest `x` is `-width / 2`. Staged `x` parks the unscaled left
 * edge on the left inset. Scale/rotate hinge from originX=0 so the
 * window collapses into that left edge instead of shrinking from center.
 */
export function libraryDockStageOffset(input: {
  viewportWidth: number
  viewportHeight: number
  islandWidth: number
  islandHeight: number
  dockBottom: number
  rootFontSize?: number
}): { x: number; y: number } {
  const left = libraryDockStageLeft(input.rootFontSize)
  const vw = input.viewportWidth
  const vh = input.viewportHeight
  const height = input.islandHeight
  return {
    x: left - vw / 2,
    y: -vh / 2 + height / 2 + input.dockBottom,
  }
}

/** Flat app icon at the visual lower-left corner, facing the camera. */
export function libraryDockStageBadgePos(input: {
  islandHeight: number
  scale?: number
  rotateXDeg?: number
  rootFontSize?: number
  hang?: number
}): { left: number; topOffset: number } {
  const visual = libraryDockStageVisualSize({
    islandWidth: 1,
    islandHeight: input.islandHeight,
    scale: input.scale,
    rotateXDeg: input.rotateXDeg,
  })
  return {
    left:
      libraryDockStageLeft(input.rootFontSize) -
      (input.hang ?? LIBRARY_DOCK_STAGE_BADGE_HANG_PX),
    topOffset: visual.height * 0.46,
  }
}

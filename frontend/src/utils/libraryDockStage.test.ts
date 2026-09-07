import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import {
  CONTROL_PANEL_COLLAPSED_HEIGHT_REM,
  CONTROL_PANEL_COLLAPSED_RADIUS_REM,
  CONTROL_PANEL_COLLAPSED_WIDTH_PX,
  CONTROL_PANEL_EXPANDED_RADIUS_REM,
  CONTROL_PANEL_EXPANDED_WIDTH_PX,
  CONTROL_PANEL_HEIGHT_COMPENSATION,
  CONTROL_PANEL_INSET_REM,
  CONTROL_PANEL_MOBILE_INSET_REM,
  CONTROL_PANEL_MOBILE_MAX_PX,
  fallbackControlPanelEdge,
  predictCollapsedControlPanelBox,
  predictExpandedControlPanelBox,
  LIBRARY_BESIDE_PANEL_GAP_REM,
  LIBRARY_DOCK_BOTTOM_REM,
  LIBRARY_DOCK_CHROME_ATTR,
  LIBRARY_DOCK_PARK_DURATION_S,
  LIBRARY_DOCK_POINTER_CHROME,
  LIBRARY_DOCK_RESTORE_SUPPRESS_MS,
  LIBRARY_DOCK_STAGE_INSET_REM,
  LIBRARY_DOCK_STAGE_ORIGIN_X,
  LIBRARY_DOCK_STAGE_ORIGIN_Y,
  libraryBesidePanelBox,
  libraryDockIslandBoxStyle,
  libraryDockIslandSize,
  predictRestoredLibraryDockBox,
  libraryDockStageBadgePos,
  libraryDockStageLeft,
  libraryDockStageOffset,
  libraryDockStageTransform,
  libraryDockStageVisualSize,
} from './libraryDockStage'

describe('libraryDockStageTransform', () => {
  it('puts rotateY left of scale so CSS scales the box first', () => {
    const transform = libraryDockStageTransform({
      x: -100,
      y: 20,
      z: 0,
      scale: 0.3,
      rotateY: 36,
      rotateX: 0,
    })
    const scaleAt = transform.indexOf('scale(0.3)')
    const rotateAt = transform.indexOf('rotateY(36deg)')
    assert.ok(scaleAt >= 0)
    assert.ok(rotateAt >= 0)
    assert.ok(rotateAt < scaleAt)
    assert.equal(transform.includes('scaleX'), false)
    assert.equal(transform.includes('scaleY'), false)
    assert.equal(transform.includes('perspective('), false)
  })

  it('does not double Motion unit suffixes', () => {
    const transform = libraryDockStageTransform({
      x: '-100px',
      y: '20px',
      z: '0px',
      scale: 0.3,
      rotateY: '36deg',
      rotateX: '0deg',
    })
    assert.equal(transform.includes('pxpx'), false)
    assert.equal(transform.includes('degdeg'), false)
    assert.ok(transform.includes('translate3d(-100px, 20px, 0px)'))
    assert.ok(transform.includes('rotateY(36deg)'))
    assert.ok(transform.includes('scale(0.3)'))
  })
})

describe('libraryDockIslandSize', () => {
  it('caps to 64rem × 40rem times 5/6 on a wide desktop', () => {
    const size = libraryDockIslandSize(1920, 1080, 16)
    assert.equal(size.width, 64 * 16 * (5 / 6))
    assert.equal(size.height, 40 * 16 * (5 / 6))
  })

  it('follows viewport on a shorter window', () => {
    const size = libraryDockIslandSize(900, 700, 16)
    assert.equal(size.width, 0.94 * 900 * (5 / 6))
    assert.equal(size.height, 0.7 * 700 * (5 / 6))
  })
})

describe('libraryDockStageLeft', () => {
  it('sits on the left inset because edit mode hides the nav island', () => {
    const left = libraryDockStageLeft(16)
    assert.equal(left, LIBRARY_DOCK_STAGE_INSET_REM * 16)
    assert.ok(left < 32)
  })
})

describe('libraryDockStageOffset', () => {
  it('parks the unscaled left edge on the left inset', () => {
    const island = libraryDockIslandSize(1920, 1080, 16)
    const bottom = LIBRARY_DOCK_BOTTOM_REM * 16
    const offset = libraryDockStageOffset({
      viewportWidth: 1920,
      viewportHeight: 1080,
      islandWidth: island.width,
      islandHeight: island.height,
      dockBottom: bottom,
      rootFontSize: 16,
    })
    const cssLeft = 1920 / 2
    const stagedLeft = cssLeft + offset.x
    assert.equal(stagedLeft, libraryDockStageLeft(16))
    const cssTop = 1080 - bottom - island.height
    const stagedTop = cssTop + offset.y
    assert.ok(Math.abs(stagedTop - (1080 - island.height) / 2) < 0.01)
  })

  it('moves left of the centered rest x', () => {
    const island = libraryDockIslandSize(1440, 900, 16)
    const offset = libraryDockStageOffset({
      viewportWidth: 1440,
      viewportHeight: 900,
      islandWidth: island.width,
      islandHeight: island.height,
      dockBottom: LIBRARY_DOCK_BOTTOM_REM * 16,
      rootFontSize: 16,
    })
    assert.ok(offset.x < -island.width / 2)
  })
})

describe('libraryDockStage hinge', () => {
  it('keeps the scaled thumbnail on-screen at the left inset', () => {
    const island = libraryDockIslandSize(1920, 1080, 16)
    const left = libraryDockStageLeft(16)
    const visual = libraryDockStageVisualSize({
      islandWidth: island.width,
      islandHeight: island.height,
    })
    assert.ok(left >= 0)
    assert.ok(left + visual.width < 1920 / 2)
  })
})

describe('libraryDockStageVisualSize', () => {
  it('reads as a window, not a sliver or a second dock', () => {
    const island = libraryDockIslandSize(1920, 1080, 16)
    const visual = libraryDockStageVisualSize({
      islandWidth: island.width,
      islandHeight: island.height,
    })
    assert.ok(visual.width > 60)
    assert.ok(visual.width < 120)
    assert.ok(visual.height > 130)
    assert.ok(visual.height < 200)
  })
})

describe('libraryDockStageBadgePos', () => {
  it('sits on the lower-left corner inside the left inset', () => {
    const island = libraryDockIslandSize(1920, 1080, 16)
    const pos = libraryDockStageBadgePos({
      islandHeight: island.height,
      rootFontSize: 16,
    })
    assert.ok(pos.left >= 0)
    assert.ok(pos.left < libraryDockStageLeft(16))
    assert.ok(pos.topOffset > 0)
  })
})

describe('libraryBesidePanelBox', () => {
  it('sits left of the panel with a 1rem gap and does not overlap', () => {
    const preferred = libraryDockIslandSize(1440, 900, 16)
    const panel = { top: 16, left: 1024 }
    const box = libraryBesidePanelBox({
      preferred,
      viewportWidth: 1440,
      viewportHeight: 900,
      panel,
      rootFontSize: 16,
    })
    const gap = LIBRARY_BESIDE_PANEL_GAP_REM * 16
    assert.equal(box.left + box.width, panel.left - gap)
    assert.ok(box.left + box.width <= panel.left)
    assert.equal(box.top, panel.top)
    assert.equal(box.bottom, 'auto')
    assert.ok(box.left >= LIBRARY_DOCK_STAGE_INSET_REM * 16)
  })

  it('shrinks when the remaining slot is narrower than the home dock', () => {
    const preferred = libraryDockIslandSize(1440, 900, 16)
    const panel = { top: 16, left: 480 }
    const box = libraryBesidePanelBox({
      preferred,
      viewportWidth: 900,
      viewportHeight: 900,
      panel,
      rootFontSize: 16,
    })
    assert.ok(box.width < preferred.width)
    assert.equal(
      box.width,
      panel.left - LIBRARY_BESIDE_PANEL_GAP_REM * 16 - LIBRARY_DOCK_STAGE_INSET_REM * 16,
    )
    assert.equal(box.left, LIBRARY_DOCK_STAGE_INSET_REM * 16)
  })

  it('falls back to the expanded control-panel chrome when the panel is unmeasured', () => {
    const edge = fallbackControlPanelEdge(1440, 16)
    assert.equal(edge.top, CONTROL_PANEL_INSET_REM * 16)
    assert.equal(
      edge.left,
      1440 - CONTROL_PANEL_INSET_REM * 16 - CONTROL_PANEL_EXPANDED_WIDTH_PX,
    )
    const preferred = libraryDockIslandSize(1440, 900, 16)
    const box = libraryBesidePanelBox({
      preferred,
      viewportWidth: 1440,
      viewportHeight: 900,
      panel: null,
      rootFontSize: 16,
    })
    assert.equal(
      box.left + box.width,
      edge.left - LIBRARY_BESIDE_PANEL_GAP_REM * 16,
    )
  })
})

describe('predictRestoredLibraryDockBox', () => {
  it('matches the rest pose: centered, bottom 5.25rem', () => {
    const island = libraryDockIslandSize(1920, 1080, 16)
    const box = predictRestoredLibraryDockBox(1920, 1080, 16)
    assert.deepEqual(box, {
      width: island.width,
      height: island.height,
      left: 1920 / 2 - island.width / 2,
      top: 1080 - 5.25 * 16 - island.height,
    })
  })
})

describe('libraryDockIslandBoxStyle', () => {
  it('matches park math so CSS does not own a second size', () => {
    const island = libraryDockIslandSize(1920, 1080, 16)
    const box = libraryDockIslandBoxStyle(island, 16)
    assert.equal(box.width, island.width)
    assert.equal(box.height, island.height)
    assert.equal(box.bottom, LIBRARY_DOCK_BOTTOM_REM * 16)
    assert.equal(box.left, '50%')
    assert.equal(
      box.transformOrigin,
      `${LIBRARY_DOCK_STAGE_ORIGIN_X * 100}% ${LIBRARY_DOCK_STAGE_ORIGIN_Y * 100}%`,
    )
  })
})

describe('library dock chrome contract', () => {
  it('treats home chrome as a data attr and nav/control as platform classes', () => {
    assert.equal(LIBRARY_DOCK_CHROME_ATTR, 'data-library-dock-chrome')
    assert.match(LIBRARY_DOCK_POINTER_CHROME, /\[data-library-dock-chrome\]/)
    assert.match(LIBRARY_DOCK_POINTER_CHROME, /\[data-sticker-pick\]/)
    assert.match(LIBRARY_DOCK_POINTER_CHROME, /\.nav-container/)
    assert.match(LIBRARY_DOCK_POINTER_CHROME, /\.global-control-bar/)
    assert.match(LIBRARY_DOCK_POINTER_CHROME, /\.tour-overlay/)
    assert.equal(LIBRARY_DOCK_POINTER_CHROME.includes('home-layout-rail'), false)
    assert.equal(
      LIBRARY_DOCK_POINTER_CHROME.includes('title-font-selector-panel'),
      false,
    )
  })

  it('derives restore suppress from the park duration', () => {
    assert.equal(
      LIBRARY_DOCK_RESTORE_SUPPRESS_MS,
      LIBRARY_DOCK_PARK_DURATION_S * 1000 + 90,
    )
  })

  it('does not duplicate island size or perspective in CSS', () => {
    const css = readFileSync(
      new URL('../components/WidgetLibraryIsland.css', import.meta.url),
      'utf8',
    )
    assert.equal(css.includes('calc(94vw * 5 / 6)'), false)
    assert.equal(css.includes('calc(64rem * 5 / 6)'), false)
    assert.equal(css.includes('calc(70vh * 5 / 6)'), false)
    assert.equal(css.includes('perspective: 1800px'), false)
    assert.ok(css.includes('.widget-library-island {'))
    assert.equal(css.includes('widget-library-island--dock'), false)
    assert.equal(css.includes('widget-library-island--panel'), false)
    assert.equal(css.includes('settings-motion.css'), false)
    assert.match(css, /z-index:\s*10000/)
    assert.match(
      css,
      /\.widget-library-tile-stage[\s\S]*background:\s*var\(--surface-bg/,
    )
    assert.equal(css.includes('#3a3a3c'), false)
  })

  it('marks home rail and style panel as dock chrome', () => {
    const home = readFileSync(
      new URL('../views/Home.tsx', import.meta.url),
      'utf8',
    )
    const style = readFileSync(
      new URL('../components/TitleFontSelector.tsx', import.meta.url),
      'utf8',
    )
    assert.match(home, /data-library-dock-chrome/)
    assert.match(style, /data-library-dock-chrome/)
  })

  it('keeps beside-panel fallback in lockstep with GCP chrome', () => {
    const css = readFileSync(
      new URL('../components/GlobalControlPanel.css', import.meta.url),
      'utf8',
    )
    assert.match(css, /\.global-control-bar\s*\{/)
    assert.match(css, /top:\s*1rem/)
    assert.match(css, /right:\s*1rem/)
    assert.match(css, /\.control-bar-trigger\.expanded/)
    assert.match(css, /width:\s*400px/)
    assert.equal(CONTROL_PANEL_INSET_REM, 1)
    assert.equal(CONTROL_PANEL_EXPANDED_WIDTH_PX, 400)
    assert.equal(CONTROL_PANEL_EXPANDED_RADIUS_REM, 1.5)
    assert.equal(CONTROL_PANEL_HEIGHT_COMPENSATION, 1.08)
    assert.equal(CONTROL_PANEL_MOBILE_MAX_PX, 640)
    assert.equal(CONTROL_PANEL_MOBILE_INSET_REM, 0.75)
    assert.match(css, /@media \(width <= 640px\)/)
    assert.match(css, /top:\s*0\.75rem/)
    assert.match(css, /width:\s*calc\(100vw - 1\.5rem\)/)
    assert.match(css, /border-radius:\s*1\.5rem/)
    assert.equal(CONTROL_PANEL_COLLAPSED_WIDTH_PX, 160)
    assert.equal(CONTROL_PANEL_COLLAPSED_HEIGHT_REM, 3)
    assert.equal(CONTROL_PANEL_COLLAPSED_RADIUS_REM, 2)
    assert.match(css, /width:\s*160px/)
    assert.match(css, /height:\s*3rem/)
    assert.match(css, /border-radius:\s*2rem/)
  })
})

describe('predictExpandedControlPanelBox', () => {
  it('uses desktop chrome: 1rem inset, 400px wide', () => {
    const box = predictExpandedControlPanelBox(1920, 500, 16)
    assert.deepEqual(box, {
      top: 16,
      left: 1920 - 16 - 400,
      width: 400,
      height: Math.ceil(500 * 1.08),
    })
  })

  it('uses mobile chrome: 0.75rem inset, remaining viewport width', () => {
    const box = predictExpandedControlPanelBox(390, 400, 16)
    assert.deepEqual(box, {
      top: 12,
      left: 12,
      width: 390 - 24,
      height: Math.ceil(400 * 1.08),
    })
  })
})

describe('predictCollapsedControlPanelBox', () => {
  it('uses desktop chrome: 1rem inset, 160×3rem', () => {
    assert.deepEqual(predictCollapsedControlPanelBox(1920, 16), {
      top: 16,
      left: 1920 - 16 - 160,
      width: 160,
      height: 48,
    })
  })

  it('uses mobile inset and keeps the collapsed island size', () => {
    assert.deepEqual(predictCollapsedControlPanelBox(390, 16), {
      top: 12,
      left: 390 - 12 - 160,
      width: 160,
      height: 48,
    })
  })
})

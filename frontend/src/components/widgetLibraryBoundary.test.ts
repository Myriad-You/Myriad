import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

describe('widget library / grid boundary', () => {
  it('keeps the catalog island out of WidgetGrid', () => {
    const grid = readFileSync(new URL('./WidgetGrid.tsx', import.meta.url), 'utf8')
    const item = readFileSync(
      new URL('./WidgetGridItem.tsx', import.meta.url),
      'utf8',
    )
    const drag = readFileSync(
      new URL('./widgetGridDrag.ts', import.meta.url),
      'utf8',
    )
    assert.equal(grid.includes('WidgetLibraryIsland'), false)
    assert.equal(grid.includes('libraryVariant'), false)
    assert.equal(grid.includes('onToggleEditMode'), false)
    assert.equal(grid.includes("variant === 'default'"), false)
    assert.equal(grid.includes("variant === 'panel'"), false)
    assert.equal(grid.includes('shouldRenderLibraryPlacementPreview'), false)
    assert.match(item, /shouldSkipWidgetEntrance/)
    assert.match(drag, /previewExiting/)
    assert.match(drag, /previewUncovered/)
  })

  it('keeps library chrome CSS out of the grid stylesheet', () => {
    const gridCss = readFileSync(
      new URL('./WidgetGrid.css', import.meta.url),
      'utf8',
    )
    assert.equal(gridCss.includes('widget-library-island'), false)
    assert.ok(gridCss.includes('widget-grid-host'))
    assert.ok(gridCss.includes('widget-grid-drag-ghost'))
    assert.ok(gridCss.includes('widget-grid-drag-ghost-tile'))
    assert.ok(gridCss.includes('widget-grid-drop-slot'))
    assert.ok(gridCss.includes('is-exiting'))
    assert.ok(gridCss.includes('widget-grid-item-handoff'))
    assert.ok(gridCss.includes('widget-grid-item-remove'))
  })

  it('pages mount the dock island as a grid sibling', () => {
    const home = readFileSync(new URL('../views/Home.tsx', import.meta.url), 'utf8')
    const panel = readFileSync(
      new URL('./ControlPanel/ControlPanelWidgets.tsx', import.meta.url),
      'utf8',
    )
    assert.match(home, /<WidgetLibraryIsland/)
    assert.match(panel, /<WidgetLibraryIsland/)
    assert.equal(home.includes('variant="dock"'), false)
    assert.equal(home.includes('variant="panel"'), false)
    assert.equal(panel.includes('variant="dock"'), false)
    assert.equal(panel.includes('variant="panel"'), false)
    assert.equal(home.includes('libraryVariant'), false)
    assert.equal(panel.includes('libraryVariant'), false)
    assert.match(panel, /parkable=\{false\}/)
    assert.equal(home.includes('parkable={false}'), false)
    const island = readFileSync(
      new URL('./WidgetLibraryIsland.tsx', import.meta.url),
      'utf8',
    )
    const islandCss = readFileSync(
      new URL('./WidgetLibraryIsland.css', import.meta.url),
      'utf8',
    )
    assert.equal(island.includes('widget-library-island--dock'), false)
    assert.equal(island.includes('widget-library-island--panel'), false)
    assert.equal(island.includes('widget-library--gallery'), false)
    assert.equal(islandCss.includes('widget-library-island--dock'), false)
    assert.equal(islandCss.includes('widget-library-island--panel'), false)
    assert.equal(islandCss.includes('widget-library-sidebar-label'), false)
    assert.equal(islandCss.includes('widget-library-title-row'), false)
    assert.equal(islandCss.includes('widget-library-toolbar'), false)
    assert.equal(islandCss.includes('widget-library-canvas'), false)
    assert.equal(islandCss.includes('widget-library-dock-body'), false)
    assert.equal(islandCss.includes('widget-library-item-frame'), false)
    assert.equal(islandCss.includes('settings-motion.css'), false)
    assert.equal(island.includes('widget-library-title-row'), false)
    assert.equal(island.includes('widget-library-toolbar'), false)
    assert.equal(island.includes('widget-library-canvas'), false)
    assert.equal(island.includes('widget-library-dock-body'), false)
    assert.equal(island.includes('--widget-library-preview-scale'), false)
    assert.equal(island.includes('isEditMode={true}'), false)
    assert.match(island, /isEditMode=\{false\}/)
  })

  it('does not dim the page while editing control-panel widgets', () => {
    const panel = readFileSync(
      new URL('./ControlPanel/ControlPanelWidgets.tsx', import.meta.url),
      'utf8',
    )
    assert.equal(panel.includes('backdrop-blur-sm'), false)
    assert.equal(panel.includes('bg-black/20'), false)
    assert.equal(panel.includes('createPortal'), false)
    assert.match(panel, /gcp-widget-edit/)
    const gcpCss = readFileSync(
      new URL('./GlobalControlPanel.css', import.meta.url),
      'utf8',
    )
    assert.match(gcpCss, /html\.gcp-widget-edit/)
  })
})

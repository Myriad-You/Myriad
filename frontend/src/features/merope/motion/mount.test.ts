import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

/** Absence scans only: which lifecycle a surface must not take. */
test('live faces share one motion owner; workbench preview stays isolated', () => {
  const panel = readFileSync(
    new URL(
      '../../../components/agent-panel/AgentPanelFace.tsx',
      import.meta.url,
    ),
    'utf8',
  )
  const widget = readFileSync(
    new URL('../../../components/widgets/MeropeWidget.tsx', import.meta.url),
    'utf8',
  )
  const studio = readFileSync(
    new URL('../SiteMotionWorkbench.tsx', import.meta.url),
    'utf8',
  )
  const workbench = readFileSync(
    new URL('../anime25drig/Anime25DWorkbench.tsx', import.meta.url),
    'utf8',
  )
  const lifecycle = readFileSync(
    new URL('./useRigMotionLifecycle.ts', import.meta.url),
    'utf8',
  )

  assert.doesNotMatch(panel, /useRigSingingLifecycle/)
  assert.doesNotMatch(panel, /useRigPreviewMotionLifecycle/)
  assert.match(panel, /showCharacter\s*=\s*motionReady\s*\|\|/)
  assert.match(panel, /ready:\s*motionReady/)
  assert.doesNotMatch(widget, /speechOccupancyRef/)
  assert.doesNotMatch(widget, /useRigPreviewMotionLifecycle/)
  assert.match(widget, /showCharacter\s*=\s*motionReady\s*\|\|/)
  assert.match(widget, /ready:\s*motionReady/)
  assert.doesNotMatch(studio, /useRigSingingLifecycle/)
  assert.doesNotMatch(studio, /getProductionMotionRuntime/)
  assert.doesNotMatch(workbench, /useRigMotionLifecycle/)
  assert.doesNotMatch(workbench, /useRigSingingLifecycle/)
  assert.doesNotMatch(workbench, /getProductionMotionRuntime/)
  assert.doesNotMatch(lifecycle, /replaceDriver/)
})

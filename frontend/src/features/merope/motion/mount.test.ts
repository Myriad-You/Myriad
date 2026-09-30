import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { workbenchSources } from '../workbench/sources.test-support'

/** Absence scans only */
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
  const studio = workbenchSources()
  // The studio owns the preview lifecycle; the panels under it must not.
  const workbench = workbenchSources(['SiteMotionWorkbench.tsx'])
  const lifecycle = readFileSync(
    new URL('./useRigMotionLifecycle.ts', import.meta.url),
    'utf8',
  )

  assert.doesNotMatch(panel, /useRigSingingLifecycle/)
  assert.doesNotMatch(panel, /useRigPreviewMotionLifecycle/)
  assert.match(panel, /wantLive = Boolean\(packageKey\)/)
  assert.match(panel, /ready:\s*motionReady/)
  assert.match(panel, /manifest=\{playsLive \|\| mounted \? manifest : null\}/)
  assert.doesNotMatch(widget, /speechOccupancyRef/)
  assert.doesNotMatch(widget, /useRigPreviewMotionLifecycle/)
  assert.match(widget, /wantLive = Boolean\(packageKey\)/)
  assert.match(widget, /ready:\s*motionReady/)
  assert.doesNotMatch(studio, /useRigSingingLifecycle/)
  assert.doesNotMatch(studio, /getProductionMotionRuntime/)
  assert.doesNotMatch(workbench, /useRigMotionLifecycle/)
  assert.doesNotMatch(workbench, /useRigSingingLifecycle/)
  assert.doesNotMatch(workbench, /getProductionMotionRuntime/)
  assert.doesNotMatch(lifecycle, /replaceDriver/)
})

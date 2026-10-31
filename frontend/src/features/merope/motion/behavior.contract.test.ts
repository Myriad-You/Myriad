import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import { arbitrateFaceSpeech } from '../presence/faceSpeechArbitration'
import { workbenchSources } from '../workbench/sources.test-support'

// The figure's own half of these contracts is figure/src/motion/behavior.contract.test.ts.
function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), 'utf8')
}

test('background Work cannot take the visible Chat face', () => {
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'chat',
      incomingMode: 'work',
      chatUtteranceActive: false,
    }),
    'record-without-speech',
  )
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'work',
      incomingMode: 'work',
      chatUtteranceActive: true,
    }),
    'record-without-speech',
  )
  assert.equal(
    arbitrateFaceSpeech({
      visibleMode: 'chat',
      incomingMode: 'chat',
      chatUtteranceActive: false,
    }),
    'speak',
  )
})

test('the site singing lifecycle consumes the snapshot and takes no rig', () => {
  assert.doesNotMatch(
    source('./useRigSingingLifecycle.ts'),
    /applySingingWrite/,
  )
  assert.doesNotMatch(source('./useRigSingingLifecycle.ts'), /rigRef/)
})

test('the site motion lifecycle stays renderer-neutral', () => {
  assert.doesNotMatch(source('./useRigMotionLifecycle.ts'), /RigCharacter/)
})

test('workbench preview stays off the production coordinator', () => {
  const workbench = workbenchSources(['SiteMotionWorkbench.tsx'])
  const studio = workbenchSources()
  assert.doesNotMatch(workbench, /useRigMotionLifecycle/)
  assert.doesNotMatch(workbench, /getProductionCoordinator/)
  assert.doesNotMatch(workbench, /getProductionMotionRuntime/)
  assert.doesNotMatch(studio, /useRigSingingLifecycle/)
  assert.doesNotMatch(studio, /getProductionMotionRuntime/)
})

test('the production runtime host does not build its own runtime', () => {
  assert.doesNotMatch(source('./runtimeHost.ts'), /new MotionRuntime/)
})

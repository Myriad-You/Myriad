import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'
import {
  FACE_PRESENCE_ENTER_MS,
  FACE_PRESENCE_EXIT_MS,
  FACE_PRESENCE_REST_MS,
  facePresenceDurationMs,
  INITIAL_FACE_PRESENCE,
  reduceFacePresence,
} from './reduceFacePresence'

describe('face presence', () => {
  test('first show waits for ready then enters', () => {
    let state = reduceFacePresence(INITIAL_FACE_PRESENCE, {
      type: 'show',
      packageKey: 'coat',
    })
    assert.equal(state.phase, 'pending')
    assert.equal(state.liveMounted, true)
    assert.equal(state.vacant, false)
    state = reduceFacePresence(state, { type: 'ready' })
    assert.equal(state.phase, 'enter')
    state = reduceFacePresence(state, { type: 'elapsed' })
    assert.equal(state.phase, 'shown')
  })

  test('hide after shown exits then vacates without keeping the player', () => {
    let state = reduceFacePresence(INITIAL_FACE_PRESENCE, {
      type: 'show',
      packageKey: 'coat',
    })
    state = reduceFacePresence(state, { type: 'ready' })
    state = reduceFacePresence(state, { type: 'elapsed' })
    state = reduceFacePresence(state, { type: 'hide' })
    assert.equal(state.phase, 'exit')
    assert.equal(state.liveMounted, false)
    assert.equal(state.vacant, false)
    state = reduceFacePresence(state, { type: 'elapsed' })
    assert.equal(state.phase, 'hidden')
    assert.equal(state.vacant, true)
    assert.equal(state.liveMounted, false)
  })

  test('a package swap exits, rests, then mounts the next package to enter', () => {
    let state = reduceFacePresence(INITIAL_FACE_PRESENCE, {
      type: 'show',
      packageKey: 'coat',
    })
    state = reduceFacePresence(state, { type: 'ready' })
    state = reduceFacePresence(state, { type: 'elapsed' })
    state = reduceFacePresence(state, { type: 'swap', packageKey: 'uniform' })
    assert.equal(state.phase, 'exit')
    assert.equal(state.liveMounted, false)
    assert.equal(state.packageKey, 'coat')
    assert.equal(state.nextPackageKey, 'uniform')
    assert.equal(state.vacant, false)
    state = reduceFacePresence(state, { type: 'ready' })
    assert.equal(state.phase, 'exit')
    state = reduceFacePresence(state, { type: 'elapsed' })
    assert.equal(state.phase, 'rest')
    assert.equal(state.liveMounted, false)
    assert.equal(state.nextPackageKey, 'uniform')
    state = reduceFacePresence(state, { type: 'elapsed' })
    assert.equal(state.phase, 'pending')
    assert.equal(state.liveMounted, true)
    assert.equal(state.packageKey, 'uniform')
    assert.equal(state.nextPackageKey, null)
    state = reduceFacePresence(state, { type: 'ready' })
    assert.equal(state.phase, 'enter')
  })

  test('hide during the empty beat does not resurrect the next package', () => {
    let state = reduceFacePresence(INITIAL_FACE_PRESENCE, {
      type: 'show',
      packageKey: 'coat',
    })
    state = reduceFacePresence(state, { type: 'ready' })
    state = reduceFacePresence(state, { type: 'elapsed' })
    state = reduceFacePresence(state, { type: 'swap', packageKey: 'uniform' })
    state = reduceFacePresence(state, { type: 'elapsed' })
    assert.equal(state.phase, 'rest')
    state = reduceFacePresence(state, { type: 'hide' })
    assert.equal(state.phase, 'hidden')
    assert.equal(state.liveMounted, false)
    assert.equal(state.nextPackageKey, null)
    assert.equal(state.vacant, true)
  })

  test('pending hide does not play an exit for a face that never appeared', () => {
    let state = reduceFacePresence(INITIAL_FACE_PRESENCE, {
      type: 'show',
      packageKey: 'coat',
    })
    state = reduceFacePresence(state, { type: 'hide' })
    assert.equal(state.phase, 'hidden')
    assert.equal(state.liveMounted, false)
    assert.equal(state.vacant, true)
  })

  test('exit is shorter than enter, with a rest beat between them', () => {
    assert.equal(FACE_PRESENCE_EXIT_MS, 360)
    assert.equal(FACE_PRESENCE_REST_MS, 100)
    assert.equal(FACE_PRESENCE_ENTER_MS, 540)
    assert.ok(FACE_PRESENCE_EXIT_MS < FACE_PRESENCE_ENTER_MS)
    assert.equal(facePresenceDurationMs('exit', false), FACE_PRESENCE_EXIT_MS)
    assert.equal(facePresenceDurationMs('rest', false), FACE_PRESENCE_REST_MS)
    assert.equal(facePresenceDurationMs('enter', false), FACE_PRESENCE_ENTER_MS)
    assert.equal(facePresenceDurationMs('exit', true), 0)
    assert.equal(facePresenceDurationMs('rest', true), 0)
  })
})

test('panel and widget share face presence, not a second WebGL', () => {
  const presence = readFileSync(
    new URL('./reduceFacePresence.ts', import.meta.url),
    'utf8',
  )
  const ui = readFileSync(new URL('./FacePresence.tsx', import.meta.url), 'utf8')
  const css = readFileSync(new URL('./merope.css', import.meta.url), 'utf8')
  const panel = readFileSync(
    new URL('../../components/agent-panel/AgentPanelFace.tsx', import.meta.url),
    'utf8',
  )
  const widget = readFileSync(
    new URL('../../components/widgets/MeropeWidget.tsx', import.meta.url),
    'utf8',
  )
  assert.match(presence, /copySurfaceFrame/)
  assert.match(ui, /copySurfaceFrame/)
  assert.match(ui, /data-phase=\{state.phase\}/)
  assert.match(css, /--face-presence-enter-ms:\s*540ms/)
  assert.match(css, /--face-presence-exit-ms:\s*360ms/)
  assert.match(css, /--face-presence-rest-ms:\s*100ms/)
  assert.match(css, /@keyframes face-presence-enter/)
  assert.match(css, /@keyframes face-presence-exit/)
  assert.doesNotMatch(
    css,
    /@keyframes face-presence-enter[\s\S]*?scale:/,
    'enter must not zoom',
  )
  assert.doesNotMatch(
    css,
    /@keyframes face-presence-exit[\s\S]*?scale:/,
    'exit must not zoom',
  )
  assert.doesNotMatch(css, /\.face-presence__live \{[\s\S]*?scale:/)
  assert.doesNotMatch(presence, /'replace'/)
  assert.doesNotMatch(css, /data-phase='enter'\].*face-presence__hold/)
  assert.match(css, /prefers-reduced-motion/)
  assert.match(panel, /FacePresence/)
  assert.match(widget, /FacePresence/)
  assert.match(ui, /onLiveUnmounted/)
  assert.match(ui, /if \(!present \|\| !ready \|\| wasReady\) return/)
  assert.match(panel, /onLiveUnmounted=/)
  assert.match(widget, /onLiveUnmounted=/)
  assert.doesNotMatch(ui, /getContext\('webgl/)
  assert.doesNotMatch(presence, /getContext\('webgl/)
})

test('outfit changes stay on the live player instead of exiting the stage', () => {
  const panel = readFileSync(
    new URL('../../components/agent-panel/AgentPanelFace.tsx', import.meta.url),
    'utf8',
  )
  const widget = readFileSync(
    new URL('../../components/widgets/MeropeWidget.tsx', import.meta.url),
    'utf8',
  )
  const character = readFileSync(
    new URL('./anime25drig/Anime25DCharacter.tsx', import.meta.url),
    'utf8',
  )
  const player = readFileSync(
    new URL('./anime25drig/player.ts', import.meta.url),
    'utf8',
  )
  assert.match(panel, /\? 'live'/)
  assert.match(widget, /\? 'live'/)
  assert.doesNotMatch(panel, /\? atlasUrl/)
  assert.doesNotMatch(widget, /\? atlasUrl/)
  assert.match(character, /replaceLivePackage/)
  assert.match(character, /\[gpuEpoch, onPlaybackError\]/)
  assert.match(player, /async replaceLivePackage/)
  assert.match(player, /The last outfit keeps drawing until the next atlas is bound/)
})

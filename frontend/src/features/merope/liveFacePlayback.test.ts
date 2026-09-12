import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, test } from 'node:test'
import {
  claimLiveFacePlayback,
  LIVE_FACE_PLAYBACK_PRIORITY,
  liveFacePlaybackHolder,
  liveFacePlaybackVacating,
  notifyLiveFaceUnmounted,
  resetLiveFacePlaybackForTests,
  setLiveFaceWanted,
} from './liveFacePlayback'

describe('live face playback', { concurrency: false }, () => {
  test('the panel outranks the home widget after the widget vacates', () => {
    resetLiveFacePlaybackForTests()
    const widget = claimLiveFacePlayback(
      'widget:a',
      LIVE_FACE_PLAYBACK_PRIORITY.widget,
    )
    assert.equal(liveFacePlaybackHolder(), 'widget:a')
    const panel = claimLiveFacePlayback(
      'agent-panel-face',
      LIVE_FACE_PLAYBACK_PRIORITY.panel,
    )
    assert.equal(liveFacePlaybackHolder(), 'widget:a')
    assert.equal(liveFacePlaybackVacating(), 'widget:a')
    notifyLiveFaceUnmounted('widget:a')
    assert.equal(liveFacePlaybackHolder(), 'agent-panel-face')
    assert.equal(liveFacePlaybackVacating(), null)
    panel()
    assert.equal(liveFacePlaybackHolder(), 'widget:a')
    widget()
    assert.equal(liveFacePlaybackHolder(), null)
  })

  test('equal priority keeps the earlier claimer until it vacates', () => {
    resetLiveFacePlaybackForTests()
    const first = claimLiveFacePlayback('a', 1)
    const second = claimLiveFacePlayback('b', 1)
    assert.equal(liveFacePlaybackHolder(), 'a')
    assert.equal(liveFacePlaybackVacating(), null)
    first()
    assert.equal(liveFacePlaybackHolder(), 'b')
    second()
  })

  test('unmounting the holder transfers without waiting for a leftover notify', () => {
    resetLiveFacePlaybackForTests()
    const widget = claimLiveFacePlayback(
      'widget:a',
      LIVE_FACE_PLAYBACK_PRIORITY.widget,
    )
    claimLiveFacePlayback(
      'agent-panel-face',
      LIVE_FACE_PLAYBACK_PRIORITY.panel,
    )
    widget()
    assert.equal(liveFacePlaybackHolder(), 'agent-panel-face')
    assert.equal(liveFacePlaybackVacating(), null)
  })

  test('a package remount on the winner does not hand the lease away', () => {
    resetLiveFacePlaybackForTests()
    claimLiveFacePlayback('widget:a', LIVE_FACE_PLAYBACK_PRIORITY.widget)
    notifyLiveFaceUnmounted('widget:a')
    assert.equal(liveFacePlaybackHolder(), 'widget:a')
    assert.equal(liveFacePlaybackVacating(), null)
  })

  test('turning off the panel waits for it to unmount before the widget re-enters', () => {
    resetLiveFacePlaybackForTests()
    claimLiveFacePlayback('widget:a', LIVE_FACE_PLAYBACK_PRIORITY.widget)
    claimLiveFacePlayback(
      'agent-panel-face',
      LIVE_FACE_PLAYBACK_PRIORITY.panel,
    )
    notifyLiveFaceUnmounted('widget:a')
    assert.equal(liveFacePlaybackHolder(), 'agent-panel-face')
    setLiveFaceWanted(
      'agent-panel-face',
      false,
      LIVE_FACE_PLAYBACK_PRIORITY.panel,
    )
    assert.equal(liveFacePlaybackHolder(), 'agent-panel-face')
    assert.equal(liveFacePlaybackVacating(), 'agent-panel-face')
    notifyLiveFaceUnmounted('agent-panel-face')
    assert.equal(liveFacePlaybackHolder(), 'widget:a')
    assert.equal(liveFacePlaybackVacating(), null)
  })
})

test('widget and panel share one WebGL player', () => {
  const widget = readFileSync(
    new URL('../../components/widgets/MeropeWidget.tsx', import.meta.url),
    'utf8',
  )
  const panel = readFileSync(
    new URL('../../components/agent-panel/AgentPanelFace.tsx', import.meta.url),
    'utf8',
  )
  const lifecycle = readFileSync(
    new URL('./motion/useRigMotionLifecycle.ts', import.meta.url),
    'utf8',
  )
  assert.match(widget, /useLiveFacePlayback\(/)
  assert.match(widget, /notifyLiveFaceUnmounted\(playbackId\)/)
  assert.match(widget, /manifest=\{playsLive \|\| mounted \? manifest : null\}/)
  assert.match(widget, /readyKeyAfterMotionChange\(motionReady, current\)/)
  assert.match(panel, /useLiveFacePlayback\(/)
  assert.match(panel, /LIVE_FACE_PLAYBACK_PRIORITY\.panel/)
  assert.match(panel, /notifyLiveFaceUnmounted\('agent-panel-face'\)/)
  assert.match(panel, /manifest=\{playsLive \|\| mounted \? manifest : null\}/)
  assert.match(panel, /if \(!motionReady\) setReadyKey\(''\)/)
  assert.match(lifecycle, /useRigSingingLifecycle\(options\.ready \?\? true\)/)
})

test('a playable rig never uses the master portrait as a stand-in', () => {
  const widget = readFileSync(
    new URL('../../components/widgets/MeropeWidget.tsx', import.meta.url),
    'utf8',
  )
  const panel = readFileSync(
    new URL('../../components/agent-panel/AgentPanelFace.tsx', import.meta.url),
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
  for (const source of [widget, panel]) {
    assert.match(source, /fallbackUrl=\{playableRig \? null : portraitUrl\}/)
  }
  assert.match(widget, /widgetFaceSlotTaken/)
  assert.match(widget, /function MeropeWidgetDuplicate/)
  assert.doesNotMatch(widget, /showTaken/)
  assert.doesNotMatch(panel, /widgetFaceSlotTaken/)
  assert.match(panel, /!loading && playbackEnabled/)
  assert.match(widget, /src=\{previewPortraitSrc\(\)\}/)
  assert.doesNotMatch(character, /fallbackUrl/)
  assert.match(character, /player\.tick\(1 \/ 60\)/)
  assert.match(character, /presentLive\(true\)/)
  assert.match(character, /if \(!recoverGpu\(\)\) onPlaybackError/)
  assert.match(character, /key=\{gpuEpoch\}/)
  assert.match(player, /loadImage\(atlasUrl, atlasAbort\.signal\)/)
  assert.match(
    player,
    /if \(this\.disposed \|\| atlasAbort\.signal\.aborted\) return/,
  )
})

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

test('realtime talk starts an Agora session then tears it down', () => {
  const source = readFileSync(
    new URL('./agoraConversation.ts', import.meta.url),
    'utf8',
  )
  assert.match(source, /startConvoSession/)
  assert.match(source, /stopConvoSession/)
  assert.match(source, /interruptConvoSession/)
  assert.match(source, /agora-rtc-sdk-ng/)
  assert.match(source, /agora-rtm/)
  assert.doesNotMatch(source, /agora-agent-client-toolkit/)
  assert.match(source, /ENABLE_AUDIO_PTS_METADATA/)
  assert.match(source, /addEventListener\(\s*'message'/)
  assert.match(source, /removeEventListener\(\s*'message'/)
  assert.match(source, /providerTurnId/)
  assert.match(source, /phase: articulation \? 'articulation' : 'energy'/)
  assert.match(source, /createMicrophoneAudioTrack/)
  assert.match(source, /attachLocalBargeIn\(/)
  assert.match(source, /user-unpublished/)
  assert.match(source, /owned\.remoteTrack && owned\.remoteTrack !== track/)
})

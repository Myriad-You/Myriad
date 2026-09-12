import assert from 'node:assert/strict'
import test from 'node:test'
import { setAgentPanelMode } from '../../components/agent-panel/agentPanelMode'
import { authSubject } from '../../utils/authSubject'
import { attachLiveBody, openTurnReply, openTurnSpeech } from './engineFace'
import { faceSpeechGate } from './faceSpeechArbitration'
import { getSpeechPipeline } from './speech/speechPipelineHost'

const enabled = { available: true, tts_enabled: true, persona_speech_enabled: true }

test('disabled TTS falls back to text mouth only for an admitted local reply', (t) => {
  const host = getSpeechPipeline()
  t.mock.method(host, 'probe', async () => false)
  host.applyStatus({ ...enabled, persona_speech_enabled: false })
  setAgentPanelMode('chat')
  try {
    assert.equal(openTurnSpeech('chat', 'visible-text').push('Hello'), null)
    assert.equal(openTurnSpeech('work', 'hidden-text').push('Hello'), 0)
    assert.equal(openTurnSpeech('chat', 'external-text', 0, undefined, 'external').push('Hello'), 0)
  } finally {
    setAgentPanelMode('work')
  }
})

test('stream TTS uses the same admission as text mouth, with no delayed background replay', (t) => {
  const host = getSpeechPipeline()
  t.mock.method(host, 'probe', async () => true)
  host.applyStatus(enabled)
  const queued: string[] = []
  t.mock.method(host.pipeline, 'enqueue', (segments) => {
    queued.push(...segments.map(segment => segment.messageId))
  })
  try {
    setAgentPanelMode('chat')
    const work = openTurnSpeech('work', 'background')
    const workMouth = openTurnReply('work', 'background')
    assert.equal(work.push('Background sentence.'), 0)
    workMouth.chunk('Background sentence.')
    const chat = openTurnSpeech('chat', 'foreground')
    const chatMouth = openTurnReply('chat', 'foreground')
    chat.push('Hello there. ')
    setAgentPanelMode('work')
    // A sentence already admitted can finish, without interrupting a Work task.
    assert.equal(faceSpeechGate.decide('work'), 'record-without-speech')
    chat.push('The rest of the sentence. ')
    chat.end()
    work.push('Late background sentence. ')
    work.end()
    workMouth.end()
    chatMouth.end()
    assert.ok(queued.length >= 2)
    assert.ok(queued.every(id => id === 'foreground'))
    assert.equal(faceSpeechGate.decide('work'), 'speak')
    const visibleWork = openTurnSpeech('work', 'visible-work')
    visibleWork.push('Visible work reply. ')
    visibleWork.end()
    assert.ok(queued.includes('visible-work'))
    const external = openTurnSpeech('work', 'external', 0, undefined, 'external')
    external.push('RTC must not also enqueue local speech.')
    external.end()
    assert.ok(!queued.includes('external'))
  } finally {
    faceSpeechGate.releaseChat()
    host.cancel()
    setAgentPanelMode('work')
  }
})

test('identity change stops playback and synthesis; late segments cannot revive the old subject', async (t) => {
  const host = getSpeechPipeline()
  t.mock.method(host, 'probe', async () => true)
  host.applyStatus(enabled)
  setAgentPanelMode('chat')
  const jobs: { signal: AbortSignal; resolve: (audio: ArrayBuffer) => void }[] = []
  t.mock.method(host, 'synthesize', (_segment, signal) => new Promise(resolve => {
    jobs.push({ signal, resolve })
  }))
  let stopped = 0
  let played = 0
  t.mock.method(host, 'play', () => {
    played += 1
    return { stop: () => { stopped += 1 } }
  })
  try {
    const old = openTurnSpeech('chat', 'old-subject')
    old.push('The first sentence. The second sentence. ')
    old.end()
    assert.equal(jobs.length, 2)
    jobs[0]!.resolve(new ArrayBuffer(1))
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(played, 1)
    // A body handing over to another surface is not loss of authorization.
    attachLiveBody()()
    assert.equal(stopped, 0)
    authSubject.change('next-user', true)
    assert.equal(stopped, 1)
    assert.equal(jobs[1]!.signal.aborted, true)
    assert.equal(host.available, false)
    host.applyStatus(enabled)
    assert.equal(old.push('An old token after a new login. '), 0)
    jobs[1]!.resolve(new ArrayBuffer(1))
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(played, 1)
    assert.equal(host.pipeline.queueLength, 0)
    const fresh = openTurnSpeech('chat', 'new-subject')
    fresh.push('The new user can speak. ')
    fresh.end()
    assert.equal(jobs.length, 3)
  } finally {
    host.cancel()
    authSubject.change('guest', true)
    setAgentPanelMode('work')
  }
})

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), 'utf8')
}

/**
 * Only "this path does not exist" belongs here: a regex over source text can
 * prove an absence, never a behaviour. Everything these tests used to assert
 * positively now has a real test, named alongside it.
 */

test('speech never reaches the run hub and never persists visemes', () => {
  // Behaviour: speech/ttsPlayer.test.ts, speech/ttsPipeline.test.ts.
  const host = source('./speechPipelineHost.ts')
  assert.match(host, /persona_speech_enabled/)
  assert.doesNotMatch(host, /run_hub|AgentProgressEvent/)
  const trace = source('../turnTrace.ts')
  assert.doesNotMatch(trace, /run_hub/)
  assert.doesNotMatch(trace, /phase: 'articulation'/)
})

test('recording never interrupts a work task', () => {
  // Behaviour: turnTrace.test.ts (an abandoned recording is dropped). The hook
  // itself needs a DOM, so the absences below stay textual. Barge-in may cancel
  // TTS via getSpeechPipeline, but must not cancel the Agent run.
  const recording = source('../../../components/agent-panel/useVoiceRecording.ts')
  assert.doesNotMatch(recording, /interruptCurrentTask/)
  assert.doesNotMatch(recording, /listenConsent/)
})

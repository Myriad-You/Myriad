import assert from 'node:assert/strict'
import test from 'node:test'
import { SpeechLifecycleController } from '../speechLifecycle'
import { RigMotionCoordinator } from './coordinator'
import { SpeechMotionLease } from './speechLease'

test('two speech producers: disposing one does not mute the other', () => {
  const coordinator = new RigMotionCoordinator()
  const panel = new SpeechMotionLease(coordinator)
  const home = new SpeechMotionLease(coordinator)
  panel.setBusy(true)
  home.setBusy(true)
  assert.equal(coordinator.owner('mouth', 0), 'speech')
  home.release()
  assert.equal(coordinator.owner('mouth', 0), 'speech')
  panel.release()
  assert.equal(coordinator.owner('mouth', 0), 'idle')
})

test('busy false and dispose are idempotent and cannot release a peer lease', () => {
  const coordinator = new RigMotionCoordinator()
  const panel = new SpeechMotionLease(coordinator)
  const home = new SpeechMotionLease(coordinator)
  panel.setBusy(true)
  home.setBusy(true)
  home.setBusy(false)
  home.setBusy(false)
  home.release()
  assert.equal(coordinator.owner('mouth', 0), 'speech')
})

test('speech start claims the mouth and dispose releases only that producer', () => {
  const coordinator = new RigMotionCoordinator()
  const panel = new SpeechMotionLease(coordinator)
  const home = new SpeechMotionLease(coordinator)
  const controller = new SpeechLifecycleController(
    {
      setSpeechActive: () => {},
      setAutoSpeech: () => {},
      setSpeechEnergy: () => {},
      setSpeechArticulation: () => {},
      enqueueSpeechText: () => {},
    },
    undefined,
    undefined,
    (busy) => panel.setBusy(busy),
  )
  home.setBusy(true)
  controller.handle({
    phase: 'start',
    messageId: 'message-1',
    utteranceId: 'stream-1',
    source: 'reply',
  })
  assert.equal(coordinator.owner('mouth', 0), 'speech')
  controller.dispose()
  assert.equal(coordinator.owner('mouth', 0), 'speech')
  home.release()
  assert.equal(coordinator.owner('mouth', 0), 'idle')
})

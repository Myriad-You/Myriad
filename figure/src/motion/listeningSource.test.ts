import assert from 'node:assert/strict'
import test from 'node:test'
import { ListeningMotionSource } from './listeningSource'

test('while they type she attends; a moment after they stop she lets it go', async () => {
  let changes = 0
  let now = 1000
  const source = new ListeningMotionSource(() => { changes += 1 }, () => now)
  assert.equal(source.current(now), null)
  source.typed(now)
  const plan = source.current(now)
  assert.equal(plan?.behaviors[0]?.function, 'attend', "a listener's attention")
  assert.equal(changes, 1)
  now += 800
  source.typed(now)
  assert.equal(source.current(now), plan, 'the same look while the message goes on')
  now += 1600
  assert.equal(source.current(now), null, 'stopped typing: no longer attending')
  source.stop()
})

test('a long message is met with a fresh look now and then, not one held forever', () => {
  let now = 0
  const source = new ListeningMotionSource(() => {}, () => now)
  source.typed(now)
  const first = source.current(now)
  for (; now <= 4500; now += 300) source.typed(now)
  const later = source.current(now)
  assert.ok(later && first && later.id !== first.id)
  source.stop()
})

test('her frame carries the attention while they type, and not over her own speaking', async () => {
  const { MotionRuntime } = await import('./runtime')
  const { RigMotionCoordinator } = await import('./coordinator')
  const attending = (runtime: InstanceType<typeof MotionRuntime>, now: number) =>
    runtime.frame(now).behaviors.some((b) => b.function === 'attend' && b.id.startsWith('listening:'))
  const listener = new MotionRuntime(new RigMotionCoordinator())
  const releaseListener = listener.retain()
  const now = globalThis.performance.now()
  assert.equal(attending(listener, now), false)
  listener.listening.typed(now)
  assert.equal(attending(listener, now), true, 'they type: she attends')
  releaseListener()
  const speaker = new MotionRuntime(new RigMotionCoordinator())
  const releaseSpeaker = speaker.retain()
  speaker.speech.handleForTest({ phase: 'start', source: 'reply', messageId: 'm', utteranceId: 'u' })
  speaker.speech.handleForTest({ phase: 'chunk', source: 'reply', messageId: 'm', utteranceId: 'u', text: '我在说话。' })
  speaker.listening.typed(now)
  assert.equal(attending(speaker, now), false, 'her own speaking is not overlaid')
  releaseSpeaker()
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { channelPriority } from './channels'
import { RigMotionCoordinator } from './coordinator'

test('channel table matches the live-face control order', () => {
  assert.ok(
    channelPriority('mouth', 'preview') > channelPriority('mouth', 'speech'),
  )
  assert.ok(
    channelPriority('mouth', 'speech') > channelPriority('mouth', 'music'),
  )
  assert.ok(
    channelPriority('headBody', 'performance') >
      channelPriority('headBody', 'music'),
  )
  assert.ok(
    channelPriority('headBody', 'music') >
      channelPriority('headBody', 'ambient'),
  )
  assert.ok(
    channelPriority('headBody', 'music') >
      channelPriority('headBody', 'coSpeech'),
  )
  assert.ok(
    channelPriority('expression', 'coSpeech') >
      channelPriority('expression', 'mood'),
  )
  assert.ok(
    channelPriority('gaze', 'preview') > channelPriority('gaze', 'performance'),
  )
})

test('speech steals only the mouth; music keeps the body', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('music', ['mouth', 'headBody'], { nowMs: 0 })
  coordinator.claim('speech', ['mouth'], { nowMs: 1 })
  const snapshot = coordinator.snapshot(1)
  assert.equal(snapshot.owners.mouth, 'speech')
  assert.equal(snapshot.owners.headBody, 'music')
  assert.equal(snapshot.owners.expression, 'idle')
})

test('explicit performance only takes the channels it claimed', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('music', ['mouth', 'headBody'], { nowMs: 0 })
  coordinator.claim('performance', ['expression'], { nowMs: 1 })
  const faceOnly = coordinator.snapshot(1)
  assert.equal(faceOnly.owners.expression, 'performance')
  assert.equal(faceOnly.owners.headBody, 'music')
  assert.equal(faceOnly.owners.mouth, 'music')

  coordinator.claim('performance', ['expression', 'headBody'], { nowMs: 2 })
  const withBody = coordinator.snapshot(2)
  assert.equal(withBody.owners.headBody, 'performance')
  assert.equal(withBody.owners.mouth, 'music')
})

test('preview outranks every live source on the selected channels', () => {
  const coordinator = new RigMotionCoordinator()
  coordinator.claim('speech', ['mouth'], { nowMs: 0 })
  coordinator.claim('music', ['headBody'], { nowMs: 0 })
  coordinator.claim('preview', ['mouth', 'headBody'], { nowMs: 1 })
  const snapshot = coordinator.snapshot(1)
  assert.equal(snapshot.owners.mouth, 'preview')
  assert.equal(snapshot.owners.headBody, 'preview')
})

test('expired leases auto-release and bump generation', () => {
  const coordinator = new RigMotionCoordinator()
  const first = coordinator.claim('music', ['mouth', 'headBody'], {
    nowMs: 0,
    ttlMs: 50,
  })
  assert.ok(first)
  assert.equal(coordinator.owner('headBody', 40), 'music')
  coordinator.tick(50)
  assert.equal(coordinator.owner('headBody', 50), 'idle')
  assert.ok(coordinator.snapshot(50).generation > first.generation)
})

test('releasing one channel leaves the rest of the lease', () => {
  const coordinator = new RigMotionCoordinator()
  const music = coordinator.claim('music', ['mouth', 'headBody'], { nowMs: 0 })
  assert.ok(music)
  coordinator.release(music, ['mouth'])
  const snapshot = coordinator.snapshot(0)
  assert.equal(snapshot.owners.mouth, 'idle')
  assert.equal(snapshot.owners.headBody, 'music')
})

test('a producer cannot release another producer lease', () => {
  const coordinator = new RigMotionCoordinator()
  const chat = coordinator.claim('speech', ['mouth'], { nowMs: 0 })
  const widget = coordinator.claim('speech', ['mouth'], { nowMs: 1 })
  assert.ok(chat)
  assert.ok(widget)
  assert.equal(
    coordinator.release({ ...chat, ownerToken: 'forged-token' }),
    false,
  )
  assert.equal(coordinator.release({ ...widget, leaseId: chat.leaseId }), false)
  assert.equal(coordinator.owner('mouth', 1), 'speech')
  assert.equal(coordinator.release(widget), true)
  assert.equal(coordinator.owner('mouth', 1), 'speech')
  assert.equal(coordinator.release(chat), true)
  assert.equal(coordinator.owner('mouth', 1), 'idle')
})

test('unmounting one speech producer leaves the other rig speaking', () => {
  const coordinator = new RigMotionCoordinator()
  const panel = coordinator.claim('speech', ['mouth'], { nowMs: 0 })
  const home = coordinator.claim('speech', ['mouth'], { nowMs: 1 })
  const homePerf = coordinator.claim('performance', ['expression'], {
    nowMs: 1,
  })
  assert.ok(panel)
  assert.ok(home)
  assert.ok(homePerf)
  coordinator.release(home)
  coordinator.release(homePerf)
  const snapshot = coordinator.snapshot(1)
  assert.equal(snapshot.owners.mouth, 'speech')
  assert.equal(snapshot.owners.expression, 'idle')
  assert.equal(snapshot.leases.length, 1)
  assert.equal(snapshot.leases[0]?.leaseId, panel.leaseId)
})

test('snapshots never expose owner tokens', () => {
  const coordinator = new RigMotionCoordinator()
  const handle = coordinator.claim('music', ['headBody'], { nowMs: 0 })
  assert.ok(handle)
  assert.ok(handle.ownerToken)
  const [lease] = coordinator.snapshot(0).leases
  assert.ok(lease)
  assert.equal('ownerToken' in lease, false)
})

test('renew refreshes TTL on the same lease id', () => {
  const coordinator = new RigMotionCoordinator()
  const first = coordinator.claim('music', ['headBody'], {
    nowMs: 0,
    ttlMs: 50,
  })
  assert.ok(first)
  const renewed = coordinator.renew(first, ['mouth', 'headBody'], {
    nowMs: 40,
    ttlMs: 50,
  })
  assert.ok(renewed)
  assert.equal(renewed.leaseId, first.leaseId)
  assert.equal(renewed.ownerToken, first.ownerToken)
  assert.ok(renewed.generation > first.generation)
  assert.equal(coordinator.owner('headBody', 80), 'music')
  coordinator.tick(90)
  assert.equal(coordinator.owner('headBody', 90), 'idle')
})

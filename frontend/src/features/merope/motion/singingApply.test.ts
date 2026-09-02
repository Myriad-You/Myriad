import assert from 'node:assert/strict'
import test from 'node:test'
import { resolveSingingApply } from './singingApply'

test('speech steals singing visemes without stopping the groove', () => {
  const apply = resolveSingingApply({
    gap: 'active',
    holdExpired: false,
    audioPaused: false,
    mouthOwner: 'speech',
    headBodyOwner: 'music',
  })
  assert.equal(apply.writeMouth, false)
  assert.equal(apply.writeGroove, true)
  assert.equal(apply.release, false)
  assert.equal(apply.restMouth, false)
})

test('a full stop still releases groove and rests a music-owned mouth', () => {
  const apply = resolveSingingApply({
    gap: 'stop',
    holdExpired: false,
    audioPaused: false,
    mouthOwner: 'music',
    headBodyOwner: 'music',
  })
  assert.equal(apply.release, true)
  assert.equal(apply.writeGroove, false)
  assert.equal(apply.restMouth, true)
})

test('pause and track-switch hold rest the mouth but keep the body pose', () => {
  const paused = resolveSingingApply({
    gap: 'active',
    holdExpired: false,
    audioPaused: true,
    mouthOwner: 'music',
    headBodyOwner: 'music',
  })
  assert.equal(paused.writeGroove, true)
  assert.equal(paused.restMouth, true)
  assert.equal(paused.writeMouth, false)

  const hold = resolveSingingApply({
    gap: 'hold',
    holdExpired: false,
    audioPaused: false,
    mouthOwner: 'music',
    headBodyOwner: 'music',
  })
  assert.equal(hold.writeGroove, true)
  assert.equal(hold.restMouth, true)

  const expired = resolveSingingApply({
    gap: 'hold',
    holdExpired: true,
    audioPaused: false,
    mouthOwner: 'music',
    headBodyOwner: 'music',
  })
  assert.equal(expired.release, true)
})

test('performance that claimed head/body suppresses groove, not the mouth', () => {
  const apply = resolveSingingApply({
    gap: 'active',
    holdExpired: false,
    audioPaused: false,
    mouthOwner: 'music',
    headBodyOwner: 'performance',
  })
  assert.equal(apply.writeGroove, false)
  assert.equal(apply.writeMouth, true)
})

test('preview ownership blocks live singing writes', () => {
  const apply = resolveSingingApply({
    gap: 'active',
    holdExpired: false,
    audioPaused: false,
    mouthOwner: 'preview',
    headBodyOwner: 'preview',
  })
  assert.equal(apply.writeGroove, false)
  assert.equal(apply.writeMouth, false)
  assert.equal(apply.release, false)
})

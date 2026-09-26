import type { RandomActionFrame, RandomActionName } from './randomAction'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  applyRandomActionFrame,
  HANDOFF_MAX,
  HANDOFF_MIN,
  idleHandoffSeconds,
  RandomActionController,
} from './randomAction'

function magnitude(frame: Readonly<RandomActionFrame>): number {
  return Math.max(
    Math.abs(frame.angleX),
    Math.abs(frame.angleY),
    Math.abs(frame.angleZ),
    Math.abs(frame.body),
    Math.abs(frame.brow),
    Math.abs(frame.eyeOpen),
    Math.abs(frame.armY),
    Math.abs(frame.armPos),
  )
}

test('waits briefly, then plays a visible staged action', () => {
  const controller = new RandomActionController(() => 0.5)
  const first = controller.sample(0, true, false)
  assert.equal(magnitude(first), 0)
  assert.equal(controller.getActiveAction(), null)
  assert.equal(magnitude(controller.sample(1.59, true, false)), 0)

  controller.sample(1.61, true, false)
  assert.notEqual(controller.getActiveAction(), null)
  const action = controller.sample(2.25, true, false)
  assert.ok(magnitude(action) > 0.1, `${magnitude(action)}`)
  assert.ok(action.ambientScale > 0.6 && action.ambientScale < 1)
})

test('reuses one frame object and keeps every action channel bounded', () => {
  let seed = 0x9E37_79B9
  const random = () => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
    return seed / 0x1_0000_0000
  }
  const controller = new RandomActionController(random)
  const output = controller.sample(0, true, false)
  let previous = { ...output }
  let largestStep = 0
  for (let frame = 1; frame <= 60 * 90; frame += 1) {
    const current = controller.sample(frame / 60, true, false)
    assert.equal(current, output)
    largestStep = Math.max(
      largestStep,
      Math.abs(current.angleX - previous.angleX),
      Math.abs(current.angleY - previous.angleY),
      Math.abs(current.angleZ - previous.angleZ),
      Math.abs(current.armY - previous.armY),
    )
    assert.ok(Math.abs(current.angleX) <= 0.2)
    assert.ok(Math.abs(current.angleY) <= 0.24)
    assert.ok(Math.abs(current.angleZ) <= 0.26)
    assert.ok(Math.abs(current.body) <= 0.28)
    assert.ok(Math.abs(current.brow) <= 0.42)
    assert.ok(Math.abs(current.browAngSym) <= 0.21)
    assert.ok(Math.abs(current.eyeOpen) <= 1)
    assert.ok(Math.abs(current.eyeX) <= 0.6)
    assert.ok(Math.abs(current.eyeY) <= 0.42)
    // An idle mood colours the mouth; only a yawn opens it wide.
    assert.ok(Math.abs(current.mouthForm) <= 0.65)
    assert.ok(current.mouthOpen >= 0 && current.mouthOpen <= 0.72)
    assert.ok(current.mouthRound >= 0 && current.mouthRound <= 0.47)
    assert.ok(Math.abs(current.armY) <= 0.526)
    assert.ok(Math.abs(current.armPos) <= 0.226)
    assert.ok(current.ambientScale >= 0.28 && current.ambientScale <= 1)
    previous = { ...current }
  }
  assert.ok(largestStep < 0.025)
})

test('cycles through the complete action catalog without immediate repeats', () => {
  let seed = 0x1234_ABCD
  const random = () => {
    seed = (seed * 1_103_515_245 + 12_345) >>> 0
    return seed / 0x1_0000_0000
  }
  const controller = new RandomActionController(random)
  const seen = new Set<RandomActionName>()
  let activeBefore: RandomActionName | null = null
  let lastStarted: RandomActionName | null = null
  for (let frame = 0; frame <= 60 * 600; frame += 1) {
    controller.sample(frame / 60, true, false)
    const active = controller.getActiveAction()
    if (active && activeBefore === null) {
      assert.notEqual(active, lastStarted)
      seen.add(active)
      lastStarted = active
    }
    activeBefore = active
  }
  assert.deepEqual(Iterator.from(seen).toArray().toSorted(), [
    'curiousTilt',
    'headDrift',
    'hum',
    'ponder',
    'postureShift',
    'shoulderEase',
    'smile',
    'softBlink',
    'yawn',
  ])
})

test('speech or another owner releases an action and resumes without a long freeze', () => {
  const controller = new RandomActionController(() => 0.5)
  controller.sample(0, true, false)
  controller.sample(1.61, true, false)
  const active = { ...controller.sample(2.25, true, false) }
  assert.ok(magnitude(active) > 0.06)

  const releaseStart = { ...controller.sample(2.25, true, true) }
  assert.deepEqual(releaseStart, active)
  assert.ok(magnitude(controller.sample(2.4, true, true)) < magnitude(active))
  assert.ok(magnitude(controller.sample(2.64, true, true)) < 1e-6)
  assert.equal(controller.getActiveAction(), null)

  assert.equal(magnitude(controller.sample(2.64, true, false)), 0)
  assert.equal(magnitude(controller.sample(3.4, true, false)), 0)
  controller.sample(4.05, true, false)
  assert.notEqual(controller.getActiveAction(), null)
})

test('a displacement waits longer to resume than automation returning does', () => {
  const resumed = (blockedOut: boolean): number => {
    const controller = new RandomActionController(() => 0.5)
    controller.sample(0, true, false)
    controller.sample(1.61, true, false)
    controller.sample(2.25, blockedOut, blockedOut)
    for (let frame = 0; frame <= 300; frame += 1) {
      const at = 2.64 + frame / 60
      controller.sample(at, true, false)
      if (controller.getActiveAction() !== null) return at
    }
    return Number.POSITIVE_INFINITY
  }
  // Automation coming back wants life at once. Being displaced by something
  // that owns the body does not: resuming on the same short delay is what let
  // a beat manufacture idle motion instead of merely yielding to it.
  const afterToggle = resumed(false)
  const afterDisplacement = resumed(true)
  assert.ok(Number.isFinite(afterDisplacement))
  assert.ok(
    afterDisplacement > afterToggle + 0.5,
    `displaced resumed at ${afterDisplacement}, toggle at ${afterToggle}`,
  )
  // ...but not so long that the character reads as frozen between beats.
  assert.ok(afterDisplacement - 2.64 < 2.5)
})

test('resuming after speech does not snap the head or hands', () => {
  const controller = new RandomActionController(() => 0.5)
  controller.sample(0, true, false)
  controller.sample(1.61, true, false)
  controller.sample(2.25, true, true)
  let previous = { ...controller.sample(2.64, true, false) }
  let largestStep = 0
  // Long enough to include the resume itself, or this only measures the
  // release ramp and says nothing about how the next clip enters.
  for (let frame = 1; frame <= 150; frame += 1) {
    const current = controller.sample(2.64 + frame / 60, true, false)
    largestStep = Math.max(
      largestStep,
      Math.abs(current.angleX - previous.angleX),
      Math.abs(current.angleY - previous.angleY),
      Math.abs(current.angleZ - previous.angleZ),
      Math.abs(current.armY - previous.armY),
    )
    previous = { ...current }
  }
  assert.ok(largestStep < 0.025)
})

test('composes expressions and gestures without reopening authored closed eyes', () => {
  const target = {
    angleX: 0.95,
    angleY: 0,
    angleZ: 0,
    body: 0,
    eyeX: 0,
    eyeY: 0,
    brow: 0.1,
    browAngSym: 0,
    eyeOpenL: 0,
    eyeOpenR: 0.8,
    irisScale: 1,
    armY: 0,
    armPos: 0,
  }
  const frame: RandomActionFrame = {
    angleX: 0.2,
    angleY: 0.1,
    angleZ: 0,
    body: 0.1,
    eyeX: 0.1,
    eyeY: 0,
    brow: 0.2,
    browAngSym: 0.1,
    eyeOpen: 0.1,
    irisScale: -0.05,
    armY: 0.3,
    armPos: -0.1,
    ambientScale: 0.4,
  }
  applyRandomActionFrame(target, frame, 1)

  assert.ok(target.angleX > 0.95 && target.angleX < 1)
  assert.equal(target.eyeOpenL, 0)
  assert.equal(target.eyeOpenR, 0.9)
  assert.ok(Math.abs(target.brow - 0.3) < 1e-12)
  assert.equal(target.armY, 0.3)
  assert.equal(target.armPos, -0.1)
})

test('the handoff window is set by the residue, not by whoever arrives', () => {
  const residue = (overrides: Partial<RandomActionFrame>): RandomActionFrame => ({
    angleX: 0,
    angleY: 0,
    angleZ: 0,
    body: 0,
    eyeX: 0,
    eyeY: 0,
    brow: 0,
    browAngSym: 0,
    eyeOpen: 0,
    irisScale: 0,
    armY: 0,
    armPos: 0,
    ambientScale: 1,
    ...overrides,
  })
  // 窗口跟出段残留走：shoulderEase 的 armY≈0.42 要比 softBlink 的残渣更长。
  const heavy = idleHandoffSeconds(residue({ armY: 0.42 }))
  const faint = idleHandoffSeconds(residue({ angleZ: 0.04 }))
  assert.ok(heavy > faint)
  assert.equal(idleHandoffSeconds(residue({})), HANDOFF_MIN)
  for (const window of [heavy, faint]) {
    assert.ok(window >= HANDOFF_MIN && window <= HANDOFF_MAX)
  }
  // Saturates rather than growing without bound on an out-of-range residue.
  assert.equal(idleHandoffSeconds(residue({ body: 9 })), HANDOFF_MAX)
})

test('a held action keeps easing into its pose instead of freezing', () => {
  const controller = new RandomActionController(() => 0.5)
  let frozen = 0
  let longestFrozen = 0
  let previous: number | null = null
  for (let frame = 0; frame < 60 * 12; frame += 1) {
    const sample = controller.sample(frame / 60, true, false)
    const active = controller.getActiveAction() !== null
    const pose = sample.angleX + sample.angleY * 3 + sample.angleZ * 7 + sample.body * 11
    if (active && previous !== null && Math.abs(pose - previous) < 1e-5) frozen += 1
    else frozen = 0
    longestFrozen = Math.max(longestFrozen, frozen)
    previous = active ? pose : null
  }
  // A sway turns back through a single still instant; a held pose used to freeze for over a second.
  assert.ok(longestFrozen / 60 < 0.2, `froze for ${longestFrozen / 60}s`)
})

test('the same idle move is played with different feelings, never the same face every time', () => {
  let seed = 0x5151_7777
  const random = () => {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0
    return seed / 0x1_0000_0000
  }
  const controller = new RandomActionController(random)
  const moods = new Set<string>()
  for (let frame = 0; frame <= 60 * 600; frame += 1) {
    controller.sample(frame / 60, true, false)
    if (controller.getActiveAction() === 'postureShift') moods.add(String(controller.getActiveMood()))
  }
  assert.ok(moods.size >= 2, [...moods].join(','))
})

test('a closed-eye smile shuts the lids and lifts the corners; it never uses the squeezed eyes', () => {
  const controller = new RandomActionController(() => 0.5)
  const catalog = (controller as unknown as { catalog: () => { name: string }[] }).catalog()
  const index = catalog.findIndex((action) => action.name === 'smile')
  ;(controller as unknown as { nextActionIndex: () => number }).nextActionIndex = () => index
  let closed = 0
  let smile = 0
  for (let frame = 0; frame < 60 * 6; frame += 1) {
    const sample = controller.sample(frame / 60, true, false)
    if (controller.getActiveAction() !== 'smile') continue
    closed = Math.min(closed, sample.eyeOpen)
    smile = Math.max(smile, sample.mouthForm)
  }
  assert.ok(closed < -0.7, `${closed}`)
  assert.ok(smile > 0.3, `${smile}`)
  assert.equal('eyeSqueeze' in controller.sample(10, true, false), false)
})

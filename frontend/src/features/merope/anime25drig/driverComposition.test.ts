import type { Anime25DDriver } from './driver'
import assert from 'node:assert/strict'
import test from 'node:test'
import { IDENTITY_DRIVER } from './driver'
import {
  applyAnime25DComposedPose,
  applyAnime25DSillyMouthOwnership,
  applyAnime25DStylizedExpression,
  prepareAnime25DWorkingTarget,
  resolveAnime25DStylizedTargets,
  stepAnime25DBlink,
  stepAnime25DDriverResponse,
} from './driverComposition'
import {
  intentExpressionOffset,
  mixBoundedExpressionChannel,
} from './performanceExpression'
import { zeroOccupancyOffset } from './poseCompositor'
import {
  stepMouthForm,
  stepMouthOpen,
  stepMouthSeal,
  stepMouthShape,
} from './speechResponse'
import { StylizedExpressionMotionController } from './stylizedExpressionMotion'

test('all shared pose producers land through one channel-weighted composition', () => {
  const target = { ...IDENTITY_DRIVER }
  const full = { gaze: 1, headBody: 1, expression: 1 }
  const none = { gaze: 0, headBody: 0, expression: 0 }
  const performance = intentExpressionOffset('think', 1)
  const stylized = new StylizedExpressionMotionController().sample(
    0,
    0,
    0,
    0,
    0,
    0,
  )
  applyAnime25DComposedPose(
    target,
    {
      ambient: full,
      random: none,
      groove: none,
      thinking: none,
      performance: full,
      stylized: full,
      coSpeech: full,
      speechMouth: 0,
      grooveMouth: 0,
    },
    {
      ambient: { angleX: 0, angleY: 0, angleZ: 0, body: 0, eyeX: 0, eyeY: 0 },
      randomAction: {
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
      },
      groove: {
        angleX: 0,
        angleY: 0,
        angleZ: 0,
        body: 0,
        armY: 0,
        armPos: 0,
        eyeX: 0,
        brow: 0,
      },
      thinking: {
        angleX: 0,
        angleY: 0,
        angleZ: 0,
        eyeX: 0,
        eyeY: 0,
        brow: 0,
        mouthCY: 0,
        mouthCAng: 0,
        mouthScale: 0,
      },
      breath: { angleX: 0.1, angleY: 0, angleZ: 0, body: 0.08 },
      performance,
      stylized,
      coSpeech: {
        brow: 0.1,
        eyeOpen: 0,
        angleY: 0.05,
        angleZ: 0.04,
        body: 0.12,
      },
    },
    zeroOccupancyOffset(),
  )

  assert.ok(target.angleX > 0.09, 'breath')
  assert.ok(target.body > 0.07, 'breath body')
  assert.ok(target.eyeX > 0.45, 'directed gaze')
  assert.ok(target.angleZ < -0.15, 'semantic head plus expressive range')
  assert.ok(target.brow > 0.23, 'semantic and co-speech brow')
})

test('semantic and staged expression extras honor independent ownership', () => {
  const target = { ...IDENTITY_DRIVER }
  const semantic = intentExpressionOffset('dizzy', 1)
  const controller = new StylizedExpressionMotionController()
  let stylized = controller.sample(0, 0, 0, 1, 0, 0)
  for (let frame = 1; frame <= 60; frame += 1) {
    stylized = controller.sample(frame / 60, 0, 0, 1, 0, 0)
  }

  applyAnime25DStylizedExpression(target, semantic, stylized, false, 0, 0.25)

  assert.equal(target.eyeDizzy, 0)
  assert.equal(target.maniac, 0)
  assert.equal(target.mouthOpen, stylized.mouthOpen * 0.25)
})

test('delight realizes its claimed bust resource through head/body ownership', () => {
  const target = { ...IDENTITY_DRIVER }
  const full = { gaze: 1, headBody: 1, expression: 1 }
  const none = { gaze: 0, headBody: 0, expression: 0 }
  const performance = intentExpressionOffset('delight', 1)
  const stylized = new StylizedExpressionMotionController().sample(
    0,
    0,
    0,
    0,
    0,
    0,
  )

  applyAnime25DComposedPose(
    target,
    {
      ambient: none,
      random: none,
      groove: none,
      thinking: none,
      performance: full,
      stylized: none,
      coSpeech: none,
      speechMouth: 0,
      grooveMouth: 0,
    },
    {
      ambient: { angleX: 0, angleY: 0, angleZ: 0, body: 0, eyeX: 0, eyeY: 0 },
      randomAction: {
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
      },
      groove: {
        angleX: 0,
        angleY: 0,
        angleZ: 0,
        body: 0,
        armY: 0,
        armPos: 0,
        eyeX: 0,
        brow: 0,
      },
      thinking: {
        angleX: 0,
        angleY: 0,
        angleZ: 0,
        eyeX: 0,
        eyeY: 0,
        brow: 0,
        mouthCY: 0,
        mouthCAng: 0,
        mouthScale: 0,
      },
      breath: { angleX: 0, angleY: 0, angleZ: 0, body: 0 },
      performance,
      stylized,
      coSpeech: { brow: 0, eyeOpen: 0, angleY: 0, angleZ: 0, body: 0 },
    },
    zeroOccupancyOffset(),
  )

  assert.ok(
    Math.abs(target.bust - (IDENTITY_DRIVER.bust + (performance.bust ?? 0))) <
      1e-12,
  )
})

test('working target preparation reuses its output and preserves legacy math', () => {
  const authored: Anime25DDriver = {
    ...IDENTITY_DRIVER,
    angleX: 0.2,
    angleY: -0.1,
    angleZ: 0.15,
    eyeX: 0.1,
    eyeY: -0.2,
    body: 0.25,
    mouse: true,
    idle: true,
  }
  const output = { ...IDENTITY_DRIVER }
  const time = 3.7
  const actual = prepareAnime25DWorkingTarget(
    output,
    authored,
    { x: 0.46, y: -0.33, inside: true },
    time,
  )

  assert.equal(actual, output)
  assert.equal(actual.angleX, 0.46 * 0.9)
  assert.equal(actual.angleY, 0.33 * 0.7)
  assert.equal(actual.angleZ, 0.15)
  assert.equal(actual.eyeX, 0.46 * 1.2)
  assert.equal(actual.eyeY, 0.33 * 0.8)
  assert.equal(actual.body, 0.25)
})

test('stylized target resolution reuses output and keeps special-eye blocking', () => {
  const target: Anime25DDriver = {
    ...IDENTITY_DRIVER,
    eyeCry: 0.18,
    eyeDizzy: 0.1,
    anger: 0.44,
    speechless: 0.2,
    maniac: 0.35,
    silly: 0.28,
    lovestruck: 0.16,
  }
  const semantic = {
    brow: 0,
    browAngSym: 0,
    eyeOpen: 0,
    eyeDizzy: 0.12,
    eyeSqueeze: 0.08,
    eyeCry: 0.15,
    eyeX: 0,
    eyeY: 0,
    mouthForm: 0,
    irisScale: 0,
    angleY: 0,
    angleZ: 0,
    anger: 0.21,
    speechless: 0.13,
    maniac: 0.19,
    silly: 0.17,
    lovestruck: 0.11,
  }
  const output = { anger: 0, speechless: 0, maniac: 0, silly: 0, lovestruck: 0 }
  const actual = resolveAnime25DStylizedTargets(output, target, semantic)
  const blocker =
    1 -
    Math.max(
      mixBoundedExpressionChannel(target.eyeDizzy, semantic.eyeDizzy, 0, 1, 0),
      mixBoundedExpressionChannel(
        target.eyeSqueeze,
        semantic.eyeSqueeze,
        0,
        1,
        0,
      ),
      mixBoundedExpressionChannel(target.eyeCry, semantic.eyeCry, 0, 1, 0),
    )

  assert.equal(actual, output)
  assert.deepEqual(actual, {
    anger:
      mixBoundedExpressionChannel(target.anger, semantic.anger, 0, 1, 0) *
      blocker,
    speechless:
      mixBoundedExpressionChannel(
        target.speechless,
        semantic.speechless,
        0,
        1,
        0,
      ) * blocker,
    maniac:
      mixBoundedExpressionChannel(target.maniac, semantic.maniac, 0, 1, 0) *
      blocker,
    silly:
      mixBoundedExpressionChannel(target.silly, semantic.silly, 0, 1, 0) *
      blocker,
    lovestruck:
      mixBoundedExpressionChannel(
        target.lovestruck,
        semantic.lovestruck,
        0,
        1,
        0,
      ) * blocker,
  })
})

test('silly mouth ownership only attenuates owned speech channels', () => {
  const target: Anime25DDriver = {
    ...IDENTITY_DRIVER,
    mouthOpen: 0.8,
    mouthWide: 0.6,
    mouthRound: 0.4,
    mouthNarrow: 0.2,
    mouthSeal: 0.1,
    mouthForm: -0.3,
    mouthCY: 0.25,
  }
  applyAnime25DSillyMouthOwnership(target, 0.75)
  assert.deepEqual(
    {
      mouthOpen: target.mouthOpen,
      mouthWide: target.mouthWide,
      mouthRound: target.mouthRound,
      mouthNarrow: target.mouthNarrow,
      mouthSeal: target.mouthSeal,
      mouthForm: target.mouthForm,
      mouthCY: target.mouthCY,
    },
    {
      mouthOpen: 0.2,
      mouthWide: 0.15,
      mouthRound: 0.1,
      mouthNarrow: 0.05,
      mouthSeal: 0.025,
      mouthForm: -0.3,
      mouthCY: 0.25,
    },
  )
})

test('blink stepping matches the frozen player state machine', () => {
  const actualState = { activeSeconds: -1, nextAtSeconds: 0.12 }
  const expectedState = { activeSeconds: -1, nextAtSeconds: 0.12 }
  const randomValues = [0.4, 0.12, 0.7, 0.6, 0.25, 0.1]
  let actualRandomIndex = 0
  let expectedRandomIndex = 0
  const actualRandom = () =>
    randomValues[actualRandomIndex++ % randomValues.length]
  const expectedRandom = () =>
    randomValues[expectedRandomIndex++ % randomValues.length]

  for (let frame = 0; frame < 240; frame += 1) {
    const time = frame / 60
    const dt = frame % 29 === 0 ? 1 / 30 : 1 / 60
    const enabled = frame % 47 !== 0
    const suppressed = frame >= 132 && frame < 139
    const actual = { ...IDENTITY_DRIVER, eyeOpenL: 0.92, eyeOpenR: 0.87 }
    const expected = { ...actual }
    stepAnime25DBlink(
      actual,
      actualState,
      time,
      dt,
      enabled,
      suppressed,
      actualRandom,
    )
    legacyStepBlink(
      expected,
      expectedState,
      time,
      dt,
      enabled,
      suppressed,
      expectedRandom,
    )
    assert.deepEqual(actualState, expectedState, `state at frame ${frame}`)
    assert.equal(
      actual.eyeOpenL,
      expected.eyeOpenL,
      `left eye at frame ${frame}`,
    )
    assert.equal(
      actual.eyeOpenR,
      expected.eyeOpenR,
      `right eye at frame ${frame}`,
    )
  }
  assert.equal(actualRandomIndex, expectedRandomIndex)
})

test('driver response matches the frozen player loop for every channel', () => {
  const actual = { ...IDENTITY_DRIVER }
  const expected = { ...IDENTITY_DRIVER }
  const authored = { ...IDENTITY_DRIVER }
  const target = { ...IDENTITY_DRIVER }
  const actualSecondary = { angleX: 0, angleY: 0, angleZ: 0, body: 0 }
  const expectedSecondary = { ...actualSecondary }
  const secondaryTarget = { ...actualSecondary }
  const keys = Object.keys(IDENTITY_DRIVER) as Array<keyof Anime25DDriver>

  for (let frame = 0; frame < 180; frame += 1) {
    for (let index = 0; index < keys.length; index += 1) {
      const key = keys[index]
      if (typeof target[key] === 'boolean') {
        continue
      }
      ;(target as unknown as Record<string, number>)[key] =
        Math.sin(frame * 0.073 + index * 0.41) * 0.72
    }
    target.eyeOpenL = (Math.sin(frame * 0.09) + 1) * 0.5
    target.eyeOpenR = (Math.cos(frame * 0.11) + 1) * 0.5
    target.irisScale = 0.9 + Math.sin(frame * 0.03) * 0.2
    target.mouthScale = 1 + Math.cos(frame * 0.05) * 0.25
    authored.idle = frame % 2 === 0
    authored.blink = frame % 3 !== 0
    authored.rand = frame % 5 !== 0
    authored.thinking = frame % 7 === 0
    authored.singing = frame % 11 === 0
    authored.talk = frame % 13 !== 0
    authored.mouse = frame % 17 === 0
    authored.phys = frame % 19 !== 0
    secondaryTarget.angleX = Math.sin(frame * 0.04)
    secondaryTarget.angleY = Math.cos(frame * 0.05)
    secondaryTarget.angleZ = Math.sin(frame * 0.06 + 0.4)
    secondaryTarget.body = Math.cos(frame * 0.03 + 0.7)
    const dt = frame % 23 === 0 ? 1 / 30 : 1 / 60

    stepAnime25DDriverResponse(
      actual,
      authored,
      target,
      actualSecondary,
      secondaryTarget,
      dt,
    )
    legacyStepDriverResponse(
      expected,
      authored,
      target,
      expectedSecondary,
      secondaryTarget,
      dt,
    )
    assert.deepEqual(actual, expected, `driver at frame ${frame}`)
    assert.deepEqual(
      actualSecondary,
      expectedSecondary,
      `secondary motion at frame ${frame}`,
    )
  }
})

function legacyStepBlink(
  target: Anime25DDriver,
  state: { activeSeconds: number; nextAtSeconds: number },
  time: number,
  dt: number,
  enabled: boolean,
  suppressed: boolean,
  random: () => number,
): void {
  if (suppressed) {
    state.activeSeconds = -1
    state.nextAtSeconds = time + 1.8
  } else if (enabled) {
    if (state.activeSeconds < 0 && time > state.nextAtSeconds) {
      state.activeSeconds = 0
      state.nextAtSeconds = time + 1.6 + random() * 3.8
      if (random() < 0.18) state.nextAtSeconds = time + 0.28
    }
    if (state.activeSeconds >= 0) {
      state.activeSeconds += dt
      const elapsed = state.activeSeconds
      let open = 1
      if (elapsed < 0.08) open = 1 - elapsed / 0.08
      else if (elapsed < 0.42) open = 0
      else if (elapsed < 0.58) open = (elapsed - 0.42) / 0.16
      else state.activeSeconds = -1
      target.eyeOpenL = Math.min(target.eyeOpenL, open)
      target.eyeOpenR = Math.min(target.eyeOpenR, open)
    }
  }
}

function legacyStepDriverResponse(
  current: Anime25DDriver,
  authored: Readonly<Anime25DDriver>,
  target: Readonly<Anime25DDriver>,
  secondaryCurrent: {
    angleX: number
    angleY: number
    angleZ: number
    body: number
  },
  secondaryTarget: Readonly<{
    angleX: number
    angleY: number
    angleZ: number
    body: number
  }>,
  dt: number,
): void {
  const rate = Math.min(1, dt * 14)
  const flags = [
    'idle',
    'blink',
    'rand',
    'thinking',
    'singing',
    'talk',
    'mouse',
    'phys',
  ] as const
  for (const key of Object.keys(IDENTITY_DRIVER) as Array<
    keyof Anime25DDriver
  >) {
    if (flags.includes(key as (typeof flags)[number])) {
      current[key] = authored[key] as never
      continue
    }
    const from = current[key] as number
    const to = target[key] as number
    if (key === 'mouthOpen') {
      current.mouthOpen = stepMouthOpen(from, to, dt)
    } else if (key === 'mouthForm') {
      current.mouthForm = stepMouthForm(from, to, dt)
    } else if (key === 'mouthSeal') {
      current.mouthSeal = stepMouthSeal(from, to, dt)
    } else if (
      key === 'mouthWide' ||
      key === 'mouthRound' ||
      key === 'mouthNarrow'
    ) {
      current[key] = stepMouthShape(from, to, dt)
    } else if (key === 'eyeCry') {
      const response = to > from ? 6 : 4.5
      current.eyeCry = from + (to - from) * (1 - Math.exp(-response * dt))
    } else if (key === 'maniac') {
      const response = to > from ? 7.2 : 4.4
      current.maniac = from + (to - from) * (1 - Math.exp(-response * dt))
    } else if (key === 'silly') {
      const response = to > from ? 7 : 4.2
      current.silly = from + (to - from) * (1 - Math.exp(-response * dt))
    } else if (key === 'lovestruck') {
      const response = to > from ? 6.6 : 3.8
      current.lovestruck = from + (to - from) * (1 - Math.exp(-response * dt))
    } else {
      ;(current[key] as number) = from + (to - from) * rate
    }
  }
  secondaryCurrent.angleX +=
    (secondaryTarget.angleX - secondaryCurrent.angleX) * rate
  secondaryCurrent.angleY +=
    (secondaryTarget.angleY - secondaryCurrent.angleY) * rate
  secondaryCurrent.angleZ +=
    (secondaryTarget.angleZ - secondaryCurrent.angleZ) * rate
  secondaryCurrent.body += (secondaryTarget.body - secondaryCurrent.body) * rate
}

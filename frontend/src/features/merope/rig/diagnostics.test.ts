import type { MeropeRigManifest } from './types'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  anime25DAbandonsCapability,
  diagnoseRig,
  rigCapabilityRegressions,
} from './diagnostics'

test('replacement regression gate reports only lost current capabilities', () => {
  const capabilities = {
    facial: true,
    lipSync: true,
    gaze: true,
    secondaryMotion: true,
    facialVariants: true,
    deformableSkinning: true,
    outfitAware: true,
    collisionAware: true,
    presentationCoverage: true,
  }
  const current = { score: 100, issues: [], capabilities }
  const candidate = {
    score: 80,
    issues: [],
    capabilities: {
      ...capabilities,
      gaze: false,
      secondaryMotion: false,
      deformableSkinning: false,
    },
  }
  assert.deepEqual(rigCapabilityRegressions(current, candidate), [
    'gaze',
    'secondaryMotion',
    'deformableSkinning',
  ])
  assert.deepEqual(rigCapabilityRegressions(candidate, current), [])
})

test('Anime2.5DRig replacement preserves FaceRig capability gates', () => {
  assert.equal(anime25DAbandonsCapability('presentationCoverage'), false)
  assert.equal(anime25DAbandonsCapability('collisionAware'), true)
  assert.equal(anime25DAbandonsCapability('gaze'), false)
  const current = {
    profile: 'face-rig' as const,
    score: 100,
    issues: [],
    capabilities: {
      facial: true,
      lipSync: true,
      gaze: true,
      secondaryMotion: true,
      facialVariants: true,
      deformableSkinning: true,
      outfitAware: true,
      collisionAware: true,
      presentationCoverage: true,
    },
  }
  const candidate = {
    ...current,
    profile: 'anime25d' as const,
    capabilities: {
      ...current.capabilities,
      gaze: false,
      collisionAware: false,
    },
  }
  assert.deepEqual(rigCapabilityRegressions(current, candidate), ['gaze'])
})

test('Anime2.5DRig diagnostics do not score retired limb gates', () => {
  const report = diagnoseRig({
    bones: [
      { id: 'root', parent: null },
      { id: 'body', parent: 'root' },
      { id: 'head', parent: 'body' },
      { id: 'face', parent: 'head' },
      { id: 'left-eye', parent: 'face' },
      { id: 'right-eye', parent: 'face' },
      { id: 'mouth', parent: 'face' },
      { id: 'a25d-front-hair-strand-1-hair-root', parent: 'head' },
    ],
    parts: [
      { id: 'a25d-face' },
      { id: 'a25d-eye-left-open', slot: 'eye-left', variant: 'open' },
      { id: 'a25d-eye-left-closed', slot: 'eye-left', variant: 'closed' },
      { id: 'a25d-eye-right-open', slot: 'eye-right', variant: 'open' },
      { id: 'a25d-eye-right-closed', slot: 'eye-right', variant: 'closed' },
      { id: 'a25d-mouth-open', slot: 'mouth', variant: 'open' },
      { id: 'a25d-mouth-close', slot: 'mouth', variant: 'closed' },
    ],
  } as unknown as MeropeRigManifest)
  assert.equal(report.profile, 'anime25d')
  assert.equal(report.capabilities.facialVariants, true)
  assert.equal(
    report.issues.some((item) =>
      [
        'missing-spatial-profile',
        'incomplete-presentation-coverage',
      ].includes(item.code),
    ),
    false,
  )
})

test('reports semantic animation capabilities and missing production features', () => {
  const report = diagnoseRig({
    bones: [
      { id: 'root' },
      { id: 'body' },
      { id: 'head' },
      { id: 'left-eye' },
      { id: 'mouth' },
      { id: 'front-hair' },
    ],
    parts: [],
  } as MeropeRigManifest)
  assert.equal(report.capabilities.lipSync, true)
  assert.equal(report.capabilities.secondaryMotion, true)
  assert.equal(report.capabilities.facialVariants, false)
  assert.equal(report.capabilities.deformableSkinning, false)
  assert.equal(report.capabilities.collisionAware, false)
  assert.ok(
    report.issues.some((item) => item.code === 'missing-spatial-profile'),
  )
  assert.ok(
    report.issues.some((item) => item.code === 'rigid-part-deformation'),
  )
  assert.ok(report.score < 100)
})

test('reports character-local head and torso volumes as collision-aware', () => {
  const report = diagnoseRig({
    bones: [
      { id: 'root', parent: null },
      { id: 'body', parent: 'root' },
      { id: 'head', parent: 'body' },
    ],
    parts: [],
    spatialProfile: {
      collisionVolumes: [
        { id: 'head', boneId: 'head' },
        { id: 'torso', boneId: 'body' },
      ],
    },
  } as MeropeRigManifest)
  assert.equal(report.capabilities.collisionAware, true)
  assert.equal(
    report.issues.some((item) => item.code === 'missing-spatial-profile'),
    false,
  )
})

test('reports unsafe presentation slot assets before runtime rendering', () => {
  const missingFallback = diagnoseRig({
    bones: [],
    parts: [{ slot: 'mouth', variant: 'open' }],
  } as unknown as MeropeRigManifest)
  assert.ok(
    missingFallback.issues.some(
      (item) => item.code === 'missing-presentation-fallback',
    ),
  )
  const unknown = diagnoseRig({
    bones: [],
    parts: [
      { slot: 'mouth', variant: 'closed' },
      { slot: 'mouth', variant: 'invented' },
    ],
  } as unknown as MeropeRigManifest)
  assert.ok(
    unknown.issues.some((item) => item.code === 'unknown-presentation-variant'),
  )
})

test('recognizes split facial textures', () => {
  const report = diagnoseRig({
    bones: [
      { id: 'root' },
      { id: 'body' },
      { id: 'head' },
      { id: 'left-eye' },
      { id: 'right-eye' },
      { id: 'mouth' },
      { id: 'a25d-handwear' },
    ],
    parts: [
      { slot: 'eye-left', variant: 'open' },
      { slot: 'eye-right', variant: 'open' },
      { slot: 'mouth', variant: 'closed' },
      { slot: 'mouth', variant: 'open' },
      { slot: 'mouth', variant: 'wide' },
      { slot: 'mouth', variant: 'round' },
    ],
  } as MeropeRigManifest)
  assert.equal(report.capabilities.facialVariants, true)
})

test('recognizes full-head expression frames while reporting absent split-eye gaze honestly', () => {
  const report = diagnoseRig({
    bones: [
      { id: 'root' },
      { id: 'body' },
      { id: 'head' },
      { id: 'left-eye' },
      { id: 'right-eye' },
      { id: 'mouth' },
    ],
    parts: [
      { slot: 'head-expression', variant: 'neutral' },
      { slot: 'head-expression', variant: 'happy' },
      { slot: 'head-expression', variant: 'sad' },
      { slot: 'head-expression', variant: 'blink' },
      { slot: 'mouth', variant: 'closed' },
      { slot: 'mouth', variant: 'open' },
      { slot: 'mouth', variant: 'wide' },
      { slot: 'mouth', variant: 'round' },
    ],
  } as MeropeRigManifest)
  assert.equal(report.capabilities.facialVariants, true)
  assert.equal(report.capabilities.gaze, false)
})

test('recognizes independently rendered iris layers as real visible gaze', () => {
  const report = diagnoseRig({
    bones: [
      { id: 'root' },
      { id: 'body' },
      { id: 'head' },
      { id: 'left-eye' },
      { id: 'right-eye' },
      { id: 'mouth' },
    ],
    parts: [
      { slot: 'head-expression', variant: 'neutral' },
      { slot: 'head-expression', variant: 'happy' },
      { slot: 'head-expression', variant: 'sad' },
      { slot: 'head-expression', variant: 'blink' },
      { slot: 'iris-left', variant: 'visible' },
      { slot: 'iris-left', variant: 'hidden' },
      { slot: 'iris-right', variant: 'visible' },
      { slot: 'iris-right', variant: 'hidden' },
      { slot: 'mouth', variant: 'closed' },
      { slot: 'mouth', variant: 'open' },
      { slot: 'mouth', variant: 'wide' },
      { slot: 'mouth', variant: 'round' },
    ],
  } as MeropeRigManifest)
  assert.equal(report.capabilities.gaze, true)
  assert.equal(
    report.issues.some((item) => item.code === 'missing-gaze'),
    false,
  )
})

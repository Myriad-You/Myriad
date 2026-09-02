import type { StylizedExpressionMotion } from './stylizedExpressionMotion'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  ANIME25D_DEFORMATION_EYE,
  ANIME25D_DEFORMATION_JAW,
  ANIME25D_DEFORMATION_MOUTH,
  ANIME25D_DEFORMATION_STYLIZED,
  ANIME25D_DEFORMATION_TIME,
  captureAnime25DDeformationChanges,
  createAnime25DDeformationChangeState,
  createAnime25DLayerDeformationPlan,
  markAnime25DLayerGeometryUpdated,
  resolveAnime25DDeformationDependencies,
  shouldUpdateAnime25DLayerGeometry,
} from './deformationDependencies'
import { IDENTITY_DRIVER } from './driver'

const ALL =
  ANIME25D_DEFORMATION_EYE |
  ANIME25D_DEFORMATION_MOUTH |
  ANIME25D_DEFORMATION_JAW |
  ANIME25D_DEFORMATION_STYLIZED |
  ANIME25D_DEFORMATION_TIME

const MORPH = {
  centerX: 10,
  centerY: 20,
  width: 30,
  height: 12,
  openMix: 0,
  wide: 0,
  round: 0,
  narrow: 0,
}

test('change capture reports only geometry inputs that actually moved', () => {
  const state = createAnime25DDeformationChangeState()
  assert.equal(
    captureAnime25DDeformationChanges(
      state,
      IDENTITY_DRIVER,
      MORPH,
      0,
      0,
      null,
    ),
    ALL,
  )
  assert.equal(
    captureAnime25DDeformationChanges(
      state,
      IDENTITY_DRIVER,
      MORPH,
      0,
      0,
      null,
    ),
    ANIME25D_DEFORMATION_TIME,
  )
  assert.equal(
    captureAnime25DDeformationChanges(
      state,
      { ...IDENTITY_DRIVER, eyeX: 0.25 },
      MORPH,
      0,
      0,
      null,
    ),
    ANIME25D_DEFORMATION_EYE | ANIME25D_DEFORMATION_TIME,
  )
  assert.equal(
    captureAnime25DDeformationChanges(
      state,
      { ...IDENTITY_DRIVER, eyeX: 0.25 },
      { ...MORPH, openMix: 0.4 },
      2,
      0.5,
      { sillyEyeScale: 0.7 } as StylizedExpressionMotion,
    ),
    ANIME25D_DEFORMATION_MOUTH |
      ANIME25D_DEFORMATION_JAW |
      ANIME25D_DEFORMATION_STYLIZED |
      ANIME25D_DEFORMATION_TIME,
  )
})

test('dependency plans classify static, eye, mouth, and timed geometry', () => {
  const base = {
    baseRole: 'decoration',
    fade: null,
    shaderGlobalTransform: true,
    localDynamic: true,
    upstreamFeatureKind: null,
    expressionDeformationKind: null,
    mouthDeformationKind: null,
  } as const
  assert.equal(resolveAnime25DDeformationDependencies(base), 0)
  assert.equal(
    resolveAnime25DDeformationDependencies({
      ...base,
      upstreamFeatureKind: 'eye-open-iris',
    }),
    ANIME25D_DEFORMATION_EYE,
  )
  assert.equal(
    resolveAnime25DDeformationDependencies({
      ...base,
      fade: 'mouthManiac',
      mouthDeformationKind: 'continuous',
    }),
    ANIME25D_DEFORMATION_MOUTH |
      ANIME25D_DEFORMATION_JAW |
      ANIME25D_DEFORMATION_STYLIZED,
  )
  assert.equal(
    resolveAnime25DDeformationDependencies({
      ...base,
      fade: 'mouthCry',
      mouthDeformationKind: 'cry',
    }),
    ANIME25D_DEFORMATION_MOUTH |
      ANIME25D_DEFORMATION_JAW |
      ANIME25D_DEFORMATION_TIME,
  )
  assert.equal(
    resolveAnime25DDeformationDependencies({
      ...base,
      expressionDeformationKind: 'lovestruck-heart',
    }),
    ANIME25D_DEFORMATION_EYE | ANIME25D_DEFORMATION_STYLIZED,
  )
  assert.equal(
    resolveAnime25DDeformationDependencies({ ...base, baseRole: 'face' }),
    ANIME25D_DEFORMATION_JAW,
  )
})

test('hidden layers retain changes until their next visible update', () => {
  const plan = createAnime25DLayerDeformationPlan(
    true,
    ANIME25D_DEFORMATION_EYE,
  )
  assert.equal(
    shouldUpdateAnime25DLayerGeometry(
      plan,
      ANIME25D_DEFORMATION_EYE,
      false,
    ),
    false,
  )
  assert.equal(
    shouldUpdateAnime25DLayerGeometry(plan, ANIME25D_DEFORMATION_TIME, true),
    true,
  )
  markAnime25DLayerGeometryUpdated(plan)
  assert.equal(
    shouldUpdateAnime25DLayerGeometry(plan, ANIME25D_DEFORMATION_TIME, true),
    false,
  )
  assert.equal(
    shouldUpdateAnime25DLayerGeometry(plan, ANIME25D_DEFORMATION_EYE, true),
    true,
  )
})

test('uncacheable layers continue to update every visible frame', () => {
  const plan = createAnime25DLayerDeformationPlan(false, 0)
  assert.equal(shouldUpdateAnime25DLayerGeometry(plan, 0, true), true)
  markAnime25DLayerGeometryUpdated(plan)
  assert.equal(shouldUpdateAnime25DLayerGeometry(plan, 0, true), true)
})

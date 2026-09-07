import assert from 'node:assert/strict'
import test from 'node:test'
import {
  anime25DRuntimeKey,
  intersectionKeepsAnime25DVisible,
  resolveAnime25DRenderSurface,
  shouldAnimateAnime25D,
  shouldApplyAnime25DResize,
  shouldUseAnime25DRuntime,
} from './runtimePolicy'

test('falls back only for the exact runtime asset that failed', () => {
  const first = anime25DRuntimeKey('asset-a', 13, '/atlas-a.png')
  assert.equal(
    shouldUseAnime25DRuntime({
      hasManifest: true,
      hasPlayback: true,
      atlasUrl: '/atlas-a.png',
      runtimeKey: first,
      failedRuntimeKey: first,
    }),
    false,
  )
  const replacement = anime25DRuntimeKey('asset-b', 13, '/atlas-b.png')
  assert.equal(
    shouldUseAnime25DRuntime({
      hasManifest: true,
      hasPlayback: true,
      atlasUrl: '/atlas-b.png',
      runtimeKey: replacement,
      failedRuntimeKey: first,
    }),
    true,
  )
})

test('zero-size boxes do not resize the WebGL canvas', () => {
  assert.equal(shouldApplyAnime25DResize(0, 160), false)
  assert.equal(shouldApplyAnime25DResize(120, 0), false)
  assert.equal(shouldApplyAnime25DResize(Number.NaN, 160), false)
  assert.equal(shouldApplyAnime25DResize(120, 160), true)
})

test('zero-size intersection stays visible until layout has a box', () => {
  assert.equal(
    intersectionKeepsAnime25DVisible({
      isIntersecting: false,
      boundingClientRect: { width: 0, height: 0 },
    }),
    true,
  )
  assert.equal(
    intersectionKeepsAnime25DVisible({
      isIntersecting: false,
      boundingClientRect: { width: 120, height: 160 },
    }),
    false,
  )
  assert.equal(
    intersectionKeepsAnime25DVisible({
      isIntersecting: true,
      boundingClientRect: { width: 120, height: 160 },
    }),
    true,
  )
})

test('runs frames only after the atlas is ready and the canvas is visible', () => {
  const active = {
    atlasReady: true,
    pageVisible: true,
    inViewport: true,
    cancelled: false,
  }
  assert.equal(shouldAnimateAnime25D(active), true)
  for (const key of ['atlasReady', 'pageVisible', 'inViewport'] as const) {
    assert.equal(shouldAnimateAnime25D({ ...active, [key]: false }), false)
  }
  assert.equal(shouldAnimateAnime25D({ ...active, cancelled: true }), false)
})

test('sizes the backing surface to the fitted CSS display', () => {
  assert.deepEqual(
    resolveAnime25DRenderSurface({
      sourceWidth: 2048,
      sourceHeight: 1024,
      cssWidth: 512,
      cssHeight: 512,
      devicePixelRatio: 2,
    }),
    {
      bufferWidth: 1024,
      bufferHeight: 512,
      displayWidth: 512,
      displayHeight: 256,
    },
  )
})

test('preserves the old source-resolution ceiling when enlarged', () => {
  assert.deepEqual(
    resolveAnime25DRenderSurface({
      sourceWidth: 2048,
      sourceHeight: 1024,
      cssWidth: 4096,
      cssHeight: 4096,
      devicePixelRatio: 4,
    }),
    {
      bufferWidth: 4096,
      bufferHeight: 2048,
      displayWidth: 4096,
      displayHeight: 2048,
    },
  )
})

test('keeps full-size rendering unchanged at the supported DPR', () => {
  assert.deepEqual(
    resolveAnime25DRenderSurface({
      sourceWidth: 1000,
      sourceHeight: 500,
      cssWidth: 1000,
      cssHeight: 500,
      devicePixelRatio: 2,
    }),
    {
      bufferWidth: 2000,
      bufferHeight: 1000,
      displayWidth: 1000,
      displayHeight: 500,
    },
  )
})

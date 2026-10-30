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

test('shrinks the backing store slower than the CSS box', () => {
  assert.deepEqual(
    resolveAnime25DRenderSurface({
      sourceWidth: 2048,
      sourceHeight: 1024,
      cssWidth: 512,
      cssHeight: 512,
      devicePixelRatio: 2,
    }),
    {
      bufferWidth: 1261,
      bufferHeight: 630,
      displayWidth: 512,
      displayHeight: 256,
    },
  )
})

test('keeps extra backing pixels on a small face without reaching source size', () => {
  const surface = resolveAnime25DRenderSurface({
    sourceWidth: 2048,
    sourceHeight: 2048,
    cssWidth: 201,
    cssHeight: 201,
    devicePixelRatio: 2,
  })
  assert.equal(surface.displayWidth, 201)
  assert.equal(surface.displayHeight, 201)
  assert.equal(surface.bufferWidth, 569)
  assert.equal(surface.bufferHeight, 569)
  assert.ok(surface.bufferWidth > 201 * 2)
  assert.ok(surface.bufferWidth < 2048 * 2)
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

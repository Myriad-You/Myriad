import assert from 'node:assert/strict'
import test from 'node:test'
import {
  Anime25DPerformanceTelemetry,
  createAnime25DFrameWork,
} from './performanceTelemetry'

test('samples one frame out of every thirty', () => {
  const telemetry = new Anime25DPerformanceTelemetry()
  assert.equal(telemetry.shouldSample(), false)
  telemetry.observe()
  const samples = Array.from({ length: 61 }, () => telemetry.shouldSample())
  assert.deepEqual(
    samples.flatMap((sample, index) => (sample ? [index] : [])),
    [0, 30, 60],
  )
})

test('reports a smoothed and allocation-safe workload snapshot', () => {
  const telemetry = new Anime25DPerformanceTelemetry()
  const work = createAnime25DFrameWork()
  work.deformedLayers = 20
  work.deformedVertices = 4_000
  work.uploadedBytes = 64_000
  work.uploadSubmitMs = 0.8
  work.shaderOnlyLayers = 6
  work.skippedVertices = 800
  work.savedUploadBytes = 6_400
  work.drawnLayers = 18
  work.drawCalls = 20
  telemetry.record({
    ...work,
    frameCpuMs: 8,
    driverMs: 1,
    springsMs: 1,
    deformMs: 5,
    drawSubmitMs: 1,
  })
  telemetry.record({
    ...work,
    frameCpuMs: 4,
    driverMs: 0.5,
    springsMs: 0.5,
    deformMs: 2.5,
    drawSubmitMs: 0.5,
  })

  const snapshot = telemetry.snapshot()
  assert.equal(snapshot.samples, 2)
  assert.equal(snapshot.frameCpuMs, 7)
  assert.equal(snapshot.deformMs, 4.375)
  assert.equal(snapshot.deformedVertices, 4_000)
  assert.equal(snapshot.skippedVertices, 800)
  assert.equal(snapshot.drawCalls, 20)
})

test('sanitizes invalid measurements at the telemetry boundary', () => {
  const telemetry = new Anime25DPerformanceTelemetry()
  telemetry.record({
    frameCpuMs: Number.NaN,
    driverMs: -1,
    springsMs: 0,
    deformMs: 0,
    uploadSubmitMs: 0,
    shaderOnlyLayers: 0,
    skippedVertices: 0,
    savedUploadBytes: 0,
    drawSubmitMs: 0,
    deformedLayers: 0,
    deformedVertices: 0,
    uploadedBytes: 0,
    drawnLayers: 0,
    drawCalls: 0,
  })
  assert.equal(telemetry.snapshot().frameCpuMs, 0)
  assert.equal(telemetry.snapshot().driverMs, 0)
})

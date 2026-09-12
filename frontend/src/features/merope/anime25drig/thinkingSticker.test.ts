import assert from 'node:assert/strict'
import test from 'node:test'
import { sampleDizzyEyeTint } from '../rig/dizzyEye'
import { ThinkingSticker } from './thinkingSticker'

test('thinking sticker fades, rotates, reuses GPU resources and releases them', () => {
  let programs = 0
  let draws = 0
  let deleted = 0
  let opacity = 0
  let ink: number[] = []
  let phase = 0
  const gl = {
    createProgram: () => {
      programs++
      return {}
    },
    createVertexArray: () => ({}),
    createShader: () => ({}),
    shaderSource() {},
    compileShader() {},
    attachShader() {},
    linkProgram() {},
    getProgramParameter: () => true,
    deleteShader() {},
    useProgram() {},
    bindVertexArray() {},
    disable() {},
    enable() {},
    blendFunc() {},
    getUniformLocation: (_: unknown, key: string) => key,
    uniform4f: (
      _: unknown,
      x: number,
      y: number,
      size: number,
      alpha: number,
    ) => {
      opacity = alpha
    },
    uniform2f() {},
    uniform3f: (_: unknown, r: number, g: number, b: number) => { ink = [r, g, b] },
    uniform1f: (_: unknown, value: number) => {
      phase = value
    },
    drawArrays: () => {
      draws++
    },
    deleteProgram: () => {
      deleted++
    },
    deleteVertexArray() {},
  } as unknown as WebGL2RenderingContext
  const sticker = new ThinkingSticker()
  sticker.setLinePixels(new Uint8ClampedArray([72, 41, 32, 255, 255, 255, 255, 0]))
  const draw = (t: number, active: number) =>
    sticker.draw(gl, t, active, 100, 100, 20, 500, 500)
  draw(0, 0)
  assert.equal(programs, 0)
  draw(0.05, 1)
  assert.deepEqual(ink, [72 / 255, 41 / 255, 32 / 255])
  sticker.setLinePixels(undefined)
  draw(0.05, 1)
  const fallback = sampleDizzyEyeTint(undefined)
  assert.deepEqual(ink, [fallback.red / 255, fallback.green / 255, fallback.blue / 255])
  assert.ok(opacity > 0 && opacity < 0.5)
  for (let i = 2; i <= 20; i++) draw(i / 20, 1)
  assert.ok(opacity > 0.99)
  assert.equal(programs, 1)
  const oldPhase = phase
  draw(1.05, 1)
  assert.ok(phase > oldPhase)
  for (let i = 22; i <= 60; i++) draw(i / 20, 0)
  const stopped = draws
  draw(3.05, 0)
  assert.equal(draws, stopped)
  sticker.dispose(gl)
  assert.equal(deleted, 1)
})

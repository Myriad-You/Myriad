import assert from 'node:assert/strict'
import test from 'node:test'
import { analyzeMotionAudio } from './audioMotionAnalysis'

test('motion features use physical frequency ranges instead of display bars', () => {
  const wave = Float32Array.from(
    { length: 2048 },
    (_, i) => 0.3 * Math.sin(i / 10),
  )
  const bins = new Float32Array(1024).fill(-Infinity)
  const output = { energy: 0, bass: 0, pulse: 0, presence: 0 }
  bins[4] = -20
  analyzeMotionAudio(wave, bins, 48000, output)
  assert.ok(output.energy > 0.6)
  assert.ok(output.bass > 0.39)
  assert.equal(output.presence, 0)
  bins.fill(-Infinity)
  bins[86] = -20
  analyzeMotionAudio(wave, bins, 48000, output)
  assert.equal(output.bass, 0)
  assert.equal(output.pulse, 0)
  assert.ok(output.presence > 0.39)
})

test('measured silence is finite zero evidence, including invalid bins', () => {
  const output = { energy: 1, bass: 1, pulse: 1, presence: 1 }
  analyzeMotionAudio(
    new Float32Array(2048),
    new Float32Array(1024).fill(NaN),
    44100,
    output,
  )
  assert.deepEqual(output, { energy: 0, bass: 0, pulse: 0, presence: 0 })
})

test('lowering playback volume does not disguise musical dynamics as silence', () => {
  const wave = Float32Array.from(
    { length: 2048 },
    (_, i) => 0.1 * Math.sin(i / 10),
  )
  const bins = new Float32Array(1024).fill(-Infinity)
  bins[4] = -28
  bins[86] = -32
  const loud = analyzeMotionAudio(wave, bins, 48000, {
    energy: 0,
    bass: 0,
    pulse: 0,
    presence: 0,
  })
  for (const volume of [0.5, 0.05, 0.001]) {
    const quiet = analyzeMotionAudio(
      wave.map((value) => value * volume),
      bins.map((value) => value + 20 * Math.log10(volume)),
      48000,
      { energy: 0, bass: 0, pulse: 0, presence: 0 },
      volume,
    )
    for (const key of ['energy', 'bass', 'pulse', 'presence'] as const)
      assert.ok(Math.abs(quiet[key] - loud[key]) < 1e-6, key)
  }
})

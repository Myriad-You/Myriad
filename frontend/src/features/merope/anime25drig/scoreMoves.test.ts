import type { BodyControl, ScoreMove } from '../../../services/agent/types'
import assert from 'node:assert/strict'
import test from 'node:test'
import { SCORE_MOVES } from '../events/performanceContract'
import { scoreMoveOffsets, scoreMoveSeconds } from './scoreMoves'

function move(kind: ScoreMove['kind'], fields: Partial<ScoreMove> = {}): ScoreMove {
  return { kind, amount: 0.8, count: 1, tempo: 1, ...fields }
}

function sample(m: ScoreMove, at: number) {
  const out: Partial<Record<BodyControl, number>> = {}
  const envelope = scoreMoveOffsets(m, at, out)
  return { out, envelope }
}

test('every move starts and ends at rest and only moves the controls the contract names', () => {
  for (const kind of Object.keys(SCORE_MOVES) as ScoreMove['kind'][]) {
    for (const side of ['left', 'right', 'both'] as const) {
      const m = move(kind, { side, direction: 'up-left', count: 2 })
      const seconds = scoreMoveSeconds(m)
      assert.ok(seconds > 0.2 && seconds < 8, `${kind} ${seconds}`)
      const touched = new Set<string>()
      let peak = 0
      for (let t = 0; t <= seconds; t += 1 / 120) {
        const { out } = sample(m, t)
        for (const [control, value] of Object.entries(out)) {
          if (Math.abs(value!) > 1e-6) touched.add(control)
          peak = Math.max(peak, Math.abs(value!))
        }
      }
      assert.ok(peak > 0.05, `${kind} moves`)
      for (const control of touched) assert.ok(SCORE_MOVES[kind].controls.includes(control as BodyControl), `${kind} ${control}`)
      for (const edge of [0, seconds - 1e-4, seconds + 0.1]) {
        const { out } = sample(m, edge)
        for (const value of Object.values(out)) assert.ok(Math.abs(value!) < 0.02, `${kind} at ${edge}: ${value}`)
      }
    }
  }
})

test('a nod dips the head, repeats smaller, and goes quicker with tempo', () => {
  const twice = move('nod', { count: 2 })
  const first = Math.min(...Array.from({ length: 50 }, (_, i) => sample(twice, i / 100).out.headNod ?? 0))
  const second = Math.min(...Array.from({ length: 50 }, (_, i) => sample(twice, 0.46 + i / 100).out.headNod ?? 0))
  assert.ok(first < -0.3 && second > first && second < -0.2, `${first} ${second}`)
  assert.ok(scoreMoveSeconds(move('nod', { tempo: 1.6 })) < scoreMoveSeconds(move('nod')))
})

test('a wink and a hand beat take one side; a glance looks its way', () => {
  const wink = sample(move('wink', { side: 'left' }), 0.2).out
  assert.ok((wink.eyeOpenLeft ?? 0) < -0.9 && wink.eyeOpenRight === undefined)
  const beat = sample(move('beat'), 0.12).out
  assert.ok((beat.rightArmRaise ?? 0) > 0.2 && beat.leftArmRaise === undefined, 'a hand beat defaults to one hand')
  const glance = sample(move('glance', { direction: 'down-left' }), 0.3).out
  assert.ok((glance.gazeHorizontal ?? 0) < -0.4 && (glance.gazeVertical ?? 0) < -0.4)
})

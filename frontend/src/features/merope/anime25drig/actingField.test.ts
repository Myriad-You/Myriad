import type { BodyControl } from '../../../services/agent/types'
import type { ResolvedScore } from '../motion/scoreTimeline'
import assert from 'node:assert/strict'
import test from 'node:test'
import { ActingField } from './actingField'

const all = () => true
function tilt(id: string, atMs: number, value: number, holdMs = 4000) {
  return { id, atMs, pose: { targets: { headTilt: value }, transitionMs: 300, holdMs } }
}

function run(field: ActingField, from: number, to: number, each?: (time: number) => void, fps = 60) {
  for (let i = 1; i <= Math.round((to - from) * fps); i++) {
    const time = from + i / fps
    field.step(1 / fps, time, all)
    each?.(time)
  }
}
const value = (field: ActingField, control: BodyControl) => field.values.get(control)

test('a standing score plays as strongly as she is free: it fades back while she speaks, and returns', () => {
  const field = new ActingField()
  const standing: ResolvedScore = { id: 1, beats: [tilt('1:0', 0, 0.5, 20000)], standing: { loopMs: 0 } }
  field.setScore(standing, 0, 0)
  run(field, 0, 2)
  assert.ok(value(field, 'headTilt')!.weight > 0.95)
  field.setSpeaking(true)
  run(field, 2, 3)
  assert.ok((value(field, 'headTilt')?.weight ?? 0) < 0.05, 'speaking: she is not daydreaming at the same time')
  field.setSpeaking(false)
  let path: number[] = []
  run(field, 3, 6, () => path.push(value(field, 'headTilt')?.weight ?? 0))
  assert.ok(path.at(-1)! > 0.9, 'free again, it comes back')
  assert.ok(path.every((weight, i) => i === 0 || weight >= path[i - 1]! - 1e-9), 'smoothly, without a jump')
  path = []
})

test('a newer standing score crossfades with the older one on the same control', () => {
  const field = new ActingField()
  field.setScore({ id: 1, beats: [tilt('1:0', 0, 0.5, 20000)], standing: { loopMs: 0 } }, 0, 0)
  run(field, 0, 2)
  field.setScore({ id: 2, beats: [tilt('2:0', 2000, -0.5, 20000)], standing: { loopMs: 0 } }, 2, 2000)
  const seen: number[] = []
  run(field, 2, 5, () => seen.push(value(field, 'headTilt')!.value))
  assert.ok(seen.at(-1)! < -0.45)
  const steps = seen.slice(1).map((v, i) => Math.abs(v - seen[i]!))
  assert.ok(Math.max(...steps) < 0.08, `no jump: ${Math.max(...steps)}`)
})

test('a looping standing score plays again each period; reply beats take her over without any hand-off', () => {
  const field = new ActingField()
  field.setScore({ id: 1, beats: [{ id: '1:0', atMs: 500, move: { kind: 'nod', amount: 1, count: 1, tempo: 1 } }], standing: { loopMs: 3000 } }, 0, 0)
  const nods: number[] = []
  run(field, 0, 10, (time) => {
    if ((field.offsets.get('headNod') ?? 0) < -0.3 && (nods.length === 0 || time - nods.at(-1)! > 1)) nods.push(time)
  })
  // Once each period: at 0.5 s and every 3 s after.
  assert.deepEqual(nods.map((t) => Math.round(t)), [1, 4, 7, 10])
  // A reply beat plays from 10 s: the standing nod due at 9.5 + 3 fades back under it.
  field.setScore({ id: 2, beats: [tilt('2:0', 10000, 0.6, 3000)] }, 10, 10000)
  let loudest = 0
  run(field, 12, 13.4, () => { loudest = Math.max(loudest, Math.abs(field.offsets.get('headNod') ?? 0)) })
  assert.ok(loudest < 0.1, `${loudest}`)
})

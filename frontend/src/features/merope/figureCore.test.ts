import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

// figure/ does not depend on core/: it reads copies of the contracts its renderer carries, and
// its own constants are held to the core's Rust here, where both are checked
// out. The copies themselves are checked by scripts/extra/figure-contracts.sh.

function figure(path: string): string {
  return readFileSync(new URL(`../../../../figure/${path}`, import.meta.url), 'utf8')
}

function core(path: string): string {
  return readFileSync(new URL(`../../../../core/${path}`, import.meta.url), 'utf8')
}

function rustList(source: string, name: string): string[] {
  const block = new RegExp(`${RegExp.escape(name)}: &\\[&str\\] = &\\[([^\\]]*)\\]`).exec(source)?.[1]
  assert.ok(block, `${name} missing from the director contract`)
  return Iterator.from(block.matchAll(/"([a-z.]+)"/gi)).map(match => match[1]!).toArray()
}

function figureList(source: string, name: string): string[] {
  const block = new RegExp(`${RegExp.escape(name)} = \\[([^\\]]*)\\]`).exec(source)?.[1]
  assert.ok(block, `${name} missing from the figure`)
  return Iterator.from(block.matchAll(/'([a-z.]+)'/gi)).map(match => match[1]!).toArray()
}

test('the figure\'s behavior functions and sources are the director\'s', () => {
  const rigState = core('crates/myriad-merope/src/rig_state.rs')
  const summary = figure('src/motion/rigStateSummary.ts')
  for (const [rust, own] of [
    ['RIG_STATE_BEHAVIOR_FUNCTIONS', 'BEHAVIOR_FUNCTIONS'],
    ['RIG_STATE_BEHAVIOR_SOURCES', 'BEHAVIOR_SOURCES'],
    ['RIG_STATE_BEHAVIOR_RESOURCES', 'BEHAVIOR_RESOURCES'],
  ] as const) {
    assert.deepEqual(figureList(summary, own).toSorted(), rustList(rigState, rust).toSorted(), own)
  }
})

test('every resource the director names is one the figure can drive, and none is a leg', () => {
  const rigState = core('crates/myriad-merope/src/rig_state.rs')
  const local = figure('src/motion/behaviorResources.ts')
  const declared = rustList(rigState, 'RIG_STATE_BEHAVIOR_RESOURCES')
  assert.ok(declared.length > 0, 'director contract listed no resources')
  for (const resource of declared)
    assert.match(local, new RegExp(`\\| '${RegExp.escape(resource)}'`), resource)
  for (const forbidden of ['legs', 'foot', 'feet', 'locomotion'])
    assert.doesNotMatch(rigState, new RegExp(`body\\.${RegExp.escape(forbidden)}`), forbidden)
})

test('the figure\'s mood bands cut where the core\'s mood_band does', () => {
  const backend = core('crates/myriad-merope/src/affect.rs')
  const frontend = figure('node_modules/@myriad-you/figure-player/src/performance/mood.ts')
  const backendBody = backend.match(/pub fn mood_band\([\s\S]*?\n\}/)?.[0]
  const frontendBody = frontend.match(/export function moodBand\([\s\S]*?\n\}/)?.[0]
  const floor = backend.match(/pub const MOOD_FLOOR:\s*f64\s*=\s*(\d+(?:\.\d+)?);/)
  assert.ok(backendBody, 'core mood_band must be found')
  assert.ok(frontendBody, 'figure moodBand must be found')
  assert.ok(floor, 'core MOOD_FLOOR must be found')

  const backendThresholds = Iterator.from(backendBody.matchAll(/\b(mood|arousal)\s*(<=|<)\s*(MOOD_FLOOR|\d+(?:\.\d+)?)/g))
    .map(([, axis, operator, value]) => ({ axis, operator, value: Number(value === 'MOOD_FLOOR' ? floor[1] : value) }))
    .toArray()
  const frontendThresholds = Iterator.from(frontendBody.matchAll(/\b(v|a)\s*(<=|<)\s*(\d+(?:\.\d+)?)/g))
    .map(([, axis, operator, value]) => ({ axis: axis === 'v' ? 'mood' : 'arousal', operator, value: Number(value) }))
    .toArray()

  assert.equal(backendThresholds.length, 5, 'expected all core band comparisons')
  assert.equal(frontendThresholds.length, 5, 'expected all figure band comparisons')
  assert.deepEqual(frontendThresholds, backendThresholds)
})

test('the renderer\'s rig-state words and phases are the director\'s', () => {
  const rigState = core('crates/myriad-merope/src/rig_state.rs')
  const motion = core('crates/myriad-merope/src/motion.rs')
  const types = figure('node_modules/@myriad-you/figure-player/src/performance/types.ts')
  const union = (name: string): string[] => {
    const body = new RegExp(`export type ${RegExp.escape(name)} =([\\s\\S]*?)\\n\\n`).exec(types)?.[1]
    assert.ok(body, `${name} missing from the renderer's performance types`)
    return Iterator.from(body.matchAll(/'([a-zA-Z-]+)'/g)).map(match => match[1]!).toArray()
  }
  for (const [rust, ts] of [
    ['RIG_STATE_MUSIC_ENERGIES', 'RigMusicEnergy'],
    ['RIG_STATE_BEAT_PHASES', 'RigBeatPhase'],
    ['RIG_STATE_MOTION_STYLES', 'RigMotionStyle'],
    ['RIG_STATE_BEHAVIOR_FUNCTIONS', 'RigBehaviorFunction'],
    ['RIG_STATE_BEHAVIOR_PHASES', 'RigBehaviorPhase'],
  ] as const) {
    assert.deepEqual(union(ts).toSorted(), rustList(rigState, rust).toSorted(), `${ts} against ${rust}`)
  }
  const phases = Iterator.from(/pub fn as_str\(self\)[\s\S]*?\n {4}\}/.exec(motion)?.[0].matchAll(/=> "([a-z]+)"/g) ?? [])
    .map(match => match[1]!).toArray()
  assert.deepEqual(union('PerformancePhase').toSorted(), phases.toSorted())
})

#!/usr/bin/env node
// Her lives, run end to end on the site's models: the multi-day simulations
// (each in a fresh schema of the test database, dropped afterwards) and her
// real recent turns answered again (read from the site's database, nothing
// written). One report of every check and of how she talks, kept under
// target/merope-reports so runs can be compared week to week.
import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const args = process.argv.slice(2)
let compare
let only
// One run of a life says little: a check on the edge holds one time in four.
// Run each a few times and read how often each check holds.
let repeat = 1
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === '--compare') compare = args[++i]
  else if (args[i] === '--only') only = args[++i]
  else if (args[i] === '--repeat') repeat = Number(args[++i])
  else {
    throw new Error('Usage: node scripts/test-merope-lives.mjs [--only NAME] [--repeat N] [--compare EARLIER_REPORT]')
  }
}
if (!Number.isInteger(repeat) || repeat < 1 || repeat > 10) throw new Error('--repeat must be 1-10')
if (!process.env.MYRIAD_MEDIA_TEST_DATABASE_URL) {
  throw new Error('Needs MYRIAD_MEDIA_TEST_DATABASE_URL (the test database; simulations use their own schema in it)')
}

const LIVES = [
  'her_days_with_someone',
  'her_days_after_a_hurt',
  'her_days_the_same_every_day',
  'her_day_in_a_group_a_stretch_at_a_time',
  'her_own_puzzle',
  'her_wish_toward_someone',
  'her_real_talk_answered_again',
].filter((name) => !only || name === only)

const dir = join(root, 'target', 'merope-reports')
mkdirSync(dir, { recursive: true })
const stamp = new Date().toISOString().replace(/[:.]/g, '-')
const report = join(dir, `lives-${stamp}.json`)
// Each life's whole output beside the report, to see why a check failed.
const logs = join(dir, `lives-${stamp}`)
mkdirSync(logs, { recursive: true })

function once(name, round) {
  const result = spawnSync('cargo', [
    'test', '-q', '-p', 'myriad-backend', '--bin', 'myriad-backend', name,
    '--', '--ignored', '--nocapture',
  ], {
    cwd: root,
    env: { ...process.env, MEROPE_REPLAY_N: process.env.MEROPE_REPLAY_N ?? '24' },
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    timeout: 30 * 60 * 1000,
  })
  const out = `${result.stdout ?? ''}${result.stderr ?? ''}`
  const log = join(logs, repeat > 1 ? `${name}-${round}.log` : `${name}.log`)
  writeFileSync(log, out)
  // Checks print as "  ✓ what" / "  ✗ what"; the replay prints how she talks.
  const checks = [...out.matchAll(/^\s+([✓✗]) (.+)$/gmu)].map(([, mark, what]) => ({ what, held: mark === '✓' }))
  const shape = out.match(/^now\s+(.+)$/m)?.[1] ?? null
  return { passed: result.status === 0, status: result.status ?? result.signal, checks, shape, log }
}

/// A life run `repeat` times: how often it passed, and each check held.
function run(name) {
  console.log(`\n[${name}]${repeat > 1 ? ` ×${repeat}` : ''}`)
  const runs = Array.from({ length: repeat }, (_, round) => once(name, round + 1))
  const checks = []
  for (const r of runs) {
    for (const check of r.checks) {
      let seen = checks.find((kept) => kept.what === check.what)
      if (!seen) checks.push((seen = { what: check.what, held: 0, of: 0 }))
      seen.of += 1
      if (check.held) seen.held += 1
    }
  }
  const passed = runs.filter((r) => r.passed).length
  for (const check of checks) {
    const mark = check.held === check.of ? '✓' : check.held === 0 ? '✗' : '~'
    console.log(`  ${mark} ${check.what}${repeat > 1 ? `  (${check.held}/${check.of})` : ''}`)
  }
  const shapes = runs.map((r) => r.shape).filter(Boolean)
  for (const shape of shapes) console.log(`  now: ${shape}`)
  console.log(`  passed ${passed}/${runs.length}`)
  return { name, passed, runs: runs.length, checks, shapes, logs: runs.map((r) => r.log) }
}

const lives = LIVES.map(run)
writeFileSync(report, JSON.stringify({ at: new Date().toISOString(), lives }, null, 2))
console.log(`\nReport: ${report}`)

if (compare) {
  const before = new Map(JSON.parse(readFileSync(resolve(compare), 'utf8')).lives.map((life) => [life.name, life]))
  console.log(`\nCompared with ${compare}:`)
  let changed = 0
  for (const life of lives) {
    const was = before.get(life.name)
    if (!was) continue
    // Earlier reports kept one run, held as true or false.
    const rate = (check) => (typeof check.held === 'boolean' ? Number(check.held) : check.held / check.of)
    const held = new Map(was.checks.map((check) => [check.what, rate(check)]))
    for (const check of life.checks) {
      if (!held.has(check.what)) continue
      const before = held.get(check.what)
      const now = rate(check)
      if (Math.abs(now - before) >= 0.34) {
        changed += 1
        const pct = (value) => `${Math.round(value * 100)}%`
        console.log(`  ${life.name}: ${now < before ? 'HOLDS LESS' : 'holds more'} ${pct(before)} -> ${pct(now)}: ${check.what}`)
      }
    }
    const wasShapes = was.shapes ?? (was.shape ? [was.shape] : [])
    if (life.shapes.length && wasShapes.join() !== life.shapes.join()) {
      console.log(`  ${life.name}: ${wasShapes.join(' | ')} -> ${life.shapes.join(' | ')}`)
    }
  }
  if (changed === 0) console.log('  no check changed')
}
process.exitCode = lives.every((life) => life.passed === life.runs) ? 0 : 1

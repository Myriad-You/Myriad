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
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === '--compare') compare = args[++i]
  else if (args[i] === '--only') only = args[++i]
  else {
    throw new Error('Usage: node scripts/test-merope-lives.mjs [--only NAME] [--compare EARLIER_REPORT]')
  }
}
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

function run(name) {
  console.log(`\n[${name}]`)
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
  writeFileSync(join(logs, `${name}.log`), out)
  // Checks print as "  ✓ what" / "  ✗ what"; the replay prints how she talks.
  const checks = [...out.matchAll(/^\s+([✓✗]) (.+)$/gmu)].map(([, mark, what]) => ({ what, held: mark === '✓' }))
  const shape = out.match(/^now\s+(.+)$/m)?.[1] ?? null
  const passed = result.status === 0
  for (const check of checks) console.log(`  ${check.held ? '✓' : '✗'} ${check.what}`)
  if (shape) console.log(`  now: ${shape}`)
  console.log(`  ${passed ? 'passed' : `FAILED (exit ${result.status ?? result.signal})`}`)
  return { name, passed, checks, shape, log: join(logs, `${name}.log`) }
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
    const held = new Map(was.checks.map((check) => [check.what, check.held]))
    for (const check of life.checks) {
      if (held.has(check.what) && held.get(check.what) !== check.held) {
        changed += 1
        console.log(`  ${life.name}: ${check.held ? 'now holds' : 'NO LONGER holds'}: ${check.what}`)
      }
    }
    if (was.shape !== life.shape && life.shape) console.log(`  ${life.name}: ${was.shape} -> ${life.shape}`)
  }
  if (changed === 0) console.log('  no check changed')
}
process.exitCode = lives.every((life) => life.passed) ? 0 : 1

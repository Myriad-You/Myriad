#!/usr/bin/env node
// Standalone deterministic acceptance. Never loads app credentials or starts UI.
import { spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const database = process.argv.includes('--database')
if (process.argv.slice(2).some((arg) => arg !== '--database')) {
  throw new Error('Usage: node scripts/test-merope-behavior.mjs [--database]')
}
if (database && !process.env.MEROPE_MEMORY_TEST_DATABASE_URL) {
  throw new Error('--database requires MEROPE_MEMORY_TEST_DATABASE_URL (database name: merope_memory_test)')
}
const temporary = mkdtempSync(join(tmpdir(), 'myriad-behavior-'))
const env = { ...process.env, MEROPE_BEHAVIOR_WIRE_PATH: join(temporary, 'wire.json') }
function run(label, command, args, cwd = root) {
  console.log(`\n[${label}]`)
  const result = spawnSync(command, args, { cwd, env, stdio: 'inherit' })
  if (result.error) throw new Error(`${label}: ${result.error.message}`)
  if (result.status !== 0) throw new Error(`${label}: failed (exit ${result.status}, signal ${result.signal ?? 'none'})`)
}
try {
  run('Nonblocking Chat director: coalescing, backpressure and cancellation', 'cargo',
    ['test', '-p', 'myriad-backend', 'chat_director', '--', '--test-threads=1', '--quiet'])
  run('Merope production behavior: perception, speech, director, scheduler and rig', process.execPath,
    ['node_modules/tsx/dist/cli.mjs', '--test', 'src/features/merope/**/*.test.ts', 'src/features/merope/*.test.ts'], join(root, 'frontend'))
  run('Frontend wire → backend Chat and event readers', 'cargo',
    ['test', '-p', 'myriad-backend', 'behavior_contract', '--', '--include-ignored', '--test-threads=1', '--nocapture'])
  env.MEROPE_DELIVERY_WIRE_PATH = join(temporary, 'delivery.json')
  run('Backend parallel director → frontend scheduler → body output', process.execPath,
    ['node_modules/tsx/dist/cli.mjs', '--test', 'src/features/merope/motion/speechDelivery.contract.test.ts'], join(root, 'frontend'))
  run('Memory interpretation and bounded failures (no database)', 'cargo',
    ['test', '-p', 'myriad-backend', 'merope::chat_remember', '--', '--test-threads=1', '--quiet'])
  run('Event gates, policy and opening-intent lifecycle', 'cargo',
    ['test', '-p', 'myriad-backend', 'agent::consciousness', '--', '--test-threads=1', '--quiet'])
  if (database) {
    run('Dedicated database: corrections, stale writes, isolation and concurrent dedup', 'cargo',
      ['test', '-p', 'myriad-backend', 'store::memory_tests', '--', '--ignored', '--test-threads=1', '--quiet'])
  }
  console.log(`\nPASS: deterministic behavior contracts. Database: ${database ? 'passed' : 'NOT RUN (use --database)'}. Model output quality: NOT TESTED.`)
} finally {
  // Only this runner's mkdtemp directory, never workspace files or DB records.
  rmSync(temporary, { recursive: true, force: true })
}

#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const args = process.argv.slice(2)
let mode = 'export'
let selected = false
let replay
let report
for (let i = 0; i < args.length; i += 1) {
  const arg = args[i]
  if (['--export', '--replay', '--live'].includes(arg)) {
    if (selected) throw new Error('Choose only one of --export, --replay, --live')
    selected = true
    mode = arg.slice(2)
    if (mode === 'replay') replay = args[++i]
    if (mode === 'replay' && (!replay || replay.startsWith('--'))) throw new Error('--replay requires an input report')
  } else if (arg === '--report') {
    if (report) throw new Error('--report must appear once')
    report = args[++i]
    if (!report || report.startsWith('--')) throw new Error('--report requires a new output file')
  } else {
    throw new Error('Usage: node scripts/test-merope-semantics.mjs [--export | --replay INPUT | --live] [--report NEW_FILE]')
  }
}
if (mode === 'live') {
  for (const key of ['MEROPE_SEMANTIC_API_KEY', 'MEROPE_SEMANTIC_PROVIDER', 'MEROPE_SEMANTIC_MODEL']) {
    if (!process.env[key]?.trim()) throw new Error(`--live requires ${key}; no application credentials are loaded`)
  }
}
report = report ? resolve(report) : join(mkdtempSync(join(tmpdir(), 'myriad-semantics-')), 'report.json')
const result = spawnSync('cargo', [
  'test', '-p', 'myriad-backend', 'semantic_eval::run_semantic_suite',
  '--', '--ignored', '--test-threads=1', '--nocapture',
], {
  cwd: root,
  env: { ...process.env, MEROPE_SEMANTIC_MODE: mode, MEROPE_SEMANTIC_REPORT: report,
    ...(replay ? { MEROPE_SEMANTIC_REPLAY: resolve(replay) } : {}) },
  stdio: 'inherit',
})
console.log(`\nSemantic ${mode} report target: ${report}`)
if (mode === 'export' && result.status === 0) console.log('Export only: no model was called; not a semantic pass.')
else console.log('Pending review, invalid output and request/behavior failures all prevent a complete pass.')
if (result.error) throw result.error
process.exitCode = result.status ?? 1

#!/usr/bin/env node
import { spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const args = process.argv.slice(2)
let mode = 'export'
let selected = false
let replay
let report
let kind
let id
let compare
let repeat = '1'
const KINDS = [
  'chat', 'memory', 'event', 'motion', 'touch', 'touch-response',
  'wonder', 'found_out', 'inner', 'own_day', 'doing_choice', 'doing_digest',
  'views', 'soup_start', 'soup_judge', 'bits', 'chime', 'stranger_note', 'threads', 'reach_judge',
  'self_story', 'serial_guess', 'wonder_own', 'explore_step', 'explore_compare',
]
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
  } else if (arg === '--kind') {
    kind = args[++i]
    if (!KINDS.includes(kind)) throw new Error(`Unknown --kind (one of ${KINDS.join(', ')})`)
  } else if (arg === '--id') {
    id = args[++i]
    if (!id || id.startsWith('--')) throw new Error('--id requires a case id prefix')
  } else if (arg === '--compare') {
    compare = args[++i]
    if (!compare || compare.startsWith('--')) throw new Error('--compare requires an earlier report')
  } else if (arg === '--repeat') {
    repeat = args[++i]
    if (!['1', '2', '3'].includes(repeat)) throw new Error('--repeat must be 1-3')
  } else {
    throw new Error('Usage: node scripts/test-merope-semantics.mjs [--export | --replay INPUT | --live] [--kind KIND] [--id CASE_PREFIX] [--repeat 1|2|3] [--report NEW_FILE] [--compare EARLIER_REPORT]')
  }
}
// Reports are kept (target/ is ignored by git), so runs can be compared over time.
if (!report) {
  const dir = join(root, 'target', 'merope-reports')
  mkdirSync(dir, { recursive: true })
  const stamp = new Date().toISOString().replace(/[:.]/g, '-')
  report = join(dir, `semantic-${mode}-${stamp}.json`)
}
report = resolve(report)
const result = spawnSync('cargo', [
  'test', '-p', 'myriad-backend', 'semantic_eval::run_semantic_suite',
  '--', '--ignored', '--test-threads=1', '--nocapture',
], {
  cwd: join(root, 'backend'),
  env: { ...process.env, MEROPE_SEMANTIC_MODE: mode, MEROPE_SEMANTIC_REPORT: report,
    MEROPE_SEMANTIC_REPEAT: repeat,
    ...(kind ? { MEROPE_SEMANTIC_KIND: kind } : {}),
    ...(id ? { MEROPE_SEMANTIC_ID: id } : {}),
    ...(replay ? { MEROPE_SEMANTIC_REPLAY: resolve(replay) } : {}) },
  stdio: 'inherit',
})
console.log(`\nSemantic ${mode} report target: ${report}`)
if (mode === 'export' && result.status === 0) console.log('Export only: no model was called; not a semantic pass.')
else console.log('Pending review, invalid output and request/behavior failures all prevent a complete pass.')
if (compare) {
  try {
    const grades = (path) => new Map(JSON.parse(readFileSync(path, 'utf8')).rows.map((row) => [row.id, row.grade]))
    const before = grades(resolve(compare))
    const after = grades(report)
    const changed = [...after].filter(([caseId, grade]) => before.has(caseId) && before.get(caseId) !== grade)
    const tally = (map) => [...map.values()].reduce((counts, grade) => ({ ...counts, [grade]: (counts[grade] ?? 0) + 1 }), {})
    console.log(`\nCompared with ${compare}:`)
    console.log('  before', JSON.stringify(tally(before)))
    console.log('  now   ', JSON.stringify(tally(after)))
    for (const [caseId, grade] of changed) console.log(`  ${caseId}: ${before.get(caseId)} -> ${grade}`)
    if (changed.length === 0) console.log('  no case changed grade')
  } catch (error) {
    console.log(`\nCould not compare with ${compare}: ${error.message}`)
  }
}
if (result.error) throw result.error
process.exitCode = result.status ?? 1

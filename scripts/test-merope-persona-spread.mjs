#!/usr/bin/env node
// Do randomly drawn personas still sound different? Live and opt-in: spends
// Pro on persona drafts and Lite on replies and judging. See
// backend/src/services/agent/persona_spread.rs.
import { spawnSync } from 'node:child_process'
import { mkdirSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const args = process.argv.slice(2)
let personas = '5'
let conditions = ''
let drafters = ''
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === '--personas') personas = args[++i]
  else if (args[i] === '--conditions') conditions = args[++i]
  else if (args[i] === '--drafters') drafters = args[++i]
  else throw new Error('Usage: node scripts/test-merope-persona-spread.mjs [--personas 2-8] [--conditions A,...,N] [--drafters source:model,...]')
}
if (!/^[2-8]$/.test(personas)) throw new Error('--personas must be 2-8')
if (conditions && !/^[A-N](,[A-N])*$/i.test(conditions)) throw new Error('--conditions takes letters A-N, comma separated')
const dir = join(root, 'target', 'merope-reports')
mkdirSync(dir, { recursive: true })
const report = join(dir, `persona-spread-${new Date().toISOString().replace(/[:.]/g, '-')}.json`)
const result = spawnSync('cargo', [
  'test', '-p', 'myriad-backend', 'persona_spread::personas_sound_apart',
  '--', '--ignored', '--nocapture',
], {
  cwd: join(root, 'backend'),
  env: { ...process.env, MEROPE_SPREAD_PERSONAS: personas, MEROPE_SPREAD_REPORT: report, MEROPE_SPREAD_CONDITIONS: conditions, MEROPE_SPREAD_DRAFTERS: drafters },
  stdio: 'inherit',
})
console.log(`\nPersona spread report: ${report}`)
console.log('Measured, not graded: read the replies before trusting the numbers.')
if (result.error) throw result.error
process.exitCode = result.status ?? 1

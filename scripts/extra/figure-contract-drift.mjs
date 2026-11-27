// What a contract in core/ has that the figure's older copy of it lacks: object keys and
// list entries, by JSON path (new cue intents, face presets, body controls, cases…).
// Printed, and appended to the job summary when CI gives one.
// usage: node figure-contract-drift.mjs <core file> <copy>
import { appendFileSync, readFileSync } from 'node:fs'
import process from 'node:process'

const [corePath, copyPath] = process.argv.slice(2)
const core = JSON.parse(readFileSync(corePath, 'utf8'))
const copy = JSON.parse(readFileSync(copyPath, 'utf8'))
const added = []
const changed = []

function walk(path, ours, theirs) {
  if (Array.isArray(ours) && Array.isArray(theirs)) {
    const known = new Set(theirs.map(item => JSON.stringify(item)))
    for (const item of ours) {
      if (!known.has(JSON.stringify(item))) added.push(`${path}[] ${typeof item === 'object' ? JSON.stringify(item).slice(0, 120) : JSON.stringify(item)}`)
    }
  } else if (ours && typeof ours === 'object' && theirs && typeof theirs === 'object' && !Array.isArray(theirs)) {
    for (const key of Object.keys(ours)) {
      if (!Object.hasOwn(theirs, key)) added.push(`${path}.${key}`)
      else walk(`${path}.${key}`, ours[key], theirs[key])
    }
  } else if (JSON.stringify(ours) !== JSON.stringify(theirs)) {
    changed.push(`${path}: ${JSON.stringify(theirs)} → ${JSON.stringify(ours)}`)
  }
}
walk('$', core, copy)

const lines = [
  `### ${copyPath} is older than ${corePath}`,
  '',
  added.length ? 'In core, not in the copy (the figure drops or rejects these):' : 'Nothing in core is missing from the copy.',
  ...added.map(item => `- \`${item}\``),
  ...(changed.length ? ['', 'Values that differ:', ...changed.map(item => `- \`${item}\``)] : []),
  '',
]
console.log(lines.join('\n'))
if (process.env.GITHUB_STEP_SUMMARY) appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${lines.join('\n')}\n`)

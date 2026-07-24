#!/usr/bin/env node

import { runCli } from '../src/cli.mjs'

try {
  process.exitCode = await runCli(process.argv.slice(2))
} catch (error) {
  const message = error instanceof Error ? error.message : String(error)
  if (process.argv.slice(2).includes('--json')) {
    console.log(JSON.stringify({ error: { code: 'execution-error', message }, exitCode: 1 }))
  } else {
    console.error(`myriad-tapp: ${message}`)
  }
  process.exitCode = 1
}

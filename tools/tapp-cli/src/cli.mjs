import { createProject, inspectProject, packProject } from './project.mjs'
import { readFile } from 'node:fs/promises'

const { version: VERSION } = JSON.parse(
  await readFile(new URL('../package.json', import.meta.url), 'utf8'),
)

const COMMANDS = {
  init: {
    usage: 'myriad-tapp init [directory] [options]',
    summary: 'Create a Tapp project. directory defaults to the current directory.',
    options: [
      '--type <page|widget|both>  Starter surface; defaults to page.',
      '--id <id>                  Manifest id.',
      '--name <name>              Display name.',
      '--author <name>            Author name.',
      '--force                    Allow a non-empty target directory.',
      '--json                     stdout is a single JSON object.',
    ],
    allowedOptions: new Set(['type', 'id', 'name', 'author', 'force', 'json']),
    success: 'Project was created and passes validation.',
  },
  check: {
    usage: 'myriad-tapp check [directory] [--json]',
    summary: 'Validate a Tapp project. directory defaults to the current directory.',
    options: ['--json  stdout is a single JSON object containing the inspection report.'],
    allowedOptions: new Set(['json']),
    success: 'Validation succeeded.',
  },
  permissions: {
    usage: 'myriad-tapp permissions [directory] [--json]',
    summary: 'List declared and statically inferred permissions.',
    options: ['--json  stdout is a single JSON object containing the inspection report.'],
    allowedOptions: new Set(['json']),
    success: 'Permissions were inspected without validation errors.',
  },
  pack: {
    usage: 'myriad-tapp pack [directory] [--out file.tapp] [--json]',
    summary: 'Validate and package a Tapp project. directory defaults to the current directory.',
    options: [
      '-o, --out <path>  Archive path; defaults to dist/{manifest.id}.tapp.',
      '--json             stdout is a single JSON object containing the package result.',
    ],
    allowedOptions: new Set(['out', 'json']),
    success: 'Validation succeeded and the archive was written.',
  },
}

const HELP = `Myriad Tapp CLI

Usage:
  myriad-tapp init [directory] [options]
  myriad-tapp check [directory] [--json]
  myriad-tapp permissions [directory] [--json]
  myriad-tapp pack [directory] [--out file.tapp] [--json]

Run "myriad-tapp <command> --help" for command options, JSON output, and exit status.

Global options:
  -h, --help     Show help.
  -v, --version  Show the CLI version.
`

class UsageError extends Error {}

function commandHelp(command) {
  const spec = COMMANDS[command]
  if (!spec) return HELP.trimEnd()
  return `Myriad Tapp CLI

Usage:
  ${spec.usage}

${spec.summary}

Options:
  ${spec.options.join('\n  ')}
  -h, --help          Show this command help.

Exit status:
  0  ${spec.success}
  1  Project validation or packaging failed.
  2  Command-line usage error.`
}

function parseArguments(args) {
  const options = {}
  const positional = []
  for (let index = 0; index < args.length; index += 1) {
    const value = args[index]
    if (value === '--json') options.json = true
    else if (value === '--force') options.force = true
    else if (value === '--help' || value === '-h') options.help = true
    else if (value === '--version' || value === '-v') options.version = true
    else if (['--type', '--id', '--name', '--author', '--out', '-o'].includes(value)) {
      const next = args[index + 1]
      if (!next || next.startsWith('-')) throw new UsageError(`${value} requires a value`)
      options[value === '-o' ? 'out' : value.slice(2)] = next
      index += 1
    } else if (value.startsWith('-')) {
      throw new UsageError(`Unknown option: ${value}`)
    } else positional.push(value)
  }
  return { options, positional }
}

function validateCommand(command, options, positional) {
  const spec = COMMANDS[command]
  if (!spec) return
  if (positional.length > 1) throw new UsageError(`${command} accepts at most one directory`)
  for (const option of Object.keys(options)) {
    if (option === 'help' || option === 'version') continue
    if (!spec.allowedOptions.has(option)) {
      throw new UsageError(`--${option} is only valid with ${Object.entries(COMMANDS)
        .filter(([, value]) => value.allowedOptions.has(option))
        .map(([name]) => name)
        .join(' or ')}`)
    }
  }
}

function printUsageError(io, error, json) {
  const message = error instanceof Error ? error.message : String(error)
  if (json) io.stdout(JSON.stringify({ error: { code: 'usage-error', message }, exitCode: 2 }))
  else io.stderr(`myriad-tapp: ${message}`)
  return 2
}

function formatLocation(item) {
  return `${item.file || 'manifest.json'}${item.line ? `:${item.line}:${item.column || 1}` : ''}`
}

function printDiagnostics(report, io) {
  for (const item of report.diagnostics) {
    io.stdout(
      `${formatLocation(item)} ${item.severity.toUpperCase()} ${item.code} ${item.message}`,
    )
  }
  const errors = report.diagnostics.filter(({ severity }) => severity === 'error').length
  const warnings = report.diagnostics.filter(({ severity }) => severity === 'warning').length
  io.stdout(
    `${errors === 0 ? 'OK' : 'FAILED'} ${report.manifest?.id || report.root}: ${errors} error(s), ${warnings} warning(s)`,
  )
}

function printPermissions(report, io) {
  const declared = new Map(
    report.permissions.declared.map((entry) => [entry.permission, entry]),
  )
  const required = new Map(
    report.permissions.required.map((entry) => [entry.permission, entry]),
  )
  const names = [...new Set([...declared.keys(), ...required.keys()])].sort()
  if (names.length === 0) {
    io.stdout('No permissions declared or inferred.')
    return
  }
  for (const permission of names) {
    const request = declared.get(permission)
    const need = required.get(permission)
    const state = request ? (need ? 'declared+used' : 'declared') : 'MISSING'
    const level = request?.level || need?.level || 'unknown'
    const reasons = need?.reasons?.join('; ') || 'manifest declaration only'
    io.stdout(`${state.padEnd(13)} ${level.padEnd(10)} ${permission}  ${reasons}`)
  }
}

function defaultIo() {
  return {
    stdout: (line) => console.log(line),
    stderr: (line) => console.error(line),
  }
}

export async function runCli(argv, providedIo = defaultIo()) {
  const io = providedIo
  const command = argv[0]
  let options
  let positional
  try {
    ;({ options, positional } = parseArguments(argv.slice(command ? 1 : 0)))
    validateCommand(command, options, positional)
  } catch (error) {
    return printUsageError(io, error, argv.includes('--json'))
  }

  if (
    !command ||
    options.help ||
    command === 'help' ||
    command === '--help' ||
    command === '-h'
  ) {
    io.stdout(commandHelp(command === 'help' ? positional[0] : command))
    return 0
  }
  if (
    options.version ||
    command === 'version' ||
    command === '--version' ||
    command === '-v'
  ) {
    io.stdout(VERSION)
    return 0
  }

  if (command === 'init') {
    const result = await createProject(positional[0] || '.', options)
    const report = await inspectProject(result.root)
    if (options.json) io.stdout(JSON.stringify({ result, report }, null, 2))
    else {
      io.stdout(`Created ${result.type} Tapp at ${result.root}`)
      printDiagnostics(report, io)
    }
    return report.diagnostics.some(({ severity }) => severity === 'error') ? 1 : 0
  }

  if (command === 'check' || command === 'permissions') {
    const report = await inspectProject(positional[0] || '.')
    if (options.json) io.stdout(JSON.stringify(report, null, 2))
    else if (command === 'permissions') printPermissions(report, io)
    else printDiagnostics(report, io)
    return report.diagnostics.some(({ severity }) => severity === 'error') ? 1 : 0
  }

  if (command === 'pack') {
    try {
      const result = await packProject(positional[0] || '.', options.out)
      if (options.json) {
        io.stdout(
          JSON.stringify(
            {
              outputPath: result.outputPath,
              sizeBytes: result.sizeBytes,
              entries: result.entries,
              diagnostics: result.report.diagnostics,
            },
            null,
            2,
          ),
        )
      } else {
        printDiagnostics(result.report, io)
        io.stdout(
          `Packed ${result.entries} file(s), ${result.sizeBytes} bytes -> ${result.outputPath}`,
        )
      }
      return 0
    } catch (error) {
      if (error.report) {
        if (options.json) io.stdout(JSON.stringify(error.report, null, 2))
        else printDiagnostics(error.report, io)
        return 1
      }
      throw error
    }
  }

  if (options.json) {
    return printUsageError(io, new UsageError(`Unknown command: ${command}`), true)
  }
  io.stderr(`Unknown command: ${command}`)
  io.stderr('Run myriad-tapp --help for usage.')
  return 2
}

export { HELP }

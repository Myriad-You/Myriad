import { createProject, inspectProject, packProject } from './project.mjs'

const HELP = `Myriad Tapp CLI

Usage:
  myriad-tapp init [directory] [--type page|widget|both]
  myriad-tapp check [directory] [--json]
  myriad-tapp permissions [directory] [--json]
  myriad-tapp pack [directory] [--out file.tapp] [--json]

Options:
  --id <id>          Tapp id used by init
  --name <name>      Display name used by init
  --author <name>    Author name used by init
  --force            Allow init in a non-empty directory
  --json             Emit machine-readable output
  -o, --out <path>   Package output path
  -h, --help         Show help
  -v, --version      Show version
`

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
      if (!next || next.startsWith('-')) throw new Error(`${value} requires a value`)
      options[value === '-o' ? 'out' : value.slice(2)] = next
      index += 1
    } else if (value.startsWith('-')) {
      throw new Error(`Unknown option: ${value}`)
    } else positional.push(value)
  }
  return { options, positional }
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
  const { options, positional } = parseArguments(argv.slice(command ? 1 : 0))

  if (
    !command ||
    options.help ||
    command === 'help' ||
    command === '--help' ||
    command === '-h'
  ) {
    io.stdout(HELP.trimEnd())
    return 0
  }
  if (
    options.version ||
    command === 'version' ||
    command === '--version' ||
    command === '-v'
  ) {
    io.stdout('0.1.0')
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

  io.stderr(`Unknown command: ${command}`)
  io.stderr('Run myriad-tapp --help for usage.')
  return 2
}

export { HELP }

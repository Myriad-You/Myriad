import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { spawnSync } from 'node:child_process'
import { after, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const bin = join(packageRoot, 'bin/myriad-tapp.mjs')
const root = await mkdtemp(join(tmpdir(), 'myriad-tapp-cli-'))
const project = join(root, 'starter')

after(async () => {
  await rm(root, { recursive: true })
})

function run(args) {
  return spawnSync(process.execPath, [bin, ...args], {
    cwd: packageRoot,
    encoding: 'utf8',
  })
}

describe('CLI adapter', () => {
  it('exposes an npx-inferable package binary', async () => {
    const packageJson = JSON.parse(
      await readFile(join(packageRoot, 'package.json'), 'utf8'),
    )
    assert.equal(packageJson.bin['tapp-cli'], 'bin/myriad-tapp.mjs')
    assert.deepEqual(packageJson.files, ['bin', 'src', 'README.md'])
    assert.equal(packageJson.publishConfig.access, 'public')
  })

  it('supports global help and version flags', () => {
    const help = run(['--help'])
    assert.equal(help.status, 0)
    assert.match(help.stdout, /Myriad Tapp CLI/)

    const version = run(['--version'])
    assert.equal(version.status, 0)
    assert.equal(version.stdout.trim(), '0.1.0')
  })

  it('documents each command for agents and rejects unsupported options', () => {
    const initHelp = run(['init', '--help'])
    assert.equal(initHelp.status, 0)
    assert.match(initHelp.stdout, /Usage:\n  myriad-tapp init/)
    assert.match(initHelp.stdout, /--type <page\|widget\|both>/)
    assert.doesNotMatch(initHelp.stdout, /--out <path>/)

    const checkHelp = run(['check', '--help'])
    assert.equal(checkHelp.status, 0)
    assert.match(checkHelp.stdout, /Exit status:\n  0  Validation succeeded/)
    assert.match(checkHelp.stdout, /--json.*stdout is a single JSON object/)

    const invalid = run(['check', '.', '--type', 'page'])
    assert.equal(invalid.status, 2)
    assert.match(invalid.stderr, /--type is only valid with init/)
  })

  it('emits a structured JSON error when a JSON command cannot run', () => {
    const failed = run(['init', packageRoot, '--json'])
    assert.equal(failed.status, 1)
    assert.equal(failed.stderr, '')
    assert.deepEqual(JSON.parse(failed.stdout), {
      error: {
        code: 'execution-error',
        message: `Target directory is not empty: ${packageRoot}`,
      },
      exitCode: 1,
    })
  })

  it('emits a structured JSON usage error for an unknown command', () => {
    const failed = run(['unknown', '--json'])
    assert.equal(failed.status, 2)
    assert.equal(failed.stderr, '')
    assert.deepEqual(JSON.parse(failed.stdout), {
      error: {
        code: 'usage-error',
        message: 'Unknown command: unknown',
      },
      exitCode: 2,
    })
  })

  it('reports an existing file as an invalid init target', async () => {
    const target = join(root, 'not-a-directory')
    await writeFile(target, 'existing file\n')

    const failed = run(['init', target, '--json'])
    assert.equal(failed.status, 1)
    assert.equal(failed.stderr, '')
    assert.deepEqual(JSON.parse(failed.stdout), {
      error: {
        code: 'execution-error',
        message: `Target path is not a directory: ${target}`,
      },
      exitCode: 1,
    })
  })

  it('runs init, check, permissions and pack end to end', () => {
    const initialized = run(['init', project, '--type', 'both', '--id', 'com.example.cli'])
    assert.equal(initialized.status, 0, initialized.stderr || initialized.stdout)
    assert.match(initialized.stdout, /Created both Tapp/)

    const checked = run(['check', project, '--json'])
    assert.equal(checked.status, 0, checked.stderr || checked.stdout)
    const report = JSON.parse(checked.stdout)
    assert.equal(report.manifest.id, 'com.example.cli')
    assert.deepEqual(report.permissions.missing, [])

    const permissions = run(['permissions', project])
    assert.equal(permissions.status, 0, permissions.stderr || permissions.stdout)
    assert.match(permissions.stdout, /ui:notification/)
    assert.match(permissions.stdout, /widget:register/)

    const packed = run(['pack', project, '--json'])
    assert.equal(packed.status, 0, packed.stderr || packed.stdout)
    const archive = JSON.parse(packed.stdout)
    // manifest + core.js + page/index.js + widget/index.js + styles.css + page.html
    // + 两个 widget 模板
    assert.equal(archive.entries, 8)
    assert.match(archive.outputPath, /com\.example\.cli\.tapp$/)
  })
})

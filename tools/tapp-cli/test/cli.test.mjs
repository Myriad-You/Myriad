import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
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
    assert.equal(archive.entries, 6)
    assert.match(archive.outputPath, /com\.example\.cli\.tapp$/)
  })
})

import assert from 'node:assert/strict'
import { execFile } from 'node:child_process'
import { access, mkdir, mkdtemp, readFile, rm, symlink, truncate, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { afterEach, describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { promisify } from 'node:util'
import {
  createProject,
  generatedContract,
  inspectProject,
  packProject,
  permissionCatalog,
} from '../src/project.mjs'
import { listZipEntries } from '../src/zip.mjs'
import { parseCapabilitySource } from '../scripts/capability-source.mjs'
import { findMyriadRepoRoot } from '../scripts/myriad-source.mjs'
import { parsePermissionSource } from '../scripts/permission-source.mjs'

const directories = []
const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const myriadRoot = findMyriadRepoRoot(packageRoot)
const upstreamIt = myriadRoot ? it : it.skip
const execFileAsync = promisify(execFile)

async function temporaryDirectory(name) {
  const root = await mkdtemp(join(tmpdir(), `myriad-tapp-${name}-`))
  directories.push(root)
  return root
}

afterEach(async () => {
  await Promise.all(directories.splice(0).map((root) => rm(root, { recursive: true })))
})

describe('Tapp project core', () => {
  it('ships a generated permission catalog', () => {
    const catalog = permissionCatalog()
    assert.ok(Object.keys(catalog.actions).length > 200)
    assert.ok(Object.keys(catalog.permissionLevels).length > 30)
  })

  upstreamIt('keeps the generated contract aligned with the backend exporter', async () => {
    const manifestPath = resolve(myriadRoot, 'tools/tapp-contract-export/Cargo.toml')
    const { stdout } = await execFileAsync(
      'cargo',
      ['run', '--quiet', '--locked', '--manifest-path', manifestPath],
      { maxBuffer: 4 * 1024 * 1024 },
    )
    const backendContract = JSON.parse(stdout)
    const current = generatedContract()
    assert.deepEqual(current.schema, backendContract.schema)
    assert.deepEqual(current.limits, backendContract.limits)
    assert.deepEqual(current.rules, backendContract.rules)
    assert.deepEqual(current.patterns, backendContract.patterns)
  })

  upstreamIt('keeps the generated catalog aligned with permissionConfig', async () => {
    const source = await readFile(
      resolve(myriadRoot, 'frontend/src/tapp/runtime/permissionConfig.ts'),
      'utf8',
    )
    const { permissionLevels, actions } = parsePermissionSource(source)
    assert.deepEqual(permissionCatalog().permissionLevels, permissionLevels)
    assert.deepEqual(permissionCatalog().actions, actions)
  })

  upstreamIt('keeps capability profiles aligned with capabilityProfiles.ts', async () => {
    const source = await readFile(
      resolve(
        myriadRoot,
        'frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts',
      ),
      'utf8',
    )
    const expected = parseCapabilitySource(source)
    const current = generatedContract().capabilities
    assert.deepEqual(current, expected)
    assert.ok(current.headlessDeniedActions.includes('ui.confirm'))
    await access(resolve(packageRoot, 'src/generated/manifest.schema.json'))
    await access(resolve(packageRoot, 'src/generated/capability-profiles.json'))
    const sdkDts = await readFile(
      resolve(packageRoot, 'src/generated/tapp-sdk.d.ts'),
      'utf8',
    )
    assert.match(sdkDts, /export interface TappSdk/)
    assert.match(sdkDts, /declare const Tapp: TappSdk/)
    assert.match(sdkDts, /showNotification/)
    assert.match(sdkDts, /federation/)
  })

  it('keeps generated manifest schema metadata for CLI consumers', async () => {
    const schema = JSON.parse(
      await readFile(resolve(packageRoot, 'src/generated/manifest.schema.json'), 'utf8'),
    )
    assert.equal(schema.title, 'Myriad Tapp Manifest')
    assert.equal(
      schema.description,
      'Generated from backend TappManifest schema. Semantic limits and permission rules live in contract.json.',
    )
    assert.equal(schema.$defs.TappApiDef.properties.bodyMode.default, 'json')
    assert.equal(generatedContract().limits.tappNonJsonHttpRequestBytes, 1024 * 1024)
    assert.ok(generatedContract().rules.httpOnlyApiFields.includes('bodyMode'))
    assert.ok(schema.properties.credentials)
    assert.ok(schema.$defs.TappApiDef.properties.credential)
    assert.ok(schema.$defs.TappApiAccess.enum.includes('manager'))
    assert.equal(generatedContract().limits.tappCredentials, 16)
    assert.equal(generatedContract().limits.credentialValueLength, 16 * 1024)
    assert.ok(generatedContract().rules.httpOnlyApiFields.includes('credential'))
    assert.ok(generatedContract().rules.forbiddenOutboundHeaders.includes('host'))
    assert.ok(generatedContract().rules.forbiddenOutboundHeaders.includes('content-length'))
  })

  upstreamIt('parses contract sources independently of quote style', async () => {
    const permissionSource = await readFile(
      resolve(myriadRoot, 'frontend/src/tapp/runtime/permissionConfig.ts'),
      'utf8',
    )
    const capabilitySource = await readFile(
      resolve(
        myriadRoot,
        'frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts',
      ),
      'utf8',
    )

    const quotedPermissionSource = permissionSource.replace(
      /\['lifecycle\.ready', 'public'\]/,
      '["lifecycle.ready", "public"]',
    )
    const quotedCapabilitySource = capabilitySource.replace(
      "'ui.showNotification'",
      '"ui.showNotification"',
    )

    assert.deepEqual(
      parsePermissionSource(quotedPermissionSource),
      parsePermissionSource(permissionSource),
    )
    assert.deepEqual(
      parseCapabilitySource(quotedCapabilitySource),
      parseCapabilitySource(capabilitySource),
    )
  })

  upstreamIt('rejects contract sources with syntax errors', async () => {
    const permissionSource = await readFile(
      resolve(myriadRoot, 'frontend/src/tapp/runtime/permissionConfig.ts'),
      'utf8',
    )
    const capabilitySource = await readFile(
      resolve(
        myriadRoot,
        'frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts',
      ),
      'utf8',
    )

    assert.throws(() => parsePermissionSource(`${permissionSource}\nconst =`))
    assert.throws(() => parseCapabilitySource(`${capabilitySource}\nconst =`))
  })

  for (const type of ['page', 'widget', 'both']) {
    it(`creates a valid ${type} starter`, async () => {
      const root = await temporaryDirectory(type)
      await createProject(root, { type, id: `com.example.${type}` })
      const report = await inspectProject(root)
      assert.deepEqual(
        report.diagnostics.filter(({ severity }) => severity === 'error'),
        [],
      )
      await access(join(root, 'types/tapp-sdk.d.ts'))
      await access(join(root, 'jsconfig.json'))
      const main = await readFile(join(root, 'main.js'), 'utf8')
      assert.match(main, /reference path="\.\/types\/tapp-sdk\.d\.ts"/)
    })
  }

  it('reports strict fields, undeclared APIs and missing permissions', async () => {
    const root = await temporaryDirectory('invalid')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions = []
    manifest.typoField = true
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await writeFile(
      join(root, 'main.js'),
      `Tapp.storage.set('key', 'value');\nTapp.api('missing', {});\n`,
    )

    const report = await inspectProject(root)
    const codes = new Set(report.diagnostics.map(({ code }) => code))
    assert.ok(codes.has('unknown-manifest-field'))
    assert.ok(codes.has('undeclared-api'))
    assert.ok(codes.has('missing-permission'))
    assert.ok(report.permissions.missing.some(({ permission }) => permission === 'storage'))
  })

  it('warns when page HTML or JS loads an engine from a CDN', async () => {
    const root = await temporaryDirectory('remote-engine')
    await createProject(root, { type: 'page' })
    await writeFile(
      join(root, 'page.html'),
      '<canvas id="scene"></canvas>\n<script src="https://unpkg.com/three@0.170.0/build/three.min.js"></script>\n',
    )
    await writeFile(
      join(root, 'main.js'),
      `import * as THREE from 'https://esm.sh/three'\n`,
    )

    const report = await inspectProject(root)
    const remote = report.diagnostics.filter(({ code }) => code === 'remote-engine-script')
    assert.ok(remote.some((item) => item.file === 'page.html'))
    assert.ok(remote.some((item) => item.file === 'main.js'))
    assert.equal(remote.every((item) => item.severity === 'warning'), true)
  })

  it('uses the TypeScript AST for calls without matching comments or strings', async () => {
    const root = await temporaryDirectory('ast-analysis')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions = ['storage']
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await writeFile(
      join(root, 'main.js'),
      `// Tapp.ui.confirm('not a call')
const example = "Tapp.api('not-real')"
const apiName = 'dynamic'
Tapp?.storage?.get('key')
Tapp.api(apiName)
`,
    )

    const report = await inspectProject(root)
    assert.deepEqual(report.permissions.usedActions.map(({ action }) => action), ['storage.get'])
    assert.equal(report.diagnostics.filter(({ code }) => code === 'dynamic-api-name').length, 1)
    assert.equal(report.diagnostics.some(({ code }) => code === 'undeclared-api'), false)
    assert.equal(report.permissions.required.some(({ permission }) => permission === 'ui:confirm'), false)
  })

  it('reports generated actions with three or more path segments', async () => {
    const root = await temporaryDirectory('nested-action')
    await createProject(root, { type: 'page' })
    await writeFile(
      join(root, 'main.js'),
      `Tapp.ai.tasks.create({})
Tapp.ui.confirm.call(null, 'ok')
`,
    )

    const report = await inspectProject(root)
    assert.ok(
      report.permissions.usedActions.some(
        ({ action, permission, file }) =>
          action === 'ai.tasks.create' && permission === 'public' && file === 'main.js',
      ),
    )
    assert.ok(report.permissions.missing.some(({ permission }) => permission === 'ui:confirm'))
  })

  it('parses TypeScript and reports source syntax errors with locations', async () => {
    const root = await temporaryDirectory('typescript-analysis')
    await createProject(root, { type: 'page' })
    await writeFile(
      join(root, 'helper.ts'),
      `const key: string = 'key'
Tapp.storage.get(key)
`,
    )
    await writeFile(join(root, 'broken.ts'), 'const value: = 1\n')

    const report = await inspectProject(root)
    assert.ok(report.permissions.usedActions.some(({ action, file }) => action === 'storage.get' && file === 'helper.ts'))
    const syntax = report.diagnostics.find(({ code }) => code === 'invalid-source-syntax')
    assert.equal(syntax.file, 'broken.ts')
    assert.equal(syntax.line, 1)
    assert.ok(syntax.column > 0)
  })

  it('returns diagnostics instead of crashing on malformed collection fields', async () => {
    const root = await temporaryDirectory('malformed-collections')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.assets = { invalid: true }
    manifest.widgets = { invalid: true }
    manifest.dataExchange = { exports: { invalid: true }, imports: 'invalid' }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    const codes = new Set(report.diagnostics.map(({ code }) => code))
    assert.ok(codes.has('invalid-assets'))
    assert.ok(codes.has('invalid-widgets'))
    assert.ok(codes.has('invalid-data-exchange'))
  })

  it('accepts omitted permissions when no capability requires one', async () => {
    const root = await temporaryDirectory('optional-permissions')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    delete manifest.permissions
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await writeFile(join(root, 'main.js'), '// no privileged capabilities\n')

    const report = await inspectProject(root)
    assert.deepEqual(
      report.diagnostics.filter(({ severity }) => severity === 'error'),
      [],
    )
  })

  it('validates and packages Agent interaction schemas', async () => {
    const root = await temporaryDirectory('agent')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('ai:chat')
    manifest.ai = {
      protocolVersion: 2,
      operations: ['chat'],
      modelTier: 'standard',
      outputFormats: ['text'],
    }
    manifest.agent = {
      protocolVersion: 2,
      interactions: [
        {
          type: 'report.create',
          inputSchema: 'schemas/report-input.json',
          resultSchema: 'schemas/report-result.json',
        },
      ],
      intents: ['report.create'],
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await mkdir(join(root, 'schemas'))
    await writeFile(join(root, 'schemas/report-input.json'), '{"type":"object"}\n')
    await writeFile(join(root, 'schemas/report-result.json'), '{"type":"object"}\n')

    const report = await inspectProject(root)
    assert.deepEqual(
      report.diagnostics.filter(({ severity }) => severity === 'error'),
      [],
    )
    assert.ok(report.packageFiles.includes('schemas/report-input.json'))
    assert.ok(report.packageFiles.includes('schemas/report-result.json'))
  })

  it('rejects declared resources that are symbolic links', async () => {
    const root = await temporaryDirectory('symlink-resource')
    const targetRoot = await temporaryDirectory('symlink-target')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.assets = ['assets/linked.txt']
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await mkdir(join(root, 'assets'))
    const target = join(targetRoot, 'outside.txt')
    await writeFile(target, 'outside project')
    await symlink(target, join(root, 'assets/linked.txt'))

    const report = await inspectProject(root)
    assert.ok(report.diagnostics.some(({ code }) => code === 'invalid-resource'))
    await assert.rejects(packProject(root), /Project validation failed/)
  })

  it('rejects declared resources that escape through a symbolic-link directory', async () => {
    const root = await temporaryDirectory('symlink-directory-resource')
    const targetRoot = await temporaryDirectory('symlink-directory-target')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.main = 'linked/outside.js'
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await writeFile(join(targetRoot, 'outside.js'), 'Tapp.lifecycle.ready()\n')
    await symlink(targetRoot, join(root, 'linked'))

    const report = await inspectProject(root)
    assert.ok(report.diagnostics.some(({ code }) => code === 'invalid-resource'))
    await assert.rejects(packProject(root), /Project validation failed/)
  })

  it('rejects package resource directories that are symbolic links', async () => {
    const root = await temporaryDirectory('symlink-package-directory')
    const targetRoot = await temporaryDirectory('symlink-package-directory-target')
    await createProject(root, { type: 'page' })
    await writeFile(join(targetRoot, 'en-US.json'), '{"hello": "world"}\n')
    await symlink(targetRoot, join(root, 'i18n'))

    const report = await inspectProject(root)
    assert.ok(report.diagnostics.some(({ code }) => code === 'invalid-i18n'))
    assert.equal(report.packageFiles.some((path) => path.startsWith('i18n/')), false)
    await assert.rejects(packProject(root), /Project validation failed/)
  })

  it('reports a diagnostic when a package resource path is not a directory', async () => {
    const root = await temporaryDirectory('package-directory-file')
    await createProject(root, { type: 'page' })
    await writeFile(join(root, 'i18n'), 'not a directory\n')

    const report = await inspectProject(root)
    assert.ok(report.diagnostics.some(({ code }) => code === 'invalid-i18n'))
    await assert.rejects(packProject(root), /Project validation failed/)
  })

  it('rejects HTTP methods outside the fixed allow-list', async () => {
    const root = await temporaryDirectory('http-method')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.apis = {
      allowed: { type: 'http', endpoint: 'https://example.com', method: 'POST' },
      rejected: { type: 'http', endpoint: 'https://example.com', method: 'FETCH' },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    const invalidApis = report.diagnostics.filter(({ code }) => code === 'invalid-api')
    assert.equal(invalidApis.length, 1)
    assert.match(invalidApis[0].message, /API rejected HTTP method/)
  })

  it('accepts raw and form declared API body modes', async () => {
    const root = await temporaryDirectory('http-body-modes')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.apis = {
      submit: {
        type: 'http',
        endpoint: 'https://example.com/submit',
        method: 'POST',
        bodyMode: 'raw',
        headers: { 'Content-Type': 'text/plain; charset=utf-8' },
        body: '{{params.body}}',
      },
      token: {
        type: 'http',
        endpoint: 'https://example.com/oauth/token',
        method: 'POST',
        bodyMode: 'form',
        body: { grant_type: 'client_credentials', scope: '{{params.scope}}' },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    assert.deepEqual(
      report.diagnostics.filter(({ code }) => code === 'invalid-api'),
      [],
    )
  })

  it('accepts write-only credentials bound to a fixed HTTPS API origin', async () => {
    const root = await temporaryDirectory('api-credential')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.credentials = [{ key: 'wegame', label: 'WeGame API Key' }]
    manifest.apis = {
      games: {
        type: 'http',
        access: 'public',
        endpoint: 'https://api.example.com/games/{{params.id}}',
        credential: {
          key: 'wegame',
          header: 'Authorization',
          prefix: 'Bearer ',
        },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    assert.deepEqual(
      report.diagnostics.filter(({ code }) => code.includes('credential')),
      [],
    )
  })

  it('accepts signed-body and query credential placements', async () => {
    const root = await temporaryDirectory('api-credential-placements')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.settings = [{ key: 'userId', label: 'Creator ID', type: 'input' }]
    manifest.credentials = [
      { key: 'afdianToken', label: 'Afdian token' },
      { key: 'owm', label: 'OpenWeather key' },
    ]
    manifest.apis = {
      sponsors: {
        type: 'http',
        access: 'public',
        method: 'POST',
        endpoint: 'https://afdian.com/api/open/query-sponsor',
        body: {
          user_id: '{{settings.userId}}',
          params: '{"page":1,"per_page":20}',
        },
        credential: {
          key: 'afdianToken',
          in: 'sign',
          field: 'sign',
          sign: {
            alg: 'md5-sorted-kv',
            over: ['params', 'ts', 'user_id'],
            timestampField: 'ts',
          },
        },
      },
      weather: {
        type: 'http',
        endpoint: 'https://api.openweathermap.org/data/2.5/weather?q={{params.city}}',
        credential: { key: 'owm', in: 'query', field: 'appid' },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    assert.deepEqual(
      report.diagnostics.filter(({ code }) => code.includes('credential')),
      [],
    )
  })

  it('rejects signed credentials on GET and object over fields', async () => {
    const root = await temporaryDirectory('api-credential-sign-shape')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.credentials = [{ key: 'afdianToken', label: 'Afdian token' }]
    manifest.apis = {
      sponsors: {
        type: 'http',
        endpoint: 'https://afdian.com/api/open/query-sponsor',
        body: {
          user_id: 'abc',
          params: { page: 1 },
        },
        credential: {
          key: 'afdianToken',
          in: 'sign',
          field: 'sign',
          sign: {
            alg: 'md5-sorted-kv',
            over: ['params', 'user_id'],
          },
        },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    const messages = report.diagnostics
      .filter(({ code }) => code.includes('credential'))
      .map(({ message }) => message)
      .join('\n')
    assert.match(messages, /signed credentials require one of/)
    assert.match(messages, /must be a scalar/)
  })

  it('rejects form credentials on GET and without an object body', async () => {
    const root = await temporaryDirectory('api-credential-form-shape')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.credentials = [{ key: 'wegame', label: 'WeGame API Key' }]
    manifest.apis = {
      games: {
        type: 'http',
        endpoint: 'https://api.example.com/submit',
        bodyMode: 'form',
        credential: { key: 'wegame', in: 'form', field: 'token' },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    const messages = report.diagnostics
      .filter(({ code }) => code.includes('credential'))
      .map(({ message }) => message)
      .join('\n')
    assert.match(messages, /form credentials require one of/)
    assert.match(messages, /form credentials require a form object body/)
  })

  it('rejects credential bindings with a templated destination host', async () => {
    const root = await temporaryDirectory('api-credential-host')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.credentials = [{ key: 'wegame', label: 'WeGame API Key' }]
    manifest.apis = {
      games: {
        type: 'http',
        endpoint: 'https://{{params.host}}/games',
        credential: { key: 'wegame', header: 'X-Api-Key' },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    assert.ok(
      report.diagnostics.some(
        ({ code, message }) =>
          code === 'invalid-api-credential' && message.includes('fixed absolute HTTPS'),
      ),
    )
  })

  it('rejects credentials that reuse a public setting key', async () => {
    const root = await temporaryDirectory('credential-setting-conflict')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.settings = [{ key: 'wegame', label: 'Legacy public key', type: 'input' }]
    manifest.credentials = [{ key: 'wegame', label: 'WeGame API Key' }]
    manifest.apis = {
      games: {
        type: 'http',
        endpoint: 'https://api.example.com/games',
        credential: { key: 'wegame', header: 'X-Api-Key' },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    assert.ok(
      report.diagnostics.some(
        ({ code }) => code === 'credential-setting-conflict',
      ),
    )
  })

  it('aligns credential binding diagnostics with installer constraints', async () => {
    const root = await temporaryDirectory('api-credential-constraints')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.credentials = [
      { key: 'wegame', label: 'WeGame API Key' },
      { key: 'unused', label: 'Unused Key' },
      { key: 'unicodePrefix', label: 'Unicode Prefix' },
    ]
    manifest.apis = {
      duplicate: {
        type: 'http',
        endpoint: 'https://api.example.com/games',
        headers: { authorization: 'literal' },
        credential: { key: 'wegame', header: 'Authorization' },
      },
      forbidden: {
        type: 'http',
        endpoint: 'https://api.example.com/games',
        credential: { key: 'missing', header: 'Host' },
      },
      oversizedPrefix: {
        type: 'http',
        endpoint: 'https://api.example.com/games',
        credential: {
          key: 'unicodePrefix',
          header: 'X-Api-Key',
          prefix: '密'.repeat(100),
        },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    const messages = report.diagnostics
      .filter(({ code }) => code.includes('credential'))
      .map(({ message }) => message)
      .join('\n')
    assert.match(messages, /declares its credential header twice/)
    assert.match(messages, /references an undeclared credential/)
    assert.match(messages, /credential header is invalid or forbidden/)
    assert.match(messages, /credential prefix is invalid/)
    assert.match(messages, /Credential unused must be bound/)
  })

  it('rejects invalid declared API body mode shapes', async () => {
    const root = await temporaryDirectory('invalid-http-body-modes')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.permissions.push('network:fetch')
    manifest.apis = {
      unknown: {
        type: 'http',
        endpoint: 'https://example.com/unknown',
        method: 'POST',
        bodyMode: 'binary',
        body: 'payload',
      },
      rawGet: {
        type: 'http',
        endpoint: 'https://example.com/raw',
        method: 'GET',
        bodyMode: 'raw',
        body: 'payload',
      },
      rawObject: {
        type: 'http',
        endpoint: 'https://example.com/raw',
        method: 'POST',
        bodyMode: 'raw',
        body: { payload: true },
      },
      formNested: {
        type: 'http',
        endpoint: 'https://example.com/form',
        method: 'POST',
        bodyMode: 'form',
        body: { scopes: ['read', 'write'] },
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    const messages = report.diagnostics
      .filter(({ code }) => code === 'invalid-api')
      .map(({ message }) => message)
      .join('\n')
    assert.match(messages, /API unknown bodyMode/)
    assert.match(messages, /API rawGet bodyMode raw requires/)
    assert.match(messages, /API rawObject raw body must be a string/)
    assert.match(messages, /API formNested form body fields must be scalar/)
  })

  it('treats bodyMode as HTTP-only for builtin APIs', async () => {
    const root = await temporaryDirectory('builtin-http-body-mode')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.apis = {
      geo: {
        type: 'builtin',
        builtin: 'geo',
        bodyMode: 'json',
      },
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    const messages = report.diagnostics
      .filter(({ code }) => code === 'invalid-api')
      .map(({ message }) => message)
      .join('\n')
    assert.match(messages, /Builtin API geo contains HTTP-only fields/)
  })

  it('rejects manifests over the manifest byte limit', async () => {
    const root = await temporaryDirectory('oversized-manifest')
    await createProject(root, { type: 'page' })
    const manifestPath = join(root, 'manifest.json')
    const raw = await readFile(manifestPath, 'utf8')
    await writeFile(manifestPath, raw + ' '.repeat(generatedContract().limits.manifestBytes))

    const report = await inspectProject(root)
    assert.ok(report.diagnostics.some(({ code }) => code === 'manifest-too-large'))
  })

  it('rejects directory-walk package files over the resource byte limit', async () => {
    const root = await temporaryDirectory('oversized-walk')
    await createProject(root, { type: 'page' })
    await mkdir(join(root, 'schemas'))
    // A sparse file is enough: the walk only checks the reported size.
    await writeFile(join(root, 'schemas/big.json'), '')
    await truncate(join(root, 'schemas/big.json'), generatedContract().limits.resourceBytes + 1)

    const report = await inspectProject(root)
    assert.ok(
      report.diagnostics.some(
        ({ code, message }) => code === 'resource-too-large' && message.includes('schemas/big.json'),
      ),
    )
  })

  it('rejects unsupported nested fields and invalid i18n resources', async () => {
    const root = await temporaryDirectory('nested')
    await createProject(root, { type: 'widget' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.widgets[0].refreshPolicy = { mode: 'interval', intervalSeconds: 10, typo: true }
    manifest.ai = {
      protocolVersion: 1,
      operations: ['image'],
      modelTier: 'unknown',
      contextSources: [],
      outputFormats: ['text'],
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await mkdir(join(root, 'i18n'))
    await writeFile(join(root, 'i18n/en-US.json'), '["not-an-object"]\n')
    await writeFile(join(root, 'i18n/README.txt'), 'unsupported\n')

    const report = await inspectProject(root)
    const codes = new Set(report.diagnostics.map(({ code }) => code))
    assert.ok(codes.has('unknown-manifest-field'))
    assert.ok(codes.has('invalid-widget-refresh'))
    assert.ok(codes.has('invalid-ai'))
    assert.ok(codes.has('invalid-i18n'))
  })

  it('packs a valid installable file layout', async () => {
    const root = await temporaryDirectory('pack')
    await createProject(root, { type: 'both', id: 'com.example.pack' })
    const result = await packProject(root)
    const archive = await readFile(result.outputPath)
    assert.deepEqual(listZipEntries(archive), [
      'main.js',
      'manifest.json',
      'page.html',
      'styles.css',
      'templates/widget-2x2.html',
      'templates/widget-4x2.html',
    ])
    assert.equal(
      listZipEntries(archive).some((path) => path.startsWith('types/')),
      false,
    )
    assert.ok(archive.subarray(0, 4).equals(Buffer.from([0x50, 0x4b, 0x03, 0x04])))
  })

  it('packs declared resources under types/ while dropping editor scaffolding', async () => {
    const root = await temporaryDirectory('types-resource')
    await createProject(root, { type: 'page', id: 'com.example.types' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.main = 'types/main.js'
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await writeFile(
      join(root, 'types/main.js'),
      await readFile(join(root, 'main.js'), 'utf8'),
    )

    const result = await packProject(root)
    const entries = listZipEntries(await readFile(result.outputPath))
    assert.ok(entries.includes('types/main.js'))
    assert.equal(entries.includes('types/tapp-sdk.d.ts'), false)
    assert.equal(entries.includes('jsconfig.json'), false)
  })

  it('matches Playground main.js composition markers', async () => {
    const { buildMainJs, PACKAGE_MARKERS } = await import('../src/package-layout.mjs')
    const composed = buildMainJs({
      core: 'const shared = 1;',
      widget: 'Tapp.widgets.a = { render() {} };',
      page: 'Tapp.lifecycle.onReady(() => {});',
    })
    assert.ok(composed.includes(PACKAGE_MARKERS.widget))
    assert.ok(composed.includes(PACKAGE_MARKERS.page))
    assert.ok(composed.indexOf('const shared') < composed.indexOf(PACKAGE_MARKERS.widget))
    assert.ok(composed.indexOf(PACKAGE_MARKERS.widget) < composed.indexOf(PACKAGE_MARKERS.page))
  })

  it('rejects declared resources over the general resource byte limit', async () => {
    const root = await temporaryDirectory('oversized-resource')
    await createProject(root, { type: 'page' })
    await writeFile(
      join(root, 'main.js'),
      Buffer.alloc(generatedContract().limits.resourceBytes + 1, 0x20),
    )

    const report = await inspectProject(root)
    assert.ok(
      report.diagnostics.some(
        ({ code, message }) => code === 'resource-too-large' && message.includes('main.js'),
      ),
    )
  })

  it('does not leave an archive behind when the stored ZIP exceeds 25 MiB', async () => {
    const root = await temporaryDirectory('oversized')
    await createProject(root, { type: 'page' })
    await writeFile(join(root, 'main.js'), Buffer.alloc(25 * 1024 * 1024, 0x20))
    const outputPath = join(root, 'oversized.tapp')

    await assert.rejects(packProject(root, outputPath), /Package exceeds 25 MiB/)
    await assert.rejects(access(outputPath), { code: 'ENOENT' })
  })

  it('requires page resources when hasPage is true', async () => {
    const root = await temporaryDirectory('missing-page')
    await createProject(root, { type: 'widget', id: 'com.example.missing-page' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.hasPage = true
    delete manifest.pageTemplate
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)

    const report = await inspectProject(root)
    assert.ok(report.diagnostics.some(({ code }) => code === 'missing-page-resource'))
    assert.equal(report.surfaces.page, true)
  })

  it('errors on headless-denied actions in headless-only projects', async () => {
    const root = await temporaryDirectory('headless-only')
    await createProject(root, { type: 'page', id: 'com.example.headless-only' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    delete manifest.hasPage
    delete manifest.pageTemplate
    delete manifest.widgets
    manifest.backgroundRequirements = ['sync']
    manifest.permissions = []
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await writeFile(
      join(root, 'main.js'),
      `Tapp.ui.confirm('ok');
`,
    )
    await rm(join(root, 'page.html'), { force: true })

    const report = await inspectProject(root)
    assert.equal(report.surfaces.headlessOnly, true)
    assert.ok(
      report.diagnostics.some(
        ({ code, severity }) => code === 'headless-denied-action' && severity === 'error',
      ),
    )
    assert.ok(report.permissions.missing.some(({ permission }) => permission === 'ui:confirm'))
  })

  it('warns when headless-denied actions appear in mixed surface projects', async () => {
    const root = await temporaryDirectory('headless-mixed')
    await createProject(root, { type: 'page', id: 'com.example.headless-mixed' })
    const manifestPath = join(root, 'manifest.json')
    const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
    manifest.backgroundRequirements = ['scheduler']
    if (!manifest.permissions.includes('ui:confirm')) {
      manifest.permissions.push('ui:confirm')
    }
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
    await writeFile(
      join(root, 'main.js'),
      `Tapp.lifecycle.onReady(async function () {
  await Tapp.ui.confirm('ok');
});
`,
    )

    const report = await inspectProject(root)
    assert.equal(report.surfaces.headless, true)
    assert.equal(report.surfaces.headlessOnly, false)
    assert.ok(
      report.diagnostics.some(
        ({ code, severity }) =>
          code === 'headless-unavailable-action' && severity === 'warning',
      ),
    )
    assert.equal(
      report.diagnostics.some(({ code }) => code === 'headless-denied-action'),
      false,
    )
  })
})

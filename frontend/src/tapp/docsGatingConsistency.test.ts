/**
 * Gating consistency between Tapp developer docs and shipped code.
 *
 * Drives real modules (categories, install progress, store paths, permission
 * fixtures) and asserts key doc claim classes still match. Failures mean the
 * docs under docs/development/tapp drifted from code authority.
 */
import assert from 'node:assert/strict'
import { existsSync, readdirSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  storeAssetStorePath,
  storePackageRoot,
} from './utils/storePackagePaths.ts'
import {
  normalizeTappCategory,
  TAPP_CATEGORIES,
} from './utils/tappCategories.ts'
import {
  isLargeTappInstall,
  LARGE_TAPP_INSTALL_BYTES,
} from './utils/tappInstallProgress.ts'
import { resolveTappListInstallRequest } from './utils/tappListInstallRequest.ts'

const HERE = dirname(fileURLToPath(import.meta.url))
/** frontend/src/tapp → repo root */
const REPO = resolve(HERE, '../../..')
const DOCS_TAPP = join(REPO, 'docs/development/tapp')
const DOCS_INDEX = join(REPO, 'docs/development/TAPP_DEVELOPMENT.md')
const FIXTURES = join(DOCS_TAPP, 'fixtures')
const TAPP_STORE_RS = join(REPO, 'backend/src/api/tapp_store.rs')
/** Router assembly (routes moved out of main.rs into router/*). */
const ROUTER_DIR = join(REPO, 'backend/src/router')
const CONTRACT_RULES = join(REPO, 'crates/tapp-contract/src/contract_rules.rs')

function collectRustRoutePathLiterals(dir: string): Set<string> {
  const paths = new Set<string>()
  if (!existsSync(dir)) return paths
  const walk = (d: string) => {
    for (const name of readdirSync(d, { withFileTypes: true })) {
      const p = join(d, name.name)
      if (name.isDirectory()) {
        walk(p)
        continue
      }
      if (!name.name.endsWith('.rs')) continue
      const text = read(p)
      for (const m of text.matchAll(/"(\/api\/tapp(?:\/[^"]*)?)"/g)) {
        if (!m[1].startsWith('/api/tapps')) {
          paths.add(m[1].replace(/\{[^}]+\}/g, '{}'))
        }
      }
    }
  }
  walk(dir)
  return paths
}

function read(path: string): string {
  return readFileSync(path, 'utf8')
}

describe('tapp docs gating consistency', () => {
  it('tAPP_DEVELOPMENT lists every markdown file under docs/development/tapp', () => {
    const index = read(DOCS_INDEX)
    const mdFiles = readdirSync(DOCS_TAPP).filter((n) => n.endsWith('.md'))
    for (const name of mdFiles) {
      assert.ok(
        index.includes(`tapp/${name}`),
        `TAPP_DEVELOPMENT.md must list tapp/${name}`,
      )
    }
    assert.ok(existsSync(join(DOCS_TAPP, 'STORE.md')))
    assert.ok(index.includes('tapp/STORE.md'))
    assert.ok(existsSync(join(FIXTURES, 'README.md')))
    assert.ok(index.includes('tapp/fixtures/README.md'))
  })

  it('core relative links among index docs resolve on disk', () => {
    const cores = [
      DOCS_INDEX,
      join(DOCS_TAPP, 'STORE.md'),
      join(DOCS_TAPP, 'QUICKSTART.md'),
      join(DOCS_TAPP, 'REST_API.md'),
      join(DOCS_TAPP, 'MANIFEST.md'),
      join(DOCS_TAPP, 'ARCHITECTURE.md'),
    ]
    const missing: string[] = []
    for (const file of cores) {
      const text = read(file)
      const re = /\]\(([^)]+)\)/g
      let m: RegExpExecArray | null
      // eslint-disable-next-line no-cond-assign -- standard re.exec loop
      while ((m = re.exec(text))) {
        const href = m[1].split('#')[0].trim()
        if (!href || /^(https?:|mailto:)/i.test(href)) continue
        const target = resolve(dirname(file), href)
        if (!existsSync(target)) {
          missing.push(`${file} -> ${href}`)
        }
      }
    }
    assert.deepEqual(missing, [], `broken relative links:\n${missing.join('\n')}`)
  })

  it('category stable IDs in MANIFEST match TAPP_CATEGORIES and normalize aliases', () => {
    const manifest = read(join(DOCS_TAPP, 'MANIFEST.md'))
    const section = manifest.split('### 应用分类')[1] ?? ''
    const tableChunk = section.split('###')[0] ?? ''
    const ids = [...tableChunk.matchAll(/^\|\s*`([a-z-]+)`\s*\|/gm)].map(
      (m) => m[1],
    )
    assert.deepEqual(
      [...ids].sort(),
      [...TAPP_CATEGORIES].sort(),
      'MANIFEST category table must match TAPP_CATEGORIES',
    )
    assert.equal(normalizeTappCategory('games'), 'game')
    assert.equal(normalizeTappCategory('tools'), 'utility')
    assert.equal(normalizeTappCategory('music'), 'media')
    assert.equal(normalizeTappCategory('development'), 'developer')
  })

  it('store package path helpers match documented examples', () => {
    assert.equal(
      storePackageRoot('apps/com.myriad.doudizhu/main.js'),
      'apps/com.myriad.doudizhu',
    )
    assert.equal(
      storeAssetStorePath(
        'apps/com.myriad.doudizhu',
        'assets/felt/table_felt.png',
      ),
      'apps/com.myriad.doudizhu/assets/felt/table_felt.png',
    )
    const storeDoc = read(join(DOCS_TAPP, 'STORE.md'))
    assert.match(
      storeDoc,
      /apps\/com\.myriad\.doudizhu\/assets\/felt\/table_felt\.png/,
    )
    assert.match(storeDoc, /(?:≥|>=)\s*1\s*MiB/)
    assert.equal(LARGE_TAPP_INSTALL_BYTES, 1024 * 1024)
    assert.equal(isLargeTappInstall(1024 * 1024 - 1), false)
    assert.equal(isLargeTappInstall(1024 * 1024), true)
  })

  it('rEST_API documented /api/tapps method+path pairs exist in create_tapp_routes', () => {
    const rest = read(join(DOCS_TAPP, 'REST_API.md'))
    const storeRs = read(TAPP_STORE_RS)
    const codePaths = new Set<string>()
    for (const m of storeRs.matchAll(
      /\.route\(\s*"([^"]+)"\s*,\s*(get|post|delete|put|patch)\(/g,
    )) {
      const path = m[1]
      const method = m[2].toUpperCase()
      const full =
        path === '/' ? '/api/tapps' : `/api/tapps${path}`.replace(/\{[^}]+\}/g, '{}')
      codePaths.add(`${method} ${full}`)
    }

    const docPairs: string[] = []
    for (const m of rest.matchAll(
      /^\|\s*(GET|POST|DELETE|PUT|PATCH)\s*\|\s*`(\/api\/tapps[^`]*)`/gm,
    )) {
      const method = m[1]
      const path = m[2].split('?')[0].replace(/\{[^}]+\}/g, '{}')
      docPairs.push(`${method} ${path}`)
    }
    assert.ok(docPairs.length >= 20, 'expected substantial /api/tapps table')
    const missing = docPairs.filter((p) => !codePaths.has(p))
    assert.deepEqual(
      missing,
      [],
      `REST_API /api/tapps routes missing from tapp_store.rs:\n${missing.join('\n')}`,
    )
  })

  it('rEST_API documented /api/tapp method+path pairs exist in backend router modules', () => {
    const rest = read(join(DOCS_TAPP, 'REST_API.md'))
    // Routes live under backend/src/router/* (not main.rs).
    const registeredPaths = collectRustRoutePathLiterals(ROUTER_DIR)

    const missing: string[] = []
    for (const m of rest.matchAll(
      /^\|\s*(GET|POST|DELETE|PUT|PATCH|GET \(WS\))\s*\|\s*`(\/api\/tapp[^`]*)`/gm,
    )) {
      const path = m[2].split('?')[0].replace(/\{[^}]+\}/g, '{}')
      if (path.startsWith('/api/tapps')) continue
      if (!registeredPaths.has(path)) {
        missing.push(path)
      }
    }
    assert.deepEqual(
      missing,
      [],
      `REST_API /api/tapp paths missing from backend/src/router:\n${missing.join('\n')}`,
    )
  })

  it('asset size claims in MANIFEST match contract_rules constants', () => {
    const rules = read(CONTRACT_RULES)
    const maxAssets = rules.match(
      /MAX_TAPP_ASSETS:\s*usize\s*=\s*(\d+)/,
    )?.[1]
    const maxBytes = rules.match(
      /MAX_TAPP_ASSET_BYTES:\s*u64\s*=\s*(\d+)\s*\*\s*1024\s*\*\s*1024/,
    )
    const maxTotal = rules.match(
      /MAX_TAPP_ASSETS_TOTAL_BYTES:\s*u64\s*=\s*(\d+)\s*\*\s*1024\s*\*\s*1024/,
    )
    assert.equal(maxAssets, '64')
    assert.equal(maxBytes?.[1], '5')
    assert.equal(maxTotal?.[1], '20')
    const manifest = read(join(DOCS_TAPP, 'MANIFEST.md'))
    assert.match(manifest, /单文件 ≤ 5 MiB/)
    assert.match(manifest, /合计 ≤ 20 MiB/)
    assert.match(manifest, /最多 64 项/)
  })

  it('brew fixture permissions match PERMISSION_MAP for brewList actions', async () => {
    const fixture = JSON.parse(
      read(join(FIXTURES, 'action_permissions.json')),
    ) as {
      actions: Array<{ domain: string; action: string; permission: string }>
    }
    const { PERMISSION_MAP } = await import('./runtime/permissionConfig.ts')
    const brew = fixture.actions.filter((a) => a.domain === 'brew')
    assert.ok(brew.length > 10)
    for (const row of brew) {
      const mapped = PERMISSION_MAP.get(row.action)
      assert.equal(
        mapped,
        row.permission,
        `PERMISSION_MAP[${row.action}] must equal fixture ${row.permission}`,
      )
    }
    const apiRef = read(join(DOCS_TAPP, 'API_REFERENCE.md'))
    // Doc must not claim discover is brew:read
    const brewSection =
      apiRef.split('## Brew 列表 API')[1]?.split('## ')[0] ?? ''
    assert.match(brewSection, /brew:manage/)
    assert.ok(
      !/brew:read`[^`]*discover/.test(brewSection) &&
        !/`brew:read`\s*\|\s*`[^`]*discover/.test(brewSection),
      'API_REFERENCE must not list discover under brew:read',
    )
    assert.match(brewSection, /`discover`/)
  })

  it('docs do not prescribe obsolete /api/tapp-store routes as live API', () => {
    for (const name of ['REST_API.md', 'STORE.md', 'ARCHITECTURE.md']) {
      const text = read(join(DOCS_TAPP, name))
      // Allowed only as explicit negation
      const positives = [
        ...text.matchAll(/\/api\/tapp-store[^\s`]*/g),
      ].filter((m) => {
        const start = Math.max(0, m.index! - 40)
        const ctx = text.slice(start, m.index! + m[0].length + 10)
        return !/不存在|没有|不是|obsolete|removed/i.test(ctx)
      })
      assert.deepEqual(
        positives.map((m) => m[0]),
        [],
        `${name} must not present /api/tapp-store as a live route`,
      )
    }
  })

  it('tappList.install request shapes match resolveTappListInstallRequest (shipped)', () => {
    // Bare numeric source is NOT a valid SDK store install (skeptic gap).
    assert.equal(
      resolveTappListInstallRequest({
        source: '1',
        tappId: 'com.example.app',
      }).kind,
      'error',
    )
    assert.equal(
      resolveTappListInstallRequest({
        source: 'store',
        storeSource: '1',
        tappId: 'com.example.app',
      }).kind,
      'store',
    )
    assert.equal(
      resolveTappListInstallRequest({
        source:
          'https://raw.githubusercontent.com/Myriad-You/tapp-store/main/index.json',
        tappId: 'com.example.app',
      }).kind,
      'store',
    )
    assert.equal(
      resolveTappListInstallRequest({
        source: 'direct',
        manifest: { id: 'com.example.app' },
        code: 'x',
      }).kind,
      'direct',
    )
    assert.equal(
      resolveTappListInstallRequest({ source: 'direct', code: 'x' }).kind,
      'error',
    )

    const apiRef = read(join(DOCS_TAPP, 'API_REFERENCE.md'))
    const listSection =
      apiRef.split('## Tapp 列表 API')[1]?.split('## ')[0] ?? ''
    // Must document canonical store shape with storeSource
    assert.match(listSection, /storeSource:\s*"1"/)
    // Must not claim bare source:"1" is a valid equivalent without marking invalid
    assert.ok(
      !/等价[^\n]*source:\s*"1"/.test(listSection) &&
        !/await Tapp\.tappList\.install\(\{\s*source:\s*"1"/.test(listSection),
      'API_REFERENCE must not present bare source:"1" as a working install example',
    )
    assert.match(listSection, /裸数字|无效|Invalid|不会当作 catalog/)

    const storeDoc = read(join(DOCS_TAPP, 'STORE.md'))
    assert.match(
      storeDoc,
      /\{\s*source:\s*"1"[^}]*\}\s*[|｜].*失败|失败.*source:\s*"1"/s,
    )
    assert.ok(
      !/source` 即 `storeSource`|source 即 storeSource|`source` 即 `storeSource`/.test(
        storeDoc,
      ),
      'STORE.md must not claim SDK source equals storeSource',
    )

    const troubleshoot = read(join(DOCS_TAPP, 'TROUBLESHOOTING.md'))
    assert.match(
      troubleshoot,
      /source:\s*"store"[\s\S]*storeSource:\s*"1"|storeSource:\s*"1"[\s\S]*source:\s*"store"/,
    )
    assert.match(troubleshoot, /裸.*source:\s*"1"|source:\s*"1".*不会/)
  })

  it('contentHandlers wires the shared install resolver (not a fork)', () => {
    const handler = read(
      join(REPO, 'frontend/src/tapp/runtime/sandbox/handlers/contentHandlers.ts'),
    )
    assert.match(handler, /resolveTappListInstallRequest/)
    assert.match(
      handler,
      /from ['"]\.\.\/\.\.\/\.\.\/utils\/tappListInstallRequest['"]/,
    )
  })
})

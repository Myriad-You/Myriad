import type { TappInstance } from './types'
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
import { classifyWidgetLibraryKind } from '../components/widgetLibrarySearch.ts'
import { PERMISSION_LEVELS } from './runtime/permissionConfig.ts'
import {
  generateFullSDK,
  generateWidgetSDK,
} from './runtime/sandbox/sdkGenerator.ts'
import {
  storeAssetStorePath,
  storePackageRoot,
} from './utils/storePackagePaths.ts'
import {
  normalizeTappCategory,
  TAPP_CATEGORIES,
  TAPP_WIDGET_CATEGORIES,
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

  it('widget categories match system Tapp IDs one-to-one', () => {
    assert.deepEqual(
      [...TAPP_WIDGET_CATEGORIES],
      [...TAPP_CATEGORIES],
      'Widget categories must be the same stable IDs as app categories',
    )
    const manifest = read(join(DOCS_TAPP, 'MANIFEST.md'))
    const section =
      manifest.split('### Widget 分类')[1]?.split('### templates')[0] ?? ''
    const ids = [...section.matchAll(/^\|\s*`([a-z-]+)`\s*\|/gm)].map(
      (m) => m[1],
    )
    assert.deepEqual(
      [...ids].sort(),
      [...TAPP_CATEGORIES].sort(),
      'MANIFEST Widget 分类 table must match TAPP_CATEGORIES',
    )
    assert.match(section, /同一套/)
    assert.match(section, /\*\*限制\*\*/)
    assert.match(section, /只能写上表八个/)
    for (const category of TAPP_CATEGORIES) {
      assert.match(section, new RegExp(`\`tapp:${category}\``))
      assert.equal(
        classifyWidgetLibraryKind({
          id: `com.example.${category}`,
          isTappWidget: true,
          category,
        }),
        `tapp:${category}`,
      )
    }
    assert.equal(
      classifyWidgetLibraryKind({
        id: 'com.example.omitted',
        isTappWidget: true,
      }),
      'tapp:utility',
    )

    const widgetDoc = read(join(DOCS_TAPP, 'WIDGET.md'))
    assert.match(widgetDoc, /同一套/)
    assert.match(widgetDoc, /MANIFEST\.md#widget-分类/)
    assert.match(widgetDoc, /\*\*限制\*\*/)

    const apiSection =
      read(join(DOCS_TAPP, 'API_REFERENCE.md'))
        .split('## 小组件 API')[1]
        ?.split('## ')[0] ?? ''
    assert.match(apiSection, /Widget 分类/)
    assert.match(apiSection, /MANIFEST\.md#widget-分类/)
    assert.match(
      apiSection,
      /ai.*data.*developer.*game.*media.*productivity.*social.*utility/,
    )
    assert.match(apiSection, /只能写这八个规范 ID/)

    const playground = read(join(DOCS_TAPP, 'PLAYGROUND_GENERATION_CONTEXT.md'))
    assert.match(playground, /同一套稳定 ID/)
    assert.match(playground, /只能写这些规范值/)

    const generatePrompt = read(
      join(REPO, 'backend/src/api/tapp_playground/types_generate.rs'),
    )
    const promptStart = generatePrompt.indexOf('const PLAYGROUND_SYSTEM_PROMPT')
    const promptEnd = generatePrompt.indexOf('"##;', promptStart)
    const systemPrompt = generatePrompt.slice(promptStart, promptEnd)
    assert.ok(systemPrompt.length > 80, 'PLAYGROUND_SYSTEM_PROMPT must exist')
    assert.match(
      systemPrompt,
      /ai, data, developer, game, media,\s*productivity, social, utility/,
    )
  })

  it('store package path helpers match documented examples', () => {
    assert.equal(
      storePackageRoot('apps/com.myriad.doudizhu/core.js'),
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
    const layout = storeDoc.split('## 仓库布局')[1]?.split(/^## /m)[0] ?? ''
    assert.match(layout, /catalog\.json/)
    assert.match(layout, /scripts\//)
    assert.match(layout, /edge\//)
    assert.match(layout, /development\//)
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
    assert.equal(maxAssets, '128')
    assert.equal(maxBytes?.[1], '16')
    assert.equal(maxTotal?.[1], '64')
    const gameAssets = rules.match(
      /MAX_TAPP_GAME_ASSETS:\s*usize\s*=\s*(\d+)/,
    )?.[1]
    const gameBytes = rules.match(
      /MAX_TAPP_GAME_ASSET_BYTES:\s*u64\s*=\s*(\d+)\s*\*\s*1024\s*\*\s*1024/,
    )
    const gameTotal = rules.match(
      /MAX_TAPP_GAME_ASSETS_TOTAL_BYTES:\s*u64\s*=\s*(\d+)\s*\*\s*1024\s*\*\s*1024/,
    )
    const archiveBytes = rules.match(
      /MAX_TAPP_ARCHIVE_BYTES:\s*usize\s*=\s*(\d+)\s*\*\s*1024\s*\*\s*1024/,
    )
    const gameArchiveBytes = rules.match(
      /MAX_TAPP_GAME_ARCHIVE_BYTES:\s*usize\s*=\s*(\d+)\s*\*\s*1024\s*\*\s*1024/,
    )
    assert.equal(gameAssets, '256')
    assert.equal(gameBytes?.[1], '32')
    assert.equal(gameTotal?.[1], '128')
    assert.equal(archiveBytes?.[1], '64')
    assert.equal(gameArchiveBytes?.[1], '128')
    const manifest = read(join(DOCS_TAPP, 'MANIFEST.md'))
    assert.match(manifest, /单文件 ≤ 16 MiB/)
    assert.match(manifest, /合计 ≤ 64 MiB/)
    assert.match(manifest, /最多 128 项/)
    assert.match(manifest, /单文件 32 MiB \/ 合计 128 MiB \/ 256 项/)
    assert.match(manifest, /普通 64 MiB \/ 合计 128 MiB/)
    assert.match(manifest, /游戏档 128 MiB \/ 合计 256 MiB/)
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
    const names = [
      ...readdirSync(DOCS_TAPP).filter((n) => n.endsWith('.md')),
      join('..', 'TAPP_DEVELOPMENT.md'),
      join('..', '..', 'features', 'TAPP_FILE_FORMAT.md'),
    ]
    for (const name of names) {
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
        modules: { 'core.js': 'x' },
      }).kind,
      'direct',
    )
    assert.equal(
      resolveTappListInstallRequest({
        source: 'direct',
        modules: { 'core.js': 'x' },
      }).kind,
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

  it('MANIFEST permission-table tokens equal the shipped catalog', () => {
    const catalog = Object.keys(PERMISSION_LEVELS).sort()
    const contract = JSON.parse(
      read(join(REPO, 'tools/tapp-cli/src/generated/contract.json')),
    ) as { permissionLevels: Record<string, string> }
    const contractTokens = Object.keys(contract.permissionLevels).sort()
    assert.deepEqual(
      contractTokens,
      catalog,
      'contract.json permissionLevels must equal PERMISSION_LEVELS',
    )

    const manifest = read(join(DOCS_TAPP, 'MANIFEST.md'))
    const section = manifest.split('## 权限列表')[1] ?? ''
    const tokens = [
      ...section.matchAll(/^\|\s*`([a-z0-9:]+)`\s*\|/gim),
    ].map((m) => m[1])
    assert.deepEqual(
      [...tokens].sort(),
      catalog,
      `MANIFEST 权限列表 must list every TappPermission catalog token (missing ${catalog
        .filter((t) => !tokens.includes(t))
        .join(', ')}; extra ${tokens.filter((t) => !catalog.includes(t)).join(', ')})`,
    )
    assert.ok(tokens.includes('ai:search'), 'MANIFEST must list ai:search')
  })

  it('API_REFERENCE capability table includes every frozen full-SDK namespace', () => {
    const gen = read(
      join(REPO, 'frontend/src/tapp/runtime/sandbox/sdkFull.ts'),
    )
    const fullFn = gen.slice(gen.indexOf('export function generateFullSDK'))
    const frozen = [
      ...fullFn.matchAll(/Object\.freeze\(Tapp\.(\w+)/g),
    ].map((m) => m[1])
    const frozenNs = [...new Set(frozen)]
    assert.ok(frozenNs.includes('game'), 'generateFullSDK must freeze Tapp.game')

    const perms = Object.keys(PERMISSION_LEVELS) as never[]
    const instance: TappInstance = {
      id: 'com.example.docs-gate',
      manifest: {
        id: 'com.example.docs-gate',
        name: 'Docs Gate',
        version: '1.0.0',
        core: { entry: 'core.js' },
        permissions: perms,
        category: 'utility',
        game: { protocol: 'session' },
      },
      status: 'running',
      installedAt: '2026-01-01T00:00:00Z',
      grantedPermissions: perms,
      userRole: 'admin',
    }
    const pageSdk = generateFullSDK(instance, 'tok', 'page')
    const widgetSdk = generateWidgetSDK(instance, 'tok')
    assert.match(pageSdk, /sendRequest\('game', 'create'/)
    assert.match(pageSdk, /api:\s*Object\.assign\(/)
    assert.match(widgetSdk, /api:\s*Object\.assign\(/)
    assert.equal(/\n\s+game:\s*\{/.test(widgetSdk), false)

    const apiRef = read(join(DOCS_TAPP, 'API_REFERENCE.md'))
    const cap = apiRef.split('## 能力边界与完整命名空间')[1] ?? ''
    const capUntilNext = cap.split(/^## /m)[0] ?? cap
    const missing = frozenNs.filter(
      (ns) => !new RegExp(`\`${ns}\``).test(capUntilNext),
    )
    assert.deepEqual(
      missing,
      [],
      `API_REFERENCE 能力边界 must mention frozen namespaces: ${missing.join(', ')}`,
    )
    assert.match(capUntilNext, /`game`/)
    assert.match(capUntilNext, /Tapp\.api\(name, params\)/)
    assert.match(capUntilNext, /Tapp\.api\.list\(\)/)
    assert.match(capUntilNext, /headless/)
    assert.match(capUntilNext, /Widget/)
    assert.match(capUntilNext, /Page/)
    assert.match(apiRef, /## Game API/)
    assert.match(apiRef, /\[Game API\]\(#game-api\)/)
  })

  it('examples do not present retired permission names as installable', () => {
    const retired = ['storage', 'federation:write', 'brew:comment']
    const files = [
      ...readdirSync(DOCS_TAPP)
        .filter((n) => n.endsWith('.md'))
        .map((n) => join(DOCS_TAPP, n)),
      DOCS_INDEX,
      join(REPO, 'docs/features/TAPP_FILE_FORMAT.md'),
    ]
    const live: string[] = []
    for (const file of files) {
      const text = read(file)
      for (const token of retired) {
        const escaped = token.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
        // Only permission arrays / permission-table cells, not namespace names.
        const re = new RegExp(
          `permissions[\\s\\S]{0,200}[\`'"]${escaped}[\`'"]`,
          'g',
        )
        for (const m of text.matchAll(re)) {
          if (new RegExp(`[\`'"]${escaped}:[a-zA-Z]+`).test(m[0])) continue
          const start = Math.max(0, (m.index ?? 0) - 40)
          const ctx = text.slice(start, (m.index ?? 0) + m[0].length + 40)
          if (
            /拒绝|已移除|不要再声明|退役|instead|历史文档|旧 `|upgrade|升级说明|不会被解码|明确拒绝/i.test(
              ctx,
            )
          ) {
            continue
          }
          live.push(
            `${file.replace(`${REPO}/`, '')}:${token} :: ${ctx.replace(/\s+/g, ' ').slice(0, 140)}`,
          )
        }
      }
    }
    assert.deepEqual(
      live,
      [],
      `retired tokens presented as live:\n${live.join('\n')}`,
    )
  })

  it('reader-facing docs name the model in CONTEXT words, not generator identifiers', () => {
    const forbidden = [
      'generateFullSDK',
      'generateWidgetSDK',
      'HEADLESS_DENIED_ACTIONS',
      'PERMISSION_MAP',
    ]
    const quickstart = read(join(DOCS_TAPP, 'QUICKSTART.md'))
    const index = read(DOCS_INDEX)
    const playgroundCtx = read(
      join(DOCS_TAPP, 'PLAYGROUND_GENERATION_CONTEXT.md'),
    )
    const widget = read(join(DOCS_TAPP, 'WIDGET.md'))
    const widgetLimit =
      widget.split('## Widget SDK 限制')[1]?.split(/^## /m)[0] ?? ''
    assert.ok(widgetLimit.length > 80, 'WIDGET SDK-limit section must exist')

    const surfaces: Array<[string, string]> = [
      ['QUICKSTART.md', quickstart],
      ['TAPP_DEVELOPMENT.md', index],
      ['WIDGET.md SDK-limit', widgetLimit],
      ['PLAYGROUND_GENERATION_CONTEXT.md', playgroundCtx],
    ]
    const hits: string[] = []
    for (const [label, text] of surfaces) {
      for (const name of forbidden) {
        if (text.includes(name)) hits.push(`${label}: ${name}`)
      }
    }
    assert.deepEqual(
      hits,
      [],
      `reader-facing docs must not teach generator identifiers:\n${hits.join('\n')}`,
    )

    const lifecycle =
      quickstart.split('## 生命周期')[1]?.split(/^## /m)[0] ?? ''
    for (const word of ['隐藏', '销毁', '卸载', '常驻']) {
      assert.ok(
        lifecycle.includes(word),
        `QUICKSTART 生命周期 must contain ${word}`,
      )
    }

    const archRow =
      index
        .split('\n')
        .find((line) => line.includes('tapp/ARCHITECTURE.md')) ?? ''
    for (const word of ['隐藏', '销毁', '卸载', '常驻']) {
      assert.ok(
        archRow.includes(word),
        `TAPP_DEVELOPMENT 架构总览 row must name ${word}: ${archRow}`,
      )
    }

    assert.match(quickstart, /Tapp\.api\(name, params\)/)
    assert.match(widgetLimit, /Tapp\.api\(name, params\)/)
    assert.match(widgetLimit, /Tapp\.api\.list\(\)/)
    assert.match(widgetLimit, /\|\s*Widget\s*\|/)
    assert.match(widgetLimit, /\|\s*Page\s*\|/)
    assert.match(widgetLimit, /headless/)
    assert.ok(
      !/Full SDK \(Page\/headless\)/.test(widgetLimit),
      'WIDGET must not lump Page/headless as one Full SDK',
    )
  })

  it('PLAYGROUND_GENERATION_CONTEXT teaches the current AI task envelope', () => {
    const playgroundCtx = read(
      join(DOCS_TAPP, 'PLAYGROUND_GENERATION_CONTEXT.md'),
    )
    const playground = read(join(DOCS_TAPP, 'PLAYGROUND.md'))

    assert.match(playgroundCtx, /Tapp\.ai\.tasks\.create/)
    assert.match(playgroundCtx, /ai:search/)
    assert.match(
      playgroundCtx,
      /generate[\s\S]{0,80}analyze[\s\S]{0,80}chat[\s\S]{0,80}image[\s\S]{0,80}search/,
    )
    assert.match(playgroundCtx, /contextProvenance/)
    assert.match(playgroundCtx, /task\.result/)
    assert.match(playgroundCtx, /queued/)
    assert.match(playgroundCtx, /Tapp\.ai\.tasks\.get/)
    assert.match(playgroundCtx, /protocolVersion/)
    assert.match(playgroundCtx, /AI_V2_NOT_DECLARED/)
    assert.match(playgroundCtx, /Tapp\.settings/)
    assert.match(playgroundCtx, /Tapp\.shared/)
    assert.match(playgroundCtx, /openUrls/)
    assert.match(playgroundCtx, /manifest\.game/)
    assert.match(playgroundCtx, /--tapp-primary/)
    assert.match(playground, /code\.assets/)
    assert.match(playgroundCtx, /Tapp\.widgets/)
    assert.match(playgroundCtx, /render\(container, props\)/)
    assert.match(playgroundCtx, /#tapp-content/)
    assert.match(playgroundCtx, /仅 Page 预览/)
    assert.match(playground, /仅 Page 预览/)
    assert.ok(
      !/Tapp\.ai\.generate\s*\(/.test(playgroundCtx),
      'generation context must not invent Tapp.ai.generate()',
    )
    assert.match(playgroundCtx, /无条件注入/)
    assert.match(playground, /PLAYGROUND_GENERATION_CONTEXT\.md/)
    assert.match(playground, /无条件注入/)
    assert.match(playground, /contextProvenance/)

    const apiReference = read(join(DOCS_TAPP, 'API_REFERENCE.md'))
    assert.match(
      apiReference,
      /version, locale, theme, features/,
    )
    assert.ok(
      !apiReference.includes('{ version, name, environment }'),
      'API_REFERENCE getApp must match host { version, locale, theme, features }',
    )
  })
})

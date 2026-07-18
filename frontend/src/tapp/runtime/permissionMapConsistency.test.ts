/**
 * Cross-stack consistency: sandbox PERMISSION_MAP (speech / brew / federation)
 * and PERMISSION_LEVELS must match the machine-readable fixtures under
 * docs/development/tapp/fixtures/.
 *
 * Edit the fixtures first, then update permissionConfig.ts / host_attribution.
 * Run from frontend/:
 *   node --experimental-strip-types --test src/tapp/runtime/permissionMapConsistency.test.ts
 */

/* eslint-disable test/no-import-node-test -- node:test; project has no vitest dep */

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'
import { PERMISSION_LEVELS, PERMISSION_MAP } from './permissionConfig.ts'

const __dirname = dirname(fileURLToPath(import.meta.url))
// runtime/ → tapp/ → src/ → frontend/ → repo root
const repoRoot = join(__dirname, '../../../..')
const fixturesDir = join(repoRoot, 'docs/development/tapp/fixtures')

interface ActionEntry {
  domain: string
  action: string
  permission: string
}

interface ActionFixture {
  actions: ActionEntry[]
}

interface HostRouteEntry {
  domain: string
  method: string
  path: string
  permission: string
}

interface HostRouteFixture {
  routes: HostRouteEntry[]
}

const HOSTED_ACTION_PREFIXES = ['speech.', 'brewList.', 'federation.'] as const

function loadJson<T>(name: string): T {
  const raw = readFileSync(join(fixturesDir, name), 'utf8')
  return JSON.parse(raw) as T
}

function isHostedDomainAction(action: string): boolean {
  return HOSTED_ACTION_PREFIXES.some(prefix => action.startsWith(prefix))
}

describe('host-proxied action → permission fixture', () => {
  const actionFixture = loadJson<ActionFixture>('action_permissions.json')
  const hostFixture = loadJson<HostRouteFixture>('host_route_permissions.json')

  it('loads non-empty action and host fixtures', () => {
    assert.ok(actionFixture.actions.length > 0)
    assert.ok(hostFixture.routes.length > 0)
  })

  it('matches PERMISSION_MAP for every fixture action', () => {
    for (const entry of actionFixture.actions) {
      const actual = PERMISSION_MAP.get(entry.action)
      assert.equal(
        actual,
        entry.permission,
        `PERMISSION_MAP[${entry.action}] must be ${entry.permission}, got ${String(actual)}`,
      )
    }
  })

  it('has a fixture row for every speech/brewList/federation PERMISSION_MAP entry', () => {
    const fixtureActions = new Set(actionFixture.actions.map(a => a.action))
    for (const action of PERMISSION_MAP.keys()) {
      if (!isHostedDomainAction(action))
        continue
      assert.ok(
        fixtureActions.has(action),
        `PERMISSION_MAP action ${action} is missing from action_permissions.json`,
      )
    }
  })

  it('uses only permissions present in PERMISSION_LEVELS', () => {
    const levelKeys = new Set(Object.keys(PERMISSION_LEVELS))
    for (const entry of actionFixture.actions) {
      assert.ok(
        levelKeys.has(entry.permission),
        `action ${entry.action} permission ${entry.permission} missing from PERMISSION_LEVELS`,
      )
    }
    for (const entry of hostFixture.routes) {
      assert.ok(
        levelKeys.has(entry.permission),
        `host route ${entry.method} ${entry.path} permission ${entry.permission} missing from PERMISSION_LEVELS`,
      )
    }
  })

  it('shares permission string sets with host routes per domain', () => {
    for (const domain of ['speech', 'brew', 'federation'] as const) {
      const hostPerms = new Set(
        hostFixture.routes
          .filter(r => r.domain === domain)
          .map(r => r.permission),
      )
      const actionPerms = new Set(
        actionFixture.actions
          .filter(a => a.domain === domain)
          .map(a => a.permission),
      )
      assert.deepEqual(
        [...hostPerms].sort(),
        [...actionPerms].sort(),
        `domain ${domain}: host route permission set must equal action permission set`,
      )
    }
  })

  it('has unique action names in the fixture', () => {
    const seen = new Set<string>()
    for (const entry of actionFixture.actions) {
      assert.ok(
        !seen.has(entry.action),
        `duplicate action in action_permissions.json: ${entry.action}`,
      )
      seen.add(entry.action)
    }
  })
})

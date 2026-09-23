/** 前端权限目录须锁定 export_tapp_contract()。 */
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import sandboxContract from '../../../../shared/tapp_sandbox_contract.json' with { type: 'json' }
import { PERMISSION_COPY } from '../constants/permissionCopy.ts'
import { PERMISSION_LEVELS, PERMISSION_MAP } from './permissionConfig.ts'

const __dirname = dirname(fileURLToPath(import.meta.url))
const repoRoot = join(__dirname, '../../../..')

interface ContractExport {
  permissionLevels: Record<string, string>
}

function loadContractExport(): ContractExport {
  const raw = readFileSync(
    join(repoRoot, 'tools/tapp-cli/src/generated/contract.json'),
    'utf8',
  )
  return JSON.parse(raw) as ContractExport
}

function tappPermissionUnionMembers(source: string): string[] {
  const start = source.indexOf('export type TappPermission =')
  assert.ok(start >= 0, 'TappPermission union missing from types/index.ts')
  const rest = source.slice(start)
  const endMatch = rest.match(/\nexport type /)
  assert.ok(endMatch?.index, 'TappPermission union has no following export type')
  const block = rest.slice(0, endMatch.index)
  const members = Iterator.from(block.matchAll(/^\s*\|\s*'([^']+)'/gm))
    .map((match) => match[1])
    .toArray()
  assert.ok(members.length > 30, `TappPermission union too small: ${members.length}`)
  return members
}

describe('permission catalog lock to tapp-contract export', () => {
  const exported = loadContractExport()
  const exportedLevels = exported.permissionLevels

  it('PERMISSION_LEVELS matches export permissionLevels', () => {
    assert.ok(exportedLevels && Object.keys(exportedLevels).length > 30)
    assert.deepEqual(PERMISSION_LEVELS, exportedLevels)
  })

  it('PERMISSION_COPY covers the export catalog', () => {
    assert.deepEqual(
      Object.keys(PERMISSION_COPY).toSorted(),
      Object.keys(exportedLevels).toSorted(),
    )
  })

  it('TappPermission union members match the export catalog', () => {
    const source = readFileSync(
      join(repoRoot, 'frontend/src/tapp/types/index.ts'),
      'utf8',
    )
    const unionMembers = tappPermissionUnionMembers(source)
    const catalogNames = Object.keys(exportedLevels).toSorted()
    assert.deepEqual(unionMembers.toSorted(), catalogNames)
  })

  it('sandbox actions only require public or catalog permissions', () => {
    for (const [action, permission] of PERMISSION_MAP) {
      assert.ok(
        permission === 'public' || permission in exportedLevels,
        `${action} requires unknown permission ${permission}`,
      )
    }
  })

  it('SandboxCapabilityProfile literals match the shared sandbox contract', () => {
    const source = readFileSync(
      join(repoRoot, 'frontend/src/tapp/runtime/sandbox/capabilityProfiles.ts'),
      'utf8',
    )
    const union = source.match(/export type SandboxCapabilityProfile =([^\n]+)/)
    assert.ok(union, 'SandboxCapabilityProfile union missing')
    const literals = Iterator.from(union[1].matchAll(/'([^']+)'/g))
      .map((match) => match[1])
      .toArray()
    assert.deepEqual(literals, sandboxContract.capabilities.profiles)
  })
})

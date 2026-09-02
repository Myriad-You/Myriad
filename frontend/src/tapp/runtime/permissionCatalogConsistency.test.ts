/**
 * Frontend permission catalog must lock to export_tapp_contract().
 *
 * PERMISSION_MAP still locks to host fixtures (permissionMapConsistency.test.ts).
 */
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import { PERMISSION_COPY } from '../constants/permissionCopy.ts'
import { PERMISSION_LEVELS } from './permissionConfig.ts'

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
  const members = [...block.matchAll(/^\s*\|\s*'([^']+)'/gm)].map(match => match[1])
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
      Object.keys(PERMISSION_COPY).sort(),
      Object.keys(exportedLevels).sort(),
    )
  })

  it('TappPermission union members match the export catalog', () => {
    const source = readFileSync(
      join(repoRoot, 'frontend/src/tapp/types/index.ts'),
      'utf8',
    )
    const unionMembers = tappPermissionUnionMembers(source)
    const catalogNames = Object.keys(exportedLevels).sort()
    assert.deepEqual([...unionMembers].sort(), catalogNames)
  })
})

/** 前端读分类须与 export_tapp_contract() 的旧名表一致。 */
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { describe, it } from 'node:test'
import { fileURLToPath } from 'node:url'

import { normalizeTappCategory, TAPP_CATEGORIES } from './tappCategories.ts'

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '../../../..')

interface ContractExport {
  rules: { tappCategoryAliases: Record<string, string> }
}

function loadContractExport(): ContractExport {
  const raw = readFileSync(
    join(repoRoot, 'tools/tapp-cli/src/generated/contract.json'),
    'utf8',
  )
  return JSON.parse(raw) as ContractExport
}

describe('tapp categories', () => {
  it('reads every older name as the name it installs as', () => {
    const aliases = loadContractExport().rules.tappCategoryAliases
    for (const [alias, name] of Object.entries(aliases)) {
      assert.equal(normalizeTappCategory(alias), name, alias)
    }
    for (const name of TAPP_CATEGORIES) {
      assert.equal(normalizeTappCategory(name), name)
    }
  })
})

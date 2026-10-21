import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

// 事实的 id 由后端事实层定，舞台按 id 取本地化标签。漏一个，界面就露出只写给模型看的英文。
const root = join(dirname(fileURLToPath(import.meta.url)), '../../../../..')
const factsDir = join(root, 'backend/src/api/reports/facts')
const locales = ['zh-CN', 'zh-TW', 'en-US', 'ja-JP', 'ko-KR', 'de-DE', 'fr-FR']

function backendFactIds(): Set<string> {
  const ids = new Set<string>()
  for (const file of readdirSync(factsDir)) {
    if (!file.endsWith('.rs') || file === 'tests.rs') continue
    const source = readFileSync(join(factsDir, file), 'utf8')
    for (const match of source.matchAll(/\.fact(?:_with_media)?\(\s*"([a-z0-9_]+)"/g)) ids.add(match[1])
  }
  return ids
}

test('every backend fact id has a stage label in every core locale', () => {
  const ids = backendFactIds()
  assert.ok(ids.size > 40, `parsed ${ids.size} fact ids; the pattern no longer matches the fact layer`)
  for (const locale of locales) {
    const stage = JSON.parse(readFileSync(join(root, `frontend/src/i18n/${locale}.json`), 'utf8')).reportStage
    const missing = [...ids].filter((id) => !stage.facts?.[id])
    assert.deepEqual(missing, [], `${locale} lacks labels`)
    for (const level of ['emerging', 'active', 'veteran', 'legendary']) {
      assert.ok(stage.factValues?.contribution_level?.[level], `${locale} lacks contribution level ${level}`)
    }
  }
})

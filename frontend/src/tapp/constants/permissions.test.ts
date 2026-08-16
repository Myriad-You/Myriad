/**
 * PERMISSION_CONFIG copy-key integrity.
 *
 * storage:read and storage:write are separate permissions (ADR 0020) and must
 * not share label/description i18n keys; every permission gets a unique pair.
 */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { PERMISSION_CONFIG } from './permissions.ts'

describe('permission copy keys', () => {
  it('storage:read and storage:write do not share label/description keys', () => {
    const read = PERMISSION_CONFIG['storage:read']
    const write = PERMISSION_CONFIG['storage:write']
    assert.notEqual(read.labelKey, write.labelKey)
    assert.notEqual(read.descriptionKey, write.descriptionKey)
    assert.equal(read.labelKey, 'permStorageRead')
    assert.equal(read.descriptionKey, 'permStorageReadDesc')
    assert.equal(write.labelKey, 'permStorageWrite')
    assert.equal(write.descriptionKey, 'permStorageWriteDesc')
  })

  it('every permission uses a unique label key and a unique description key', () => {
    const labelKeys = new Set<string>()
    const descriptionKeys = new Set<string>()
    for (const entry of Object.values(PERMISSION_CONFIG)) {
      assert.ok(
        !labelKeys.has(entry.labelKey),
        `duplicate labelKey ${entry.labelKey}`,
      )
      assert.ok(
        !descriptionKeys.has(entry.descriptionKey),
        `duplicate descriptionKey ${entry.descriptionKey}`,
      )
      labelKeys.add(entry.labelKey)
      descriptionKeys.add(entry.descriptionKey)
    }
  })
})

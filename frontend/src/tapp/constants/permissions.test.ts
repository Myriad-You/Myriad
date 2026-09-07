/**
 * PERMISSION_CONFIG copy-key integrity.
 *
 * storage:read and storage:write are separate permissions and must
 * not share label/description i18n keys; every permission gets a unique pair.
 */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import { PERMISSION_LEVELS, PERMISSION_MAP } from '../runtime/permissionConfig.ts'
import { PERMISSION_COPY } from './permissionCopy.ts'

describe('permission copy keys', () => {
  it('storage:read and storage:write do not share label/description keys', () => {
    const read = PERMISSION_COPY['storage:read']
    const write = PERMISSION_COPY['storage:write']
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
    for (const entry of Object.values(PERMISSION_COPY)) {
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

  it('exposes 3d:generate as elevated and keeps asset reads public', () => {
    assert.equal(PERMISSION_LEVELS['3d:generate'], 'elevated')
    assert.equal(PERMISSION_COPY['3d:generate'].labelKey, 'perm3dGenerate')
    assert.equal(PERMISSION_MAP.get('model3d.createTask'), '3d:generate')
    assert.equal(PERMISSION_MAP.get('model3d.getUrl'), 'public')
    assert.equal(PERMISSION_MAP.get('model3d.getMetadata'), 'public')
  })

  it('keeps file.download public so generated content can be saved without storage:read', () => {
    assert.equal(PERMISSION_MAP.get('file.download'), 'public')
    assert.equal(PERMISSION_MAP.get('storage.get'), 'storage:read')
  })
})

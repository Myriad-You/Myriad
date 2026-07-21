/**
 * Pure-function tests for store package asset path resolution.
 *
 *   cd frontend && node --experimental-strip-types --test src/tapp/utils/storePackagePaths.test.ts
 */
/* eslint-disable test/no-import-node-test */

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { storeAssetStorePath, storePackageRoot } from './storePackagePaths.ts'

describe('storePackageRoot', () => {
  it('returns parent of main.js / manifest.json', () => {
    assert.equal(
      storePackageRoot('apps/com.myriad.doudizhu/main.js'),
      'apps/com.myriad.doudizhu',
    )
    assert.equal(
      storePackageRoot('apps/com.myriad.doudizhu/manifest.json'),
      'apps/com.myriad.doudizhu',
    )
  })

  it('handles bare filename and leading slash', () => {
    assert.equal(storePackageRoot('main.js'), '')
    assert.equal(storePackageRoot('/nested/a/b/c.js'), 'nested/a/b')
  })
})

describe('storeAssetStorePath', () => {
  it('joins package root with assets path for doudizhu layout', () => {
    assert.equal(
      storeAssetStorePath(
        'apps/com.myriad.doudizhu',
        'assets/felt/table_felt.png',
      ),
      'apps/com.myriad.doudizhu/assets/felt/table_felt.png',
    )
  })

  it('works with empty root and normalizes slashes', () => {
    assert.equal(storeAssetStorePath('', 'assets/x.png'), 'assets/x.png')
    assert.equal(
      storeAssetStorePath('apps/foo/', '/assets/x.png'),
      'apps/foo/assets/x.png',
    )
  })
})

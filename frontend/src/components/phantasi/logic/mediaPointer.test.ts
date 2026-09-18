import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { mediaPointerUrl, resolveMediaPointer } from './mediaPointer.ts'

const api = 'http://localhost:3000'

describe('resolveMediaPointer', () => {
  it('rewrites catalog paths and absolute federation URLs to the current API origin', () => {
    assert.deepEqual(
      resolveMediaPointer(
        { id: 7, url: '/media/federation/1/a.jpg' },
        api,
      ),
      { id: 7, url: 'http://localhost:3000/media/federation/1/a.jpg' },
    )
    assert.deepEqual(
      resolveMediaPointer(
        { id: 8, url: 'https://old.example/media/federation/1/b.png' },
        api,
      ),
      { id: 8, url: 'http://localhost:3000/media/federation/1/b.png' },
    )
  })

  it('does not drop listed catalog media that already has a usable URL', () => {
    const listed = resolveMediaPointer(
      { id: 5, url: '/media/federation/1/asset.png' },
      api,
    )
    assert.ok(listed)
    assert.equal(listed.id, 5)
    assert.equal(listed.url, 'http://localhost:3000/media/federation/1/asset.png')
    assert.deepEqual(
      resolveMediaPointer({ id: 5, url: '/asset.png' }, api),
      { id: 5, url: '/asset.png' },
    )
  })

  it('rejects an upload payload that cannot point at a file', () => {
    assert.equal(resolveMediaPointer({ id: 1, url: '' }, api), null)
    assert.equal(resolveMediaPointer({ id: 1, url: '   ' }, api), null)
    assert.equal(resolveMediaPointer({ id: 1 }, api), null)
    assert.equal(resolveMediaPointer({ url: '/media/federation/1/a.jpg' }, api), null)
    assert.equal(mediaPointerUrl(''), null)
  })
})

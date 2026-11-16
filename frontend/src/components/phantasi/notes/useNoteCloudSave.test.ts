import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  cloudFieldsOf,
  mergeCloudFields,
  sameCloudFields,
} from './useNoteCloudSave'

const base = {
  title: 'a',
  contentMd: 'x\ny',
  topic: null,
  cover: null,
  publishedAt: 1,
}

describe('sameCloudFields', () => {
  it('null 和 undefined 的主题/封面当一样', () => {
    assert.ok(sameCloudFields(base, { ...base, topic: null }))
    assert.ok(!sameCloudFields(base, { ...base, contentMd: 'x' }))
    assert.ok(!sameCloudFields(base, { ...base, publishedAt: 2 }))
  })
})

describe('mergeCloudFields', () => {
  it('远端没有改发布时间时保留本地未确认修改', () => {
    assert.equal(mergeCloudFields(base, { ...base, publishedAt: 2 }, base).publishedAt, 2)
  })
  it('本地和远端各改一处都留下，发布时间听远端', () => {
    const local = { ...base, contentMd: 'X\ny', topic: 'ai' }
    const remote = { ...base, contentMd: 'x\nY', publishedAt: 9 }
    assert.deepEqual(mergeCloudFields(base, local, remote), {
      title: 'a',
      contentMd: 'X\nY',
      topic: 'ai',
      cover: null,
      publishedAt: 9,
      slug: null,
      oldPaths: null,
    })
  })
})

describe('cloudFieldsOf', () => {
  it('从服务端文档抽出五个字段', () => {
    const fields = cloudFieldsOf({
      id: 1,
      item_id: null,
      title: 't',
      content_md: 'c',
      topic: undefined as unknown as string | null,
      image: null,
      status: 'draft',
      scheduled_at: null,
      published_at: null,
      revision: 3,
      last_error: null,
      updated_at: 0,
    })
    assert.deepEqual(fields, {
      title: 't',
      contentMd: 'c',
      topic: null,
      cover: null,
      publishedAt: null,
      slug: null,
      oldPaths: null,
    })
  })
})

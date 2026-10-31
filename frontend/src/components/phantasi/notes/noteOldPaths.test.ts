import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { ApiError } from '../../../services/api'
import {
  isOldPathTakenError,
  MAX_NOTE_OLD_PATH_BYTES,
  MAX_NOTE_OLD_PATHS,
  normalizeOldPath,
  noteOldPathsInvalid,
  noteOldPathsWire,
  oldPathsField,
  oldPathsFromField,
} from './noteOldPaths.ts'

// 和后端 `old_paths.rs` 的单元测试同一批例子：两边规范化的结果必须一致。
describe('normalizeOldPath', () => {
  it('完整网址和路径落到同一个样子', () => {
    for (const raw of [
      '/2024/05/hello/',
      '/2024/05/hello',
      '2024/05/hello/',
      'https://old.example/2024/05/hello/',
      'HTTP://old.example/2024/05/hello?replytocom=3#c',
      '//old.example/2024//05/hello/',
      '  /2024/05/hello/  ',
    ]) {
      assert.equal(normalizeOldPath(raw), '/2024/05/hello', raw)
    }
  })

  it('百分号编码统一，中文解开', () => {
    assert.equal(normalizeOldPath('/posts/你好-世界/'), '/posts/你好-世界')
    assert.equal(normalizeOldPath('/posts/%E4%BD%A0%E5%A5%BD-%e4%b8%96%e7%95%8c/'), '/posts/你好-世界')
    assert.equal(normalizeOldPath('/a%2fb'), '/a%2Fb')
    assert.equal(normalizeOldPath('/a%20b'), '/a b')
    assert.equal(normalizeOldPath('/100%'), '/100%25')
    assert.equal(normalizeOldPath('/a%+1'), '/a%25+1')
    assert.equal(normalizeOldPath('/%c4%e3%ba%c3/'), '/%C4%E3%BA%C3')
  })

  it('站内地址不收，老博客在 /blog/ 下的可以', () => {
    for (const raw of ['/', 'https://old.example', '/api/blog/redirect', '/API/x', '/blog/', '/blog/posts/hello/', '/Blog/Posts/x', '/library', '/tapp/run/x', '/users/me', '/.well-known/webfinger']) {
      assert.equal(normalizeOldPath(raw), 'reserved', raw)
    }
    assert.equal(normalizeOldPath('/blog/2024/05/hello/'), '/blog/2024/05/hello')
    assert.equal(normalizeOldPath('/apis/x'), '/apis/x')
  })

  it('不像地址的和太长的不收', () => {
    assert.equal(normalizeOldPath('   '), 'invalid')
    assert.equal(normalizeOldPath('/a/../b'), 'invalid')
    assert.equal(normalizeOldPath('/a\\b'), 'invalid')
    assert.equal(normalizeOldPath('/a%0Ab'), 'invalid')
    assert.equal(normalizeOldPath(`/${'a'.repeat(MAX_NOTE_OLD_PATH_BYTES)}`), 'too-long')
    assert.equal(normalizeOldPath(`/${'a'.repeat(MAX_NOTE_OLD_PATH_BYTES - 1)}`), `/${'a'.repeat(MAX_NOTE_OLD_PATH_BYTES - 1)}`)
  })
})

describe('noteOldPathsWire', () => {
  it('每行一条，空行跳过，重复的只留一条', () => {
    assert.deepEqual(noteOldPathsWire('/a/\n\n  \n/a\nhttps://x.test/b'), ['/a', '/b'])
    assert.deepEqual(noteOldPathsWire(''), [])
    assert.equal(noteOldPathsInvalid(''), false)
  })

  it('有一行不合法或超出条数就整组不收', () => {
    assert.equal(noteOldPathsWire('/ok\n/api/x'), undefined)
    assert.equal(noteOldPathsInvalid('/ok\n/blog/posts/x/'), true)
    const lines = Array.from({ length: MAX_NOTE_OLD_PATHS + 1 }, (_, index) => `/p${index}`)
    assert.equal(noteOldPathsWire(lines.join('\n')), undefined)
    assert.equal(noteOldPathsWire(lines.slice(1).join('\n'))?.length, MAX_NOTE_OLD_PATHS)
  })

  it('云端字段一行一条往返', () => {
    assert.equal(oldPathsField(['/a', '/b']), '/a\n/b')
    assert.equal(oldPathsField(undefined), null)
    assert.deepEqual(oldPathsFromField('/a\n/b'), ['/a', '/b'])
    assert.deepEqual(oldPathsFromField(''), [])
  })
})

describe('isOldPathTakenError', () => {
  it('只把带 old address 的 409 当成被占用', () => {
    assert.equal(isOldPathTakenError(new ApiError('The old address "/a" already points to another note', 409)), true)
    assert.equal(isOldPathTakenError(new ApiError('Note draft was updated elsewhere', 409)), false)
    assert.equal(isOldPathTakenError(new ApiError('"/api" is not a valid old address', 400)), false)
  })
})

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'

import {
  decodeDownloadBase64,
  defaultDownloadFilename,
  parseHostDownloadUrl,
  parseLocalImageCacheUrl,
  validateFileDownloadOptions,
} from './fileDownload.ts'

const HASH = `ab${'a'.repeat(62)}`
const IMAGE_PATH = `/api/brew/image-cache/ab/${HASH}.png`
const MODEL_ID = 'a'.repeat(64)
const MODEL_PATH = `/api/model3d/assets/${MODEL_ID}`

describe('parseHostDownloadUrl', () => {
  it('accepts image-cache and public 3D asset paths', () => {
    assert.deepEqual(parseLocalImageCacheUrl(IMAGE_PATH), {
      path: IMAGE_PATH,
      defaultFilename: 'image.png',
      mimeType: 'image/png',
    })
    assert.equal(
      parseHostDownloadUrl(`https://example.com${IMAGE_PATH}?x=1`)?.path,
      IMAGE_PATH,
    )
    assert.deepEqual(parseHostDownloadUrl(MODEL_PATH), {
      path: MODEL_PATH,
      defaultFilename: 'model.glb',
      mimeType: 'model/gltf-binary',
    })
  })

  it('rejects anything that is not this site generated asset', () => {
    assert.equal(parseHostDownloadUrl('https://evil.example/img.png'), null)
    assert.equal(parseHostDownloadUrl('/api/brew/image-cache/aa/../x.png'), null)
    assert.equal(
      parseHostDownloadUrl(`/api/brew/image-cache/aa/${HASH}.png`),
      null,
    )
    assert.equal(parseHostDownloadUrl(`${MODEL_PATH}/metadata`), null)
    assert.equal(parseHostDownloadUrl('/api/model3d/assets/not-a-hash'), null)
    assert.equal(parseHostDownloadUrl('blob:https://example.com/abc'), null)
  })
})

describe('decodeDownloadBase64', () => {
  it('decodes raw base64 and data URLs', () => {
    const raw = decodeDownloadBase64(btoa('hello'))
    assert.equal(new TextDecoder().decode(raw?.bytes), 'hello')
    const data = decodeDownloadBase64(
      `data:audio/mpeg;base64,${btoa('audio')}`,
    )
    assert.equal(data?.mimeType, 'audio/mpeg')
    assert.equal(new TextDecoder().decode(data?.bytes), 'audio')
    assert.equal(decodeDownloadBase64('%%%'), null)
  })
})

describe('validateFileDownloadOptions', () => {
  it('accepts exactly one of content, url, or base64', () => {
    assert.equal(
      validateFileDownloadOptions({
        content: 'hello',
        filename: 'hello.txt',
      }).valid,
      true,
    )
    assert.equal(
      validateFileDownloadOptions({ url: IMAGE_PATH, filename: 'cat.png' })
        .valid,
      true,
    )
    assert.equal(validateFileDownloadOptions({ url: MODEL_PATH }).valid, true)
    assert.equal(
      validateFileDownloadOptions({
        base64: btoa('hi'),
        filename: 'a.bin',
      }).valid,
      true,
    )
    assert.equal(validateFileDownloadOptions({ base64: btoa('hi') }).valid, true)
    assert.equal(
      validateFileDownloadOptions({
        content: 'hello',
        url: IMAGE_PATH,
        filename: 'x.txt',
      }).valid,
      false,
    )
    assert.equal(
      validateFileDownloadOptions({ url: 'https://evil.example/a.png' }).valid,
      false,
    )
    assert.equal(
      validateFileDownloadOptions({ audio: btoa('hi-there-audio-bytes') }).valid,
      true,
    )
    assert.equal(
      validateFileDownloadOptions({ value: { url: IMAGE_PATH } }).valid,
      true,
    )
    assert.equal(
      validateFileDownloadOptions({
        result: { value: { url: IMAGE_PATH } },
      }).valid,
      true,
    )
    assert.equal(
      validateFileDownloadOptions({
        url: 'blob:https://example.com/abc',
        assetId: MODEL_ID,
      }).valid,
      true,
    )
  })
})

describe('defaultDownloadFilename', () => {
  it('uses path basename or mime', () => {
    assert.equal(defaultDownloadFilename('audio/mpeg'), 'audio.mp3')
    assert.equal(
      defaultDownloadFilename('image/png', 'assets/cat.png'),
      'cat.png',
    )
    assert.equal(defaultDownloadFilename(), 'download.bin')
  })
})

import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { encode } from 'uqr'
import {
  buildDeviceBindUri,
  DEVICE_BIND_URI_VERSION,
  isLoopbackSiteUrl,
  normalizeSiteUrl,
  parseDeviceBindUri,
  resolveBindSiteUrl,
} from './deviceBindQr.ts'

describe('deviceBindQr', () => {
  it('normalizes HTTP(S) site URLs and rejects other schemes', () => {
    assert.equal(normalizeSiteUrl('https://myriad.example/'), 'https://myriad.example')
    assert.equal(
      normalizeSiteUrl('  http://192.168.1.10:3000/sub/  '),
      'http://192.168.1.10:3000/sub',
    )
    assert.equal(
      normalizeSiteUrl('https://myriad.example/path/?foo=1#bar'),
      'https://myriad.example/path',
    )
    assert.equal(normalizeSiteUrl('javascript:alert(1)'), null)
    assert.equal(normalizeSiteUrl('ftp://myriad.example'), null)
    assert.equal(normalizeSiteUrl('not-a-url'), null)
  })

  it('round-trips myriad://device/bind URIs deterministically', () => {
    const token = 'mys_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg'
    const cases = [
      { input: 'https://myriad.example/', expectedSite: 'https://myriad.example' },
      { input: 'http://192.168.1.24:8080/', expectedSite: 'http://192.168.1.24:8080' },
      { input: 'https://host.example/app/', expectedSite: 'https://host.example/app' },
    ]
    for (const item of cases) {
      const uri = buildDeviceBindUri({ site: item.input, token })
      assert.ok(uri.startsWith(`myriad://device/bind?v=${DEVICE_BIND_URI_VERSION}&site=`))
      assert.deepEqual(parseDeviceBindUri(uri), {
        version: 1,
        site: item.expectedSite,
        token,
      })
    }
  })

  it('rejects invalid bind URIs and inputs', () => {
    assert.throws(() => buildDeviceBindUri({ site: 'javascript:alert(1)', token: 'mys_abc' }))
    assert.throws(() => buildDeviceBindUri({ site: 'https://myriad.example', token: '   ' }))
    assert.equal(parseDeviceBindUri('https://myriad.example/bind?v=1&site=https://a&token=mys_1'), null)
    assert.equal(parseDeviceBindUri('myriad://other/bind?v=1&site=https://a&token=mys_1'), null)
    assert.equal(parseDeviceBindUri('myriad://device/other?v=1&site=https://a&token=mys_1'), null)
    assert.equal(parseDeviceBindUri('myriad://device/bind?v=2&site=https://a&token=mys_1'), null)
    assert.equal(parseDeviceBindUri('myriad://device/bind?v=1&site=javascript:1&token=mys_1'), null)
    assert.equal(parseDeviceBindUri('myriad://device/bind?v=1&site=https://a&token='), null)
  })

  it('prefers configured site URL over browser origin', () => {
    assert.equal(
      resolveBindSiteUrl('https://public.example/', 'http://localhost:1102'),
      'https://public.example',
    )
    assert.equal(
      resolveBindSiteUrl(null, 'http://localhost:1102/'),
      'http://localhost:1102',
    )
    assert.equal(
      resolveBindSiteUrl('not-a-url', 'https://lan.example:8443/'),
      'https://lan.example:8443',
    )
  })

  it('detects loopback URLs accurately', () => {
    for (const url of [
      'http://localhost:1102',
      'https://127.0.0.1:8080',
      'http://[::1]:3000',
      'http://0.0.0.0:8080',
      'http://dev.localhost:1102',
    ]) {
      assert.equal(isLoopbackSiteUrl(url), true, url)
    }
    for (const url of [
      'https://myriad.example',
      'http://192.168.1.50:1102',
      'http://10.0.0.5:8080',
    ]) {
      assert.equal(isLoopbackSiteUrl(url), false, url)
    }
  })

  it('encodes a square QR matrix with 4-module white border via uqr', () => {
    const uri = buildDeviceBindUri({
      site: 'https://myriad.example',
      token: 'mys_0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefg',
    })
    const qr = encode(uri, { ecc: 'M', border: 4 })
    assert.ok(qr.size >= 29)
    assert.equal(qr.data.length, qr.size)
    for (let i = 0; i < 4; i++) {
      assert.equal(qr.data[i].every((cell) => !cell), true)
      assert.equal(qr.data[qr.size - 1 - i].every((cell) => !cell), true)
    }
  })
})

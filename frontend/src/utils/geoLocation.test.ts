import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

describe('geoLocation fallbacks', () => {
  const src = readFileSync(new URL('./geoLocation.ts', import.meta.url), 'utf8')
  const handlerSrc = readFileSync(
    new URL('../tapp/runtime/sandbox/handlers/advancedHandlers.ts', import.meta.url),
    'utf8',
  )

  it('does not use mixed-content HTTP geo APIs', () => {
    assert.equal(src.includes('http://ip-api.com'), false)
    assert.match(src, /https:\/\/ipapi\.co\/json\//)
    assert.match(src, /https:\/\/get\.geojs\.io\/v1\/ip\/geo\.json/)
  })

  it('coalesces IP and precise lookups onto one in-flight request', () => {
    assert.match(src, /let ipGeoInflight/)
    assert.match(src, /let preciseInflight/)
    assert.match(src, /GEO_NEGATIVE_TTL/)
    assert.match(src, /if \(ipGeoInflight\)/)
    assert.match(src, /if \(preciseInflight\)/)
  })

  it('does not call browser geolocation unless precise location is enabled', () => {
    assert.match(src, /export async function isPreciseLocationEnabled/)
    assert.match(src, /getUIConfigDeduped/)
    const browser = src.slice(src.indexOf('export async function getBrowserGeolocation'))
    const gate = browser.indexOf('isPreciseLocationEnabled')
    const gps = browser.indexOf('getCurrentPosition')
    assert.ok(gate >= 0 && gate < gps, 'setting gate must run before getCurrentPosition')
    const precise = src.slice(src.indexOf('export async function resolvePreciseLocation'))
    assert.match(precise, /isPreciseLocationEnabled/)
    assert.match(precise, /getClientGeoLocation/)
  })

  it('coarsens the browser fix before it is cached or sent out', () => {
    const browser = src.slice(src.indexOf('export async function getBrowserGeolocation'))
    const coarsen = browser.indexOf('coarsenCoordinate(position.coords.latitude)')
    assert.ok(coarsen >= 0, 'browser fix must be coarsened')
    assert.ok(coarsen < browser.indexOf('reverseGeocodeCity('))
    assert.ok(coarsen < browser.indexOf('writeBrowserGeoCache('))
  })

  it('shares host IP geo with TAPP context.getGeo', () => {
    assert.match(handlerSrc, /getClientGeoLocation/)
    assert.match(
      handlerSrc,
      /bridge\.registerHandler\('context\.getGeo'[\s\S]*getClientGeoLocation/,
    )
    assert.doesNotMatch(
      handlerSrc,
      /bridge\.registerHandler\('context\.getGeo'[\s\S]*getContextGeo/,
    )
  })
})

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

describe('weather location coalescing', () => {
  const src = readFileSync(new URL('./weather.ts', import.meta.url), 'utf8')

  it('reuses one in-flight weather fetch and a last-known cache before locating', () => {
    assert.match(src, /let weatherInflight/)
    assert.match(src, /LAST_WEATHER_KEY/)
    assert.match(src, /readLastWeather/)
    const info = src.slice(src.indexOf('export async function getWeatherInfo'))
    const locate = info.indexOf('resolvePreciseLocation')
    const last = info.indexOf('readLastWeather')
    const inflight = info.indexOf('weatherInflight')
    assert.ok(last >= 0 && last < locate, 'last weather must be checked before locating')
    assert.ok(inflight >= 0 && inflight < locate, 'in-flight share must run before locating')
    assert.match(info, /isPreciseLocationEnabled/)
    assert.match(info, /hasBrowserGeoFix/)
  })
})

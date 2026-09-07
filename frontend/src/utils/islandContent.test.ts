/**
 * @vitest-environment node
 */
import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import {
  allowsIslandType,
  DEFAULT_ISLAND_CONTENT,
  islandBagValues,
  islandContentFromBagFields,
  islandContentFromPublicUi,
  parseIslandFlag,
} from './islandContent'

describe('islandContent', () => {
  it('treats missing values as shown', () => {
    assert.equal(parseIslandFlag(undefined), true)
    assert.equal(parseIslandFlag(null), true)
    assert.equal(parseIslandFlag(''), true)
    assert.equal(parseIslandFlag('true'), true)
    assert.equal(parseIslandFlag(true), true)
  })

  it('treats explicit false as hidden', () => {
    assert.equal(parseIslandFlag(false), false)
    assert.equal(parseIslandFlag(0), false)
    assert.equal(parseIslandFlag('false'), false)
    assert.equal(parseIslandFlag('0'), false)
    assert.equal(parseIslandFlag(' False '), false)
  })

  it('reads public UI booleans and bag strings', () => {
    assert.deepEqual(
      islandContentFromPublicUi({
        island_show_greeting: false,
        island_show_weather: true,
      }),
      {
        ...DEFAULT_ISLAND_CONTENT,
        greeting: false,
      },
    )
    assert.deepEqual(
      islandContentFromBagFields([
        { key: 'island_show_quote', value: 'false' },
        { key: 'island_show_tapp', value: 'false' },
      ]),
      {
        ...DEFAULT_ISLAND_CONTENT,
        quote: false,
        tapp: false,
      },
    )
  })

  it('keeps notifications on the island regardless of these flags', () => {
    const hidden = islandBagValues({
      greeting: false,
      weather: false,
      quote: false,
      music: false,
      tapp: false,
    })
    const prefs = islandContentFromPublicUi(hidden)
    assert.equal(allowsIslandType(prefs, 'notification'), true)
    assert.equal(allowsIslandType(prefs, 'greeting'), false)
    assert.equal(allowsIslandType(prefs, 'tapp-demo'), false)
    assert.equal(allowsIslandType(prefs, 'theme'), true)
  })
})

/**
 * TAPP persona card projection.
 *
 *   pnpm exec tsx --test src/tapp/runtime/sandbox/personaCard.test.ts
 */

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'
import { projectPersonaCard, sameOriginPortraitUrl } from './personaCard.ts'

const CARD_FIELDS = [
  'activity',
  'enabled',
  'moodBand',
  'name',
  'portraitUrl',
] as const

describe('sameOriginPortraitUrl', () => {
  it('keeps host paths that img-src already allows', () => {
    assert.equal(
      sameOriginPortraitUrl('/api/brew/image-cache/ab/abcd.png'),
      '/api/brew/image-cache/ab/abcd.png',
    )
    assert.equal(sameOriginPortraitUrl('/uploads/face.png'), '/uploads/face.png')
  })

  it('drops off-site, protocol, and traversal values', () => {
    assert.equal(sameOriginPortraitUrl('https://cdn.example.com/a.png'), null)
    assert.equal(sameOriginPortraitUrl('//cdn.example.com/a.png'), null)
    assert.equal(sameOriginPortraitUrl('/uploads/../secret'), null)
    assert.equal(sameOriginPortraitUrl('javascript:alert(1)'), null)
    assert.equal(sameOriginPortraitUrl('asset_1-2.png'), null)
    assert.equal(sameOriginPortraitUrl(''), null)
    assert.equal(sameOriginPortraitUrl(null), null)
  })

  it('drops whitespace and C0 controls without a literal NUL in source', () => {
    assert.equal(sameOriginPortraitUrl('/uploads/face 1.png'), null)
    assert.equal(sameOriginPortraitUrl('/uploads/face\t.png'), null)
    assert.equal(sameOriginPortraitUrl('/uploads/face\u0000.png'), null)
    assert.equal(sameOriginPortraitUrl('/uploads/face\u0001.png'), null)
    const source = readFileSync(new URL('./personaCard.ts', import.meta.url))
    assert.equal(source.includes(0), false)
  })
})

describe('projectPersonaCard', () => {
  it('maps baseline vitals to the calm idle band', () => {
    assert.deepEqual(
      projectPersonaCard({
        enabled: true,
        name: 'Arael',
      }),
      {
        enabled: true,
        name: 'Arael',
        moodBand: 'calm',
        activity: 'idle',
        portraitUrl: null,
      },
    )
  })

  it('exposes exactly the five public fields; numbers and soul stay off', () => {
    const card = projectPersonaCard({
      enabled: true,
      name: '瞳',
      mood: 40,
      arousal: 70,
      activity: 'working',
      portraitUrl: '/api/brew/image-cache/ab/face.png',
      personality: 'secret soul',
      visualProfile: { hair: 'black' },
    } as never)
    assert.deepEqual(Object.keys(card).sort(), [...CARD_FIELDS])
    assert.equal(card.moodBand, 'tense')
    assert.equal(card.activity, 'working')
    assert.equal(card.portraitUrl, '/api/brew/image-cache/ab/face.png')
    assert.equal('mood' in card, false)
    assert.equal('arousal' in card, false)
    assert.equal('personality' in card, false)
    assert.equal('visualProfile' in card, false)
  })

  it('keeps the off-state name the caller already resolved', () => {
    const card = projectPersonaCard({
      enabled: false,
      name: 'Agent',
      portraitUrl: '/uploads/face.png',
    })
    assert.equal(card.enabled, false)
    assert.equal(card.name, 'Agent')
    assert.equal(card.portraitUrl, '/uploads/face.png')
  })
})

describe('persona.get handler wall', () => {
  it('projects through projectPersonaCard instead of returning the GET body', () => {
    const source = readFileSync(
      new URL('./handlers/personaHandlers.ts', import.meta.url),
      'utf8',
    )
    assert.match(source, /data:\s*projectPersonaCard\(/)
    assert.doesNotMatch(source, /data:\s*(persona|face|config)\b/)
  })

  it('keeps the playground stub on the same five fields', () => {
    const source = readFileSync(
      new URL('./handlers/playgroundPreviewHandlers.ts', import.meta.url),
      'utf8',
    )
    const stub = source.match(
      /registerHandler\('persona\.get'[\s\S]*?data:\s*\{([^}]+)\}/,
    )
    assert.ok(stub, 'playground persona.get stub is missing')
    const keys = [...stub[1].matchAll(/^\s*([A-Z]+):/gim)].map(
      match => match[1],
    )
    assert.deepEqual(keys.sort(), [...CARD_FIELDS])
  })
})

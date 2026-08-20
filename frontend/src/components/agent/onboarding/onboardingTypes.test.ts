import assert from 'node:assert/strict'
import test from 'node:test'
import {
  flattenPersona,
  parseFlattenedPersona,
  parseList,
  personaFromApi,
} from './onboardingTypes'

test('flatten then parse keeps character fields', () => {
  const persona = {
    summary: '话少，认真。',
    temperament: ['克制', '细心'],
    likes: ['雨声'],
    drives: ['理解彼此'],
    socialStyle: '不抢话',
    speechStyle: '简洁温和',
  }
  const parsed = parseFlattenedPersona(flattenPersona(persona))
  assert.deepEqual(parsed.temperament, persona.temperament)
  assert.equal(parsed.summary, persona.summary)
  assert.equal(parsed.socialStyle, persona.socialStyle)
})

test('parseList keeps commas inside an English item', () => {
  assert.deepEqual(parseList('Blunt mouth, soft heart、Recharges alone'), [
    'Blunt mouth, soft heart',
    'Recharges alone',
  ])
})

test('personaFromApi reads character fields and ignores visuals', () => {
  const parsed = personaFromApi({
    summary: '安静但会认真回应。',
    temperament: ['克制'],
    visualIdentity: { hairShape: '短发' },
  })
  assert.equal(parsed.summary, '安静但会认真回应。')
  assert.deepEqual(parsed.temperament, ['克制'])
  assert.equal(parsed.speechStyle, '')
})

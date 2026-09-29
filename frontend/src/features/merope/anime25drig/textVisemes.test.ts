import assert from 'node:assert/strict'
import test from 'node:test'
import { compileTextVisemes } from './textVisemes'

async function vowels(text: string, locale: string) {
  const cues = await compileTextVisemes(text, locale)
  return cues
    .filter((cue) => cue.viseme !== 'rest' && cue.viseme !== 'closed' && cue.viseme !== 'narrow')
    .map((cue) => `${cue.viseme}:${cue.openness ?? 1}`)
}

test('a u is a smaller round than an o, an i a flatter spread than an e', async () => {
  const [u, o] = await vowels('うお', 'ja')
  assert.ok(u.startsWith('round:') && o === 'round:1', `${u} ${o}`)
  assert.ok(Number(u.split(':')[1]) < 1)
  const [i, e] = await vowels('いえ', 'ja')
  assert.ok(i.startsWith('wide:') && e === 'wide:1', `${i} ${e}`)
  assert.ok(Number(i.split(':')[1]) < 1)
  const english = await vowels('moon more meet met', 'en')
  assert.deepEqual(english.map((cue) => Number(cue.split(':')[1]) < 1), [true, false, false, true, false])
})

test('a Chinese final is held in its main vowel: 好 opens, 我 rounds, 五 rounds small, 你 spreads', async () => {
  // The final, after any glide into it.
  const final = async (text: string) => (await vowels(text, 'zh')).at(-1)!
  const hao = await final('好')
  const wo = await final('我')
  const wu = await final('五')
  const ni = await final('你')
  assert.equal(hao, 'open:1')
  assert.equal(wo, 'round:1')
  assert.ok(wu.startsWith('round:') && Number(wu.split(':')[1]) < 1, wu)
  assert.ok(ni.startsWith('wide:') && Number(ni.split(':')[1]) < 1, ni)
})

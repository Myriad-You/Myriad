import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { consumeSetupSecretFromLocation } from './setupSecretFromUrl.ts'

const SECRET = `s${'a'.repeat(31)}`

describe('consumeSetupSecretFromLocation', () => {
  it('reads hash and strips it', () => {
    const replaced: string[] = []
    const value = consumeSetupSecretFromLocation(
      { pathname: '/', search: '', hash: `#setup_secret=${SECRET}` },
      (url) => replaced.push(url),
    )
    assert.equal(value, SECRET)
    assert.deepEqual(replaced, ['/'])
  })

  it('reads query and strips it', () => {
    const replaced: string[] = []
    const value = consumeSetupSecretFromLocation(
      { pathname: '/', search: `?setup_secret=${SECRET}&x=1`, hash: '' },
      (url) => replaced.push(url),
    )
    assert.equal(value, SECRET)
    assert.deepEqual(replaced, ['/?x=1'])
  })

  it('prefers hash over query', () => {
    const other = `b${'c'.repeat(31)}`
    const value = consumeSetupSecretFromLocation({
      pathname: '/',
      search: `?setup_secret=${other}`,
      hash: `#setup_secret=${SECRET}`,
    })
    assert.equal(value, SECRET)
  })

  it('ignores short or illegal values', () => {
    assert.equal(
      consumeSetupSecretFromLocation({
        pathname: '/',
        search: '?setup_secret=short',
        hash: '',
      }),
      null,
    )
    assert.equal(
      consumeSetupSecretFromLocation({
        pathname: '/',
        search: '',
        hash: '#setup_secret=has space and===',
      }),
      null,
    )
  })

  it('leaves SPA path hashes alone', () => {
    const replaced: string[] = []
    const value = consumeSetupSecretFromLocation(
      { pathname: '/', search: '', hash: '#/setup' },
      (url) => replaced.push(url),
    )
    assert.equal(value, null)
    assert.deepEqual(replaced, [])
  })
})

import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { describe, it } from 'node:test'

describe('logout destroys Tapp runtimes', () => {
  it('resetTappSubjectState runs destroyAll before logout clears the user', () => {
    const src = readFileSync(new URL('./AuthContext.tsx', import.meta.url), 'utf8')
    const resetAt = src.indexOf('TappRuntimeGrant.destroyAll()')
    const logoutAt = src.indexOf('const logout = useCallback')
    const resetCallAt = src.indexOf('void resetTappSubjectState()', logoutAt)
    assert.ok(resetAt >= 0, 'destroyAll must stay on the auth-reset path')
    assert.ok(logoutAt >= 0, 'logout callback')
    assert.ok(resetCallAt > logoutAt, 'logout must reset Tapp state')
    const invalidateAt = src.indexOf("authSubject.change('guest', true)", logoutAt)
    assert.ok(invalidateAt > logoutAt && invalidateAt < resetCallAt,
      'old speech and callbacks must be invalidated synchronously, before async cleanup')
    assert.ok(
      src.indexOf('setUser(null)', resetCallAt) > resetCallAt,
      'clearing the user after destroy keeps a dead grant from surviving',
    )
  })
})

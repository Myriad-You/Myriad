import assert from 'node:assert/strict'
import test from 'node:test'
import { authSubjectKey, AuthSubjectScope } from './authSubject'

test('identity invalidation is synchronous; same-user refresh is not invalidation', () => {
  const scope = new AuthSubjectScope()
  let resets = 0
  const guest = scope.signal
  const off = scope.subscribe(() => { resets += 1; assert.equal(guest.aborted, true) })
  scope.change(authSubjectKey({ id: 1 }))
  const user = scope.signal
  scope.change(authSubjectKey({ id: 1 }))
  assert.equal(user.aborted, false)
  assert.equal(resets, 1)
  scope.change(authSubjectKey({ id: 2 }))
  assert.equal(user.aborted, true)
  scope.change('guest', true)
  const nextGuest = scope.signal
  scope.change('guest', true)
  assert.equal(nextGuest.aborted, true)
  assert.equal(resets, 4)
  off()
  scope.change('changing')
  assert.equal(resets, 4)
})

import assert from 'node:assert/strict'
import test from 'node:test'
import { authSubject } from '../utils/authSubject'
import { currentPagePublisher, getCurrentPageContent } from './currentPage'

test('identity invalidation clears the source and rejects late publisher writes and cleanup', () => {
  const publishA = currentPagePublisher()
  publishA({ type: 'custom', title: 'A-private' })
  assert.equal(getCurrentPageContent()?.title, 'A-private')
  authSubject.change('B', true)
  assert.equal(getCurrentPageContent(), null)
  publishA({ type: 'custom', title: 'A-late' })
  assert.equal(getCurrentPageContent(), null)
  const publishB = currentPagePublisher()
  publishB({ type: 'custom', title: 'B' })
  publishA(null)
  assert.equal(getCurrentPageContent()?.title, 'B')
  authSubject.change('B')
  assert.equal(getCurrentPageContent()?.title, 'B', 'same-user refresh preserves context')
  authSubject.change('guest', true)
})

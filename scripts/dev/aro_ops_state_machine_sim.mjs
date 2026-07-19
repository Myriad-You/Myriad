#!/usr/bin/env node
/**
 * Runtime simulation of Aro messenger composer / feed+ state machine.
 * Does not need a browser — exercises the same predicates as aro.ts helpers.
 *
 * Usage: node scripts/dev/aro_ops_state_machine_sim.mjs
 */
import assert from 'node:assert/strict'

function isChannelStatusWritable(status) {
  return status === 'active' || status === 'accepted'
}

function isChannelComposerLocked(state) {
  if (state.activeKind !== 'channel') return false
  if (!state.channelDetail) return !!state.activeId
  return !isChannelStatusWritable(state.channelDetail.status)
}

function updateSendState(state) {
  const locked = isChannelComposerLocked(state)
  const blocked = !state.activeId || locked || !!state.sending
  const inputDisabled = locked || !state.activeId
  const hasContent = !!(
    (!inputDisabled && (state.inputText || '').trim()) ||
    (!locked && state.pendingAttach)
  )
  const ready = !blocked && hasContent
  return {
    locked,
    blocked,
    composerLockedClass: locked,
    inputDisabled,
    attachDisabled: blocked,
    sendDisabled: !ready,
    ready,
  }
}

function canComposePost(state) {
  return (
    !state.isGuest &&
    state.currentView === 'feed' &&
    state.feedSubTab === 'timeline'
  )
}

function canFollowFromFeed(state) {
  return (
    !state.isGuest &&
    state.currentView === 'feed' &&
    state.feedSubTab === 'following'
  )
}

function updateFeedPlusVisibility(state) {
  const showPost = canComposePost(state)
  const showFollow = canFollowFromFeed(state)
  return { showPost, showFollow, showPlus: showPost || showFollow }
}

// --- Scenarios ---
const failures = []
function check(name, cond, detail) {
  try {
    assert.ok(cond, detail || name)
    console.log('  PASS', name)
  } catch (e) {
    failures.push(name + ': ' + (e.message || e))
    console.log('  FAIL', name, '—', e.message || e)
  }
}

console.log('1) Accepted / active channel: send+attach enabled with text')
{
  for (const status of ['active', 'accepted']) {
    const r = updateSendState({
      activeKind: 'channel',
      activeId: 'c1',
      channelDetail: { status },
      sending: false,
      inputText: 'hi',
    })
    check(`${status}+text ready`, r.ready && !r.locked && !r.attachDisabled)
  }
}

console.log('2) Pending channel: composer locked (backend rejects send)')
{
  const r = updateSendState({
    activeKind: 'channel',
    activeId: 'c1',
    channelDetail: { status: 'pending', initiated_by: 'remote' },
    sending: false,
    inputText: 'hi',
  })
  check('pending locked', r.locked && r.attachDisabled && r.sendDisabled && !r.ready)
}

console.log('3) Closed channel: composer locked')
{
  const r = updateSendState({
    activeKind: 'channel',
    activeId: 'c1',
    channelDetail: { status: 'closed' },
    sending: false,
    inputText: 'hi',
  })
  check('closed locked', r.locked && r.composerLockedClass)
}

console.log('4) Leave closed channel → empty: lock class cleared, blocked by no activeId')
{
  // Simulate back / leave cleanup
  const after = {
    activeKind: null,
    activeId: null,
    channelDetail: null,
    sending: false,
    inputText: '',
  }
  const r = updateSendState(after)
  check('no stick composer-locked', !r.composerLockedClass)
  check('no active still blocked', r.blocked && r.inputDisabled)
}

console.log('5) Switch closed → accepted via openConversation mid/final')
{
  // Mid-open: channel detail cleared
  const mid = updateSendState({
    activeKind: 'channel',
    activeId: 'c2',
    channelDetail: null,
    sending: false,
    inputText: 'hi',
  })
  check('mid-open locks channel', mid.locked)

  const done = updateSendState({
    activeKind: 'channel',
    activeId: 'c2',
    channelDetail: { status: 'accepted' },
    sending: false,
    inputText: 'hi',
  })
  check('after load accepted unlocks', !done.locked && done.ready)
}

console.log('6) Accept path: pending → accepted unlocks')
{
  const before = updateSendState({
    activeKind: 'channel',
    activeId: 'c1',
    channelDetail: { status: 'pending' },
    sending: false,
    inputText: 'hello',
  })
  check('before accept locked', before.locked)
  const after = updateSendState({
    activeKind: 'channel',
    activeId: 'c1',
    channelDetail: { status: 'accepted' },
    sending: false,
    inputText: 'hello',
  })
  check('after accept ready', after.ready && !after.locked)
}

console.log('7) Room open: never channel-locked')
{
  const r = updateSendState({
    activeKind: 'room',
    activeId: 'r1',
    channelDetail: null,
    sending: false,
    inputText: 'hi',
  })
  check('room ready', r.ready && !r.locked && !r.attachDisabled)
}

console.log('8) Feed + visibility')
{
  const cases = [
    [{ isGuest: false, currentView: 'feed', feedSubTab: 'timeline' }, true, true, false],
    [{ isGuest: false, currentView: 'feed', feedSubTab: 'following' }, true, false, true],
    [{ isGuest: false, currentView: 'feed', feedSubTab: 'followers' }, false, false, false],
    [{ isGuest: false, currentView: 'messages', feedSubTab: 'timeline' }, false, false, false],
    [{ isGuest: true, currentView: 'feed', feedSubTab: 'timeline' }, false, false, false],
    [{ isGuest: false, currentView: null, feedSubTab: 'timeline' }, false, false, false],
  ]
  for (const [st, showPlus, showPost, showFollow] of cases) {
    const r = updateFeedPlusVisibility(st)
    check(
      `plus view=${st.currentView} sub=${st.feedSubTab} guest=${st.isGuest}`,
      r.showPlus === showPlus && r.showPost === showPost && r.showFollow === showFollow,
      JSON.stringify(r),
    )
  }
}

console.log('9) currentView always set on switchView-like transition')
{
  let currentView = 'feed'
  function switchView(view, isGuest) {
    if (isGuest && view !== 'feed') view = 'feed'
    currentView = view
    return currentView
  }
  check('messages', switchView('messages', false) === 'messages')
  check('guest forced feed', switchView('messages', true) === 'feed')
  check('rings', switchView('rings', false) === 'rings')
  // After guest force, currentView is feed so + can show when owner later
  currentView = 'feed'
  check(
    'owner timeline +',
    updateFeedPlusVisibility({ isGuest: false, currentView, feedSubTab: 'timeline' })
      .showPlus,
  )
}

// --- loadUserRole fail-open for authenticated members ---
function applyKnownUserRole(state, role) {
  const r = role == null ? '' : String(role).trim().toLowerCase()
  if (r === 'admin') {
    state.userRole = 'admin'
    state.isAdmin = true
    state.isGuest = false
    return true
  }
  if (r === 'user' || r === 'member' || r === 'owner') {
    state.userRole = 'user'
    state.isAdmin = false
    state.isGuest = false
    return true
  }
  if (r === 'guest') {
    state.userRole = 'guest'
    state.isAdmin = false
    state.isGuest = true
    return true
  }
  return false
}

function isAuthenticatedUserContext(user) {
  if (!user || typeof user !== 'object') return false
  if (user.role === 'guest') return false
  const id = user.id != null ? String(user.id).trim() : ''
  const username = user.username != null ? String(user.username).trim() : ''
  if (id && id !== '0' && id.toLowerCase() !== 'guest' && id.toLowerCase() !== 'anonymous') {
    return true
  }
  if (
    username &&
    username.toLowerCase() !== 'guest' &&
    username.toLowerCase() !== 'anonymous'
  ) {
    return true
  }
  return false
}

/** Mirrors fixed loadUserRole resolution order (pure). */
function resolveUserRole({ roleFromApi, user }) {
  const state = { userRole: 'guest', isGuest: true, isAdmin: false }
  if (roleFromApi != null && String(roleFromApi).trim() !== '') {
    if (applyKnownUserRole(state, roleFromApi)) {
      if (!state.isGuest) return state
      // soft guest — fall through to getUser
    }
  }
  if (user && user.role != null && String(user.role).trim() !== '') {
    if (applyKnownUserRole(state, user.role)) return state
  }
  if (user && user.isAdmin === true) {
    state.userRole = 'admin'
    state.isAdmin = true
    state.isGuest = false
    return state
  }
  if (isAuthenticatedUserContext(user)) {
    state.userRole = 'user'
    state.isAdmin = false
    state.isGuest = false
    return state
  }
  state.userRole = 'guest'
  state.isAdmin = false
  state.isGuest = true
  return state
}

console.log('10) loadUserRole: role API fail + real user ⇒ NOT guest-locked')
{
  const r = resolveUserRole({
    roleFromApi: null,
    user: { id: '42', username: 'haru', role: undefined },
  })
  check('fallback member', !r.isGuest && r.userRole === 'user')
}

console.log('11) loadUserRole: host false guest + real user ⇒ promote to member')
{
  // Host returns userRole||'guest' even when logged in; getUser has real id.
  const r = resolveUserRole({
    roleFromApi: 'guest',
    user: { id: '1', username: 'x' },
  })
  check('promote past false guest', !r.isGuest && r.userRole === 'user')
}

console.log('11b) loadUserRole: true guest (role guest + no auth identity)')
{
  const r = resolveUserRole({
    roleFromApi: 'guest',
    user: { role: 'guest' },
  })
  check('true guest context', r.isGuest)
}

console.log('12) loadUserRole: empty getRole + getUser.role=user')
{
  const r = resolveUserRole({
    roleFromApi: '',
    user: { id: '9', username: 'owner', role: 'user' },
  })
  check('empty role string falls through to user.role', !r.isGuest && r.userRole === 'user')
}

console.log('13) loadUserRole: no user, role API fail ⇒ guest')
{
  const r = resolveUserRole({
    roleFromApi: null,
    user: null,
  })
  check('true guest', r.isGuest)
}

console.log('14) loadUserRole: admin from getRole')
{
  const r = resolveUserRole({
    roleFromApi: 'admin',
    user: null,
  })
  check('admin', !r.isGuest && r.isAdmin && r.userRole === 'admin')
}

console.log('\n' + (failures.length ? `FAILED ${failures.length}` : 'ALL PASSED'))
if (failures.length) {
  for (const f of failures) console.error(' -', f)
  process.exit(1)
}

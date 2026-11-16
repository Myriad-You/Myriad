import type { TappInstance } from '@myriad/tapp-runtime'
import assert from 'node:assert/strict'
import test from 'node:test'
import {
  clearBackgroundResidents,
  getBackgroundResidentsSnapshot,
  publishBackgroundResidents,
  registerBackgroundResidentStopHandler,
  stopBackgroundResident,
  subscribeBackgroundResidents,
} from './backgroundResidentStore'

function tapp(id: string, name: string): TappInstance {
  return {
    id,
    manifest: {
      id,
      name,
      version: '1.0.0',
      description: '',
      permissions: [],
      category: 'utility',
    },
    status: 'running',
    installationStatus: 'running',
    installedAt: '2026-01-01T00:00:00.000Z',
    grantedPermissions: [],
    userRole: 'admin',
  }
}

test('background resident store publishes live residents and routes stop requests', () => {
  clearBackgroundResidents()
  let notifications = 0
  const unsubscribe = subscribeBackgroundResidents(() => notifications++)

  publishBackgroundResidents([tapp('clock', 'Clock')], () => [
    'scheduler',
    'notification',
  ])
  assert.deepEqual(getBackgroundResidentsSnapshot(), [
    {
      id: 'clock',
      name: 'Clock',
      requirements: ['scheduler', 'notification'],
    },
  ])
  assert.equal(notifications, 1)

  publishBackgroundResidents([tapp('clock', 'Clock')], () => [
    'scheduler',
    'notification',
  ])
  assert.equal(notifications, 1, 'equivalent snapshots must remain stable')

  let stopped = ''
  const unregisterStop = registerBackgroundResidentStopHandler((tappId) => {
    stopped = tappId
  })
  stopBackgroundResident('clock')
  assert.equal(stopped, 'clock')

  unregisterStop()
  unsubscribe()
  clearBackgroundResidents()
})

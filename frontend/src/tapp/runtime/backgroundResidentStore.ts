import type { BackgroundRequirement, TappInstance } from '../types'
import { useSyncExternalStore } from 'react'
import { createStore } from '../../utils/store'

export interface BackgroundResident {
  id: string
  name: string
  requirements: readonly BackgroundRequirement[]
}

const EMPTY_RESIDENTS: readonly BackgroundResident[] = []

let stopHandler: ((tappId: string) => void) | null = null

function sameResidents(
  left: readonly BackgroundResident[],
  right: readonly BackgroundResident[],
): boolean {
  return (
    left.length === right.length &&
    left.every((resident, index) => {
      const candidate = right[index]
      return (
        candidate?.id === resident.id &&
        candidate.name === resident.name &&
        candidate.requirements.length === resident.requirements.length &&
        candidate.requirements.every(
          (requirement, requirementIndex) =>
            requirement === resident.requirements[requirementIndex],
        )
      )
    })
  )
}

const residents = createStore<readonly BackgroundResident[]>(EMPTY_RESIDENTS, sameResidents)

export function publishBackgroundResidents(
  tapps: readonly TappInstance[],
  requirementsFor: (tappId: string) => BackgroundRequirement[],
): void {
  const next = tapps.map((tapp) => ({
    id: tapp.id,
    name: tapp.manifest.name,
    requirements: requirementsFor(tapp.id),
  }))
  residents.set(next.length > 0 ? next : EMPTY_RESIDENTS)
}

export function clearBackgroundResidents(): void {
  residents.set(EMPTY_RESIDENTS)
}

export function registerBackgroundResidentStopHandler(
  handler: (tappId: string) => void,
): () => void {
  stopHandler = handler
  return () => {
    if (stopHandler === handler) stopHandler = null
  }
}

export function stopBackgroundResident(tappId: string): void {
  stopHandler?.(tappId)
}

export const subscribeBackgroundResidents = residents.subscribe
export const getBackgroundResidentsSnapshot = residents.get

export function getServerBackgroundResidentsSnapshot(): readonly BackgroundResident[] {
  return EMPTY_RESIDENTS
}

export function useBackgroundResidents(): readonly BackgroundResident[] {
  return useSyncExternalStore(
    subscribeBackgroundResidents,
    getBackgroundResidentsSnapshot,
    getServerBackgroundResidentsSnapshot,
  )
}

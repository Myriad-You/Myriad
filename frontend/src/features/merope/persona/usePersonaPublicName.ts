import { useSyncExternalStore } from 'react'
import {
  onPersonaPublicName,
  PERSONA_OFF_NAME,
  personaPublicName,
} from './publicName'

export function usePersonaPublicName(): string {
  return useSyncExternalStore(
    onPersonaPublicName,
    personaPublicName,
    () => PERSONA_OFF_NAME,
  )
}

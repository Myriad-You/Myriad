import type { RefObject } from 'react'
import type { RigCharacterHandle } from './rig/RigCharacter'
import { useEffect } from 'react'
import { MEROPE_SPEECH_EVENT, meropeSpeechEventDetail } from './speechEvents'
import { SpeechLifecycleController } from './speechLifecycle'

/** Connect one mounted rig to Agent reply speech without owning its UI host. */
export function useRigSpeechLifecycle(
  rigRef: RefObject<RigCharacterHandle | null>,
): void {
  useEffect(() => {
    const controller = new SpeechLifecycleController({
      setSpeechActive: (active) => rigRef.current?.setSpeechActive(active),
      setAutoSpeech: (active) => rigRef.current?.setAutoSpeech(active),
      setSpeechEnergy: (energy) => rigRef.current?.setSpeechEnergy(energy),
      setSpeechArticulation: (articulation) =>
        rigRef.current?.setSpeechArticulation(articulation),
    })
    const onSpeech = (event: Event) => {
      const detail = meropeSpeechEventDetail(
        (event as CustomEvent<unknown>).detail,
      )
      if (detail) controller.handle(detail)
    }
    window.addEventListener(MEROPE_SPEECH_EVENT, onSpeech)
    return () => {
      window.removeEventListener(MEROPE_SPEECH_EVENT, onSpeech)
      controller.dispose()
    }
  }, [rigRef])
}

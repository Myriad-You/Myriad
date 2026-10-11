import type { Dispatch, SetStateAction } from 'react'
import type { AgentPersona } from '../../../services/agent/agentApi'
import { useCallback } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import { agentService } from '../../../services/agent'
import { emitAppEvent } from '../../../utils/appEvents'
import { userFacingError } from '../../../utils/userFacingError'
import { reportMeropeError } from './workbenchShared'

interface Options {
  persona: AgentPersona | null
  setPersona: Dispatch<SetStateAction<AgentPersona | null>>
  /** Re-reads her persona, design and wardrobe after the server changed them. */
  reload: () => Promise<void>
  loadFace: () => Promise<unknown>
}

/**
 * Putting back what her name or visual settings held before (see
 * `FieldHistory`): each is an ordinary edit, which keeps what it replaces
 * in the history too.
 */
export function usePersonaRestore({
  persona,
  setPersona,
  reload,
  loadFace,
}: Options) {
  const { t } = useI18n()
  const o = t.agentPersona.onboarding

  /** Puts back a name she had, as if typed in again. */
  const restoreName = useCallback(
    async (name: string) => {
      const named = name.trim()
      if (!persona || !named) return
      try {
        const saved = await agentService.putPersona({
          name: named,
          personality: persona.personality ?? '',
          // Her persona names her too, as every save of it does.
          ...(persona.persona
            ? { persona: { ...persona.persona, displayName: named } }
            : {}),
        })
        setPersona(saved)
        emitAppEvent('arael-persona-updated')
      } catch (reason) {
        reportMeropeError(userFacingError(reason, o.saveFailed))
      }
    },
    [o.saveFailed, persona, setPersona],
  )

  /** Puts back what one key of her visual profile held (`gender`, …). */
  const restoreVisual = useCallback(
    async (key: string, value: unknown) => {
      if (!persona) return
      try {
        await agentService.putPersona({
          name: persona.name,
          personality: persona.personality ?? '',
          visualProfile: { ...(persona.visualProfile ?? {}), [key]: value },
        })
        await reload()
        await loadFace()
        emitAppEvent('arael-persona-updated')
      } catch (reason) {
        reportMeropeError(userFacingError(reason, o.visualDesignSaveFailed))
      }
    },
    [loadFace, o.visualDesignSaveFailed, persona, reload],
  )

  return { restoreName, restoreVisual }
}

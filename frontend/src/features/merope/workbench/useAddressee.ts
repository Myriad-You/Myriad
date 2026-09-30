import type { Dispatch, SetStateAction } from 'react'
import type { AgentPersona } from '../../../services/agent/agentApi'
import { useCallback, useState } from 'react'
import { ADDRESSEE_UPDATED_EVENT } from '../../../components/agent/meropeVitals'
import { agentService } from '../../../services/agent'
import { userFacingError } from '../../../utils/userFacingError'
import { reportMeropeError } from './workbenchShared'

interface SavedAddressee {
  mood: number
  activity: string
  doNotDisturb: boolean
  doNotDisturbActive?: boolean
  dndStart?: string | null
  dndEnd?: string | null
}

/** Do-not-disturb: the switch and the quiet hours. */
export function useAddressee(
  setPersonaSnapshot: Dispatch<SetStateAction<AgentPersona | null>>,
  saveFailed: string,
) {
  const [doNotDisturb, setDoNotDisturb] = useState(false)
  const [dndStart, setDndStart] = useState('')
  const [dndEnd, setDndEnd] = useState('')
  const [busy, setBusy] = useState(false)

  const hydrate = useCallback((persona: AgentPersona | null) => {
    setDoNotDisturb(persona?.doNotDisturb === true)
    setDndStart(persona?.dndStart?.trim() || '')
    setDndEnd(persona?.dndEnd?.trim() || '')
  }, [])

  const apply = useCallback(
    (saved: SavedAddressee) => {
      setDoNotDisturb(saved.doNotDisturb)
      setDndStart(saved.dndStart?.trim() || '')
      setDndEnd(saved.dndEnd?.trim() || '')
      setPersonaSnapshot((current) =>
        current
          ? {
              ...current,
              doNotDisturb: saved.doNotDisturb,
              doNotDisturbActive: saved.doNotDisturbActive,
              dndStart: saved.dndStart ?? null,
              dndEnd: saved.dndEnd ?? null,
              mood: saved.mood,
              activity: saved.activity,
            }
          : current,
      )
      window.dispatchEvent(new CustomEvent(ADDRESSEE_UPDATED_EVENT))
    },
    [setPersonaSnapshot],
  )

  const saveDoNotDisturb = useCallback(
    async (next: boolean) => {
      const previous = doNotDisturb
      setDoNotDisturb(next)
      setBusy(true)
      try {
        apply(await agentService.putAddressee({ doNotDisturb: next }))
      } catch (reason) {
        setDoNotDisturb(previous)
        reportMeropeError(userFacingError(reason, saveFailed))
      } finally {
        setBusy(false)
      }
    },
    [apply, doNotDisturb, saveFailed],
  )

  const saveSchedule = useCallback(
    async (start: string, end: string) => {
      setBusy(true)
      try {
        apply(
          await agentService.putAddressee({
            dndStart: start,
            dndEnd: end,
          }),
        )
      } catch (reason) {
        reportMeropeError(userFacingError(reason, saveFailed))
      } finally {
        setBusy(false)
      }
    },
    [apply, saveFailed],
  )

  /** Quiet hours are saved once both ends are set, or both cleared. */
  const commitHours = () => {
    if ((dndStart && dndEnd) || (!dndStart && !dndEnd)) {
      void saveSchedule(dndStart, dndEnd)
    }
  }

  return {
    doNotDisturb,
    dndStart,
    setDndStart,
    dndEnd,
    setDndEnd,
    busy,
    hydrate,
    saveDoNotDisturb,
    commitHours,
  }
}

export type Addressee = ReturnType<typeof useAddressee>

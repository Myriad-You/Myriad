/** Homepage tiles share the overlay cache; unmount only drops the apply. */

import type { PhantasiSource } from '../../../types/phantasi'
import { useCallback, useEffect, useRef, useState } from 'react'
import { getSources } from '../../../services/phantasiApi'
import { RequestTurn } from '../logic/requestTurn'

export interface WidgetSourcesState {
  sources: PhantasiSource[]
  loading: boolean
  failed: boolean
}

export function useWidgetSources(
  isPreview: boolean,
  failLabel: string,
): WidgetSourcesState {
  const [sources, setSources] = useState<PhantasiSource[]>([])
  const [loading, setLoading] = useState(!isPreview)
  const [failed, setFailed] = useState(false)
  const turns = useRef(new RequestTurn())

  const load = useCallback(async () => {
    if (isPreview) return
    const signal = turns.current.begin()
    try {
      const next = await getSources()
      if (signal.aborted) return
      setSources(next)
      setFailed(false)
      setLoading(false)
    } catch (error) {
      if (signal.aborted) return
      console.error(`${failLabel} failed to load sources:`, error)
      setFailed(true)
      setLoading(false)
    }
  }, [failLabel, isPreview])

  useEffect(() => {
    void load()
    return () => turns.current.cancel()
  }, [load])

  return { sources, loading, failed }
}

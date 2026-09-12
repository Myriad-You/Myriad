import type { ControlMode } from './modes'
import { useCallback, useEffect, useState } from 'react'
import {
  waveAfterBoardEdit,
  waveFromLane,
  waveIfEditTargetLost,
  waveOnChange,
  waveOnClose,
} from './barWave'

export function useBarWave({
  starredEdit,
  hasStarred,
  hasTopic,
  isEditMode,
  hasSelectedSource,
  onEnterEditMode,
  onExitEditMode,
  setSearchQuery,
}: {
  starredEdit: boolean
  hasStarred: boolean
  hasTopic: boolean
  isEditMode: boolean
  hasSelectedSource: boolean
  onEnterEditMode?: () => void
  onExitEditMode?: () => void
  setSearchQuery?: (query: string) => void
}) {
  const [mode, setMode] = useState<ControlMode>(() =>
    waveFromLane({ starredEdit, hasStarred, hasTopic }),
  )

  useEffect(() => {
    setMode(waveFromLane({ starredEdit, hasStarred, hasTopic }))
  }, [starredEdit, hasStarred, hasTopic])

  useEffect(() => {
    const next = waveAfterBoardEdit(isEditMode, mode)
    if (next) setMode(next)
  }, [isEditMode, mode])

  useEffect(() => {
    const next = waveIfEditTargetLost(mode, hasSelectedSource, isEditMode)
    if (next) setMode(next)
  }, [isEditMode, mode, hasSelectedSource])

  const changeMode = useCallback(
    (next: ControlMode) => {
      const change = waveOnChange(next, mode)
      if (change.enterEdit) onEnterEditMode?.()
      else if (change.exitEdit) onExitEditMode?.()
      if (change.clearSearch) setSearchQuery?.('')
      setMode(next)
    },
    [mode, onEnterEditMode, onExitEditMode, setSearchQuery],
  )

  const close = useCallback(() => {
    const next = waveOnClose(mode)
    if (next.exitEdit) onExitEditMode?.()
    setMode(next.mode)
  }, [mode, onExitEditMode])

  return { mode, setMode, changeMode, close }
}

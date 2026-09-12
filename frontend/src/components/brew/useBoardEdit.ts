/** skin 不进口。 */

import type { BrewSource } from '../../types/brew'
import type { BrewBoard } from './logic/board'
import type { BrewControlsHandle } from './manager/modes'

import { useCallback, useEffect, useRef, useState } from 'react'
import { showBrewError } from './brewNotice'

export function useBoardEdit(
  board: BrewBoard,
  filtered: BrewSource[],
  onRemoveSources: ((ids: number[]) => Promise<void>) | undefined,
  onRefreshSource: (sourceId: number) => void,
  deleteFailed: string,
  refreshFailed: string,
) {
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())
  const [isEditMode, setIsEditMode] = useState(false)
  const [boardEdit, setBoardEdit] = useState(false)
  const [sitesOpen, setSitesOpen] = useState(false)
  const [isDeleting, setIsDeleting] = useState(false)
  const [isRefreshing, setIsRefreshing] = useState(false)
  const barRef = useRef<BrewControlsHandle>(null)

  const handleToggleSelect = useCallback((sourceId: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(sourceId)) next.delete(sourceId)
      else next.add(sourceId)
      return next
    })
  }, [])

  const handleSelectAll = useCallback(() => {
    setSelectedIds((prev) =>
      prev.size === filtered.length
        ? new Set()
        : new Set(filtered.map((source) => source.id)),
    )
  }, [filtered])

  const handleEnterEditMode = useCallback(() => setIsEditMode(true), [])
  const handleExitEditMode = useCallback(() => {
    setIsEditMode(false)
    setSelectedIds(new Set())
  }, [])
  const handleWaveDisplayed = useCallback((wave: string) => {
    setBoardEdit(wave === 'edit' || wave === 'source-edit')
  }, [])

  const handleOpenSourceEdit = useCallback((source: BrewSource) => {
    setSelectedIds(new Set([source.id]))
    setIsEditMode(true)
    barRef.current?.changeMode('source-edit')
  }, [])

  useEffect(() => {
    if (board === 'feeds' && !sitesOpen && isEditMode) {
      handleExitEditMode()
    }
  }, [board, sitesOpen, isEditMode, handleExitEditMode])

  const handleBatchDelete = useCallback(async () => {
    if (selectedIds.size === 0 || !onRemoveSources) return
    const ids = Iterator.from(selectedIds).toArray()
    setIsDeleting(true)
    try {
      await onRemoveSources?.(ids)
      setSelectedIds(new Set())
    } catch (err) {
      await showBrewError(err, deleteFailed)
    } finally {
      setIsDeleting(false)
    }
  }, [selectedIds, onRemoveSources, deleteFailed])

  const handleBatchRefresh = useCallback(async () => {
    const refreshable = filtered.filter(
      (source) => source.source_type !== 'link',
    )
    if (refreshable.length === 0) return
    setIsRefreshing(true)
    try {
      await Promise.all(refreshable.map((source) => onRefreshSource(source.id)))
    } catch (err) {
      await showBrewError(err, refreshFailed)
    } finally {
      setIsRefreshing(false)
    }
  }, [filtered, onRefreshSource, refreshFailed])

  return {
    selectedIds,
    isEditMode,
    boardEdit,
    sitesOpen,
    setSitesOpen,
    isDeleting,
    isRefreshing,
    barRef,
    handleToggleSelect,
    handleSelectAll,
    handleEnterEditMode,
    handleExitEditMode,
    handleWaveDisplayed,
    handleOpenSourceEdit,
    handleBatchDelete,
    handleBatchRefresh,
  }
}

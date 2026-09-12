/** 收藏不是板块。 */

import type { Dispatch, SetStateAction } from 'react'
import type { BrewItem } from '../../types/brew'

import { useCallback, useRef, useState } from 'react'
import * as brewApi from '../../services/brewApi'
import { reportBrewError } from './brewNotice'
import { dropItems, dropStarredId } from './logic/itemState'

export function useBrewStarred(
  items: BrewItem[],
  setItems: Dispatch<SetStateAction<BrewItem[]>>,
  setTotal: Dispatch<SetStateAction<number>>,
  starFailed: string,
  setError: (message: string) => void,
) {
  const [editMode, setEditMode] = useState(false)
  const [selectedIds, setSelectedIds] = useState<Set<number>>(new Set())
  const [processing, setProcessing] = useState(false)
  const processingRef = useRef(false)

  const enterEdit = useCallback(() => {
    setEditMode(true)
    setSelectedIds(new Set())
  }, [])

  const exitEdit = useCallback(() => {
    setEditMode(false)
    setSelectedIds(new Set())
  }, [])

  const selectAll = useCallback(() => {
    setSelectedIds((prev) =>
      prev.size === items.length
        ? new Set()
        : new Set(items.map((item) => item.id)),
    )
  }, [items])

  const toggle = useCallback((id: number) => {
    setSelectedIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }, [])

  const unselect = useCallback((id: number) => {
    setSelectedIds((prev) => dropStarredId(prev, id))
  }, [])

  const batchUnstar = useCallback(async () => {
    if (selectedIds.size === 0 || processingRef.current) return
    processingRef.current = true
    setProcessing(true)
    try {
      const ids = Iterator.from(selectedIds).toArray()
      const results = await Promise.allSettled(
        ids.map((id) => brewApi.unstarItem(id)),
      )
      const succeeded = new Set(ids.filter((_, index) => results[index].status === 'fulfilled'))
      setItems((prev) => dropItems(prev, succeeded))
      setTotal((prev) => Math.max(0, prev - succeeded.size))
      setSelectedIds((prev) => prev.difference(succeeded))
      const failure = results.find((result) => result.status === 'rejected')
      if (failure?.status === 'rejected') {
        reportBrewError(failure.reason, starFailed, setError)
      } else {
        exitEdit()
      }
    } catch (err) {
      reportBrewError(err, starFailed, setError)
    } finally {
      processingRef.current = false
      setProcessing(false)
    }
  }, [
    selectedIds,
    setItems,
    setTotal,
    exitEdit,
    starFailed,
    setError,
  ])

  return {
    editMode,
    selectedIds,
    processing,
    enterEdit,
    exitEdit,
    selectAll,
    toggle,
    unselect,
    batchUnstar,
  }
}

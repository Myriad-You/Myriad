import type { Dispatch, SetStateAction } from 'react'
import type { BrewItem } from '../../types/brew'
import type { BrewViewMode } from './logic/board'

import { useCallback, useEffect, useRef, useState } from 'react'
import * as brewApi from '../../services/brewApi'
import { dropItem } from './logic/itemState'
import { RequestTurn } from './logic/requestTurn'

export function useBrewNotes(
  selectedItem: BrewItem | null,
  setSelectedItem: Dispatch<SetStateAction<BrewItem | null>>,
  setItems: Dispatch<SetStateAction<BrewItem[]>>,
  viewMode: BrewViewMode,
  topicKey: string | undefined,
  loadItems: (reset?: boolean) => Promise<void>,
  reloadBoard: () => void,
  loadSources: () => Promise<void>,
  loadStats: () => Promise<void>,
) {
  const [noteEditor, setNoteEditor] = useState<number | 'new' | null>(null)
  const saveTurn = useRef(new RequestTurn())
  useEffect(() => () => saveTurn.current.cancel(), [])

  const onSaved = useCallback(
    async (id: number) => {
      setNoteEditor(null)
      await Promise.all([loadSources(), loadStats()])
      if (selectedItem?.id === id) {
        const signal = saveTurn.current.begin()
        try {
          const fresh = await brewApi.getItem(id, undefined, { signal })
          if (!signal.aborted) {
            setSelectedItem((current) => (current?.id === id ? fresh : current))
          }
        } catch {
          /* 取不回来就保持原样 */
        }
      }
      if (viewMode === 'topic-feed' && topicKey) {
        void loadItems(true)
      }
    },
    [
      selectedItem?.id,
      setSelectedItem,
      viewMode,
      topicKey,
      loadItems,
      loadSources,
      loadStats,
    ],
  )

  const onDeleted = useCallback(
    (id: number) => {
      setNoteEditor(null)
      setItems((prev) => dropItem(prev, id))
      if (selectedItem?.id === id) setSelectedItem(null)
      reloadBoard()
    },
    [selectedItem?.id, setItems, setSelectedItem, reloadBoard],
  )

  const write = useCallback(() => setNoteEditor('new'), [])
  const edit = useCallback((id: number) => setNoteEditor(id), [])
  const close = useCallback(() => setNoteEditor(null), [])

  return { noteEditor, write, edit, close, onSaved, onDeleted }
}

import type { SortOption } from './types'

import {
  LuClock as Clock,
  LuFolderOpen as FolderOpen,
  LuSortAsc as SortAsc,
  LuSparkles as Sparkles,
} from '@lib/icons'

export function buildBrewSortOptions(): SortOption[] {
  return [
    {
      value: 'smart',
      labelKey: 'sortBySmart',
      icon: <Sparkles className="w-4 h-4" />,
    },
    {
      value: 'update',
      labelKey: 'sortByUpdate',
      icon: <Clock className="w-4 h-4" />,
    },
    {
      value: 'category',
      labelKey: 'sortByCategory',
      icon: <FolderOpen className="w-4 h-4" />,
    },
    {
      value: 'pinyin',
      labelKey: 'sortByPinyin',
      icon: <SortAsc className="w-4 h-4" />,
    },
  ]
}

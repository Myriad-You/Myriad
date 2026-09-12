/** 不碰 DOM。 */

import type { ControlMode } from './modes'

export function waveFromLane(flags: {
  starredEdit: boolean
  hasStarred: boolean
  hasTopic: boolean
}): ControlMode {
  if (flags.starredEdit) return 'starred-edit'
  if (flags.hasStarred) return 'starred'
  if (flags.hasTopic) return 'topic-feed'
  return 'default'
}

/** null = 不动。 */
export function waveAfterBoardEdit(
  isEditMode: boolean,
  mode: ControlMode,
): ControlMode | null {
  if (isEditMode && mode !== 'edit' && mode !== 'source-edit') return 'edit'
  if (!isEditMode && (mode === 'edit' || mode === 'source-edit')) return 'default'
  return null
}

/** 单选源没了，全屏编辑必须退回。 */
export function waveIfEditTargetLost(
  mode: ControlMode,
  hasSelectedSource: boolean,
  isEditMode: boolean,
): ControlMode | null {
  if (mode !== 'source-edit' || hasSelectedSource) return null
  return isEditMode ? 'edit' : 'default'
}

export function waveOnClose(mode: ControlMode): {
  mode: ControlMode
  exitEdit: boolean
} {
  if (mode === 'source-edit') return { mode: 'edit', exitEdit: false }
  return { mode: 'default', exitEdit: mode === 'edit' }
}

export function waveOnChange(
  next: ControlMode,
  current: ControlMode,
): { enterEdit: boolean; exitEdit: boolean; clearSearch: boolean } {
  const nextEdit = next === 'edit' || next === 'source-edit'
  const currentEdit = current === 'edit' || current === 'source-edit'
  return {
    enterEdit: nextEdit,
    exitEdit: !nextEdit && currentEdit,
    clearSearch: next === 'search',
  }
}

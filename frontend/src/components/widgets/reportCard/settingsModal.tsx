import type { ReportCardClickAction } from './types'
import { memo, useCallback, useEffect, useState } from 'react'
import { useI18n } from '../../../contexts/I18nContext'
import {
  WidgetSettingsChoice,
  WidgetSettingsChoices,
  WidgetSettingsSection,
  WidgetSettingsTip,
} from '../shared/WidgetSettingsTip'

interface ReportCardSettingsModalState {
  isOpen: boolean
  selectedAction: ReportCardClickAction
  title: string
  anchorRect?: DOMRect
  onSelect?: (action: ReportCardClickAction) => void
  onClose?: () => void
}

let reportCardSettingsModalState: ReportCardSettingsModalState = {
  isOpen: false,
  selectedAction: 'report',
  title: '',
}

const reportCardSettingsModalListeners: Set<() => void> = new Set()

export function openReportCardSettingsModal(
  selectedAction: ReportCardClickAction,
  anchorRect: DOMRect,
  onSelect: (action: ReportCardClickAction) => void,
  onClose?: () => void,
  title = '',
) {
  reportCardSettingsModalState = {
    isOpen: true,
    selectedAction,
    title,
    anchorRect,
    onSelect,
    onClose,
  }
  reportCardSettingsModalListeners.forEach((listener) => listener())
}

export function closeReportCardSettingsModal() {
  const onClose = reportCardSettingsModalState.onClose
  reportCardSettingsModalState = {
    ...reportCardSettingsModalState,
    isOpen: false,
    onClose: undefined,
  }
  onClose?.()
  reportCardSettingsModalListeners.forEach((listener) => listener())
}

export function subscribeToReportCardSettingsModal(listener: () => void) {
  reportCardSettingsModalListeners.add(listener)
  return () => {
    reportCardSettingsModalListeners.delete(listener)
  }
}

export const ReportCardSettingsModal = memo(() => {
  const [, forceUpdate] = useState({})
  const { t } = useI18n()

  useEffect(() => {
    return subscribeToReportCardSettingsModal(() => {
      forceUpdate({})
    })
  }, [])

  const { isOpen, selectedAction, title, anchorRect, onSelect } =
    reportCardSettingsModalState

  const handleSelect = useCallback(
    (action: ReportCardClickAction) => {
      onSelect?.(action)
      closeReportCardSettingsModal()
    },
    [onSelect],
  )

  return (
    <WidgetSettingsTip
      open={isOpen}
      anchor={anchorRect ?? null}
      title={title.trim() || t.widgetGrid.widgetSettings}
      width={300}
      height={220}
      onClose={closeReportCardSettingsModal}
    >
      <WidgetSettingsSection label={t.platformCard.settingsTitle}>
        <WidgetSettingsChoices label={t.platformCard.settingsTitle}>
          <WidgetSettingsChoice
            selected={selectedAction === 'social'}
            label={t.platformCard.clickToSocial}
            onClick={() => handleSelect('social')}
          />
          <WidgetSettingsChoice
            selected={selectedAction === 'report'}
            label={t.platformCard.clickToReport}
            onClick={() => handleSelect('report')}
          />
        </WidgetSettingsChoices>
      </WidgetSettingsSection>
    </WidgetSettingsTip>
  )
})

ReportCardSettingsModal.displayName = 'ReportCardSettingsModal'

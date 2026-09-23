import { FaExclamationTriangle, LuRefreshCw } from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import React, { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { useConfigI18n as useI18n } from '../../../contexts/I18nContext'
import { settingsAgentLabel } from '../../../features/merope/publicName'
import { usePersonaPublicName } from '../../../features/merope/usePersonaPublicName'
import { emitAppEvent } from '../../../utils/appEvents'
import { AgentConfigSection } from '../../config'
import { ConfigDefaultsProvider } from '../../config/ConfigDefaultsProvider'
import {
  useConfigEditor,
  useConfigMessage,
  useConfigSessionKey,
} from '../../config/form'
import { useAgentDomain } from '../../config/form/domains/useAgentDomain'
import MyriadConfigIcon from '../../config/MyriadConfigIcon'
import {
  SETTINGS_PAGE_MOTION,
  SettingsButton,
  SettingsPageActionsProvider,
} from '../../settings'
import { scheduleScrollToSettingGuide } from '../../settings/guides/guideAnchor'
import { Spinner } from '../../Spinner'
import '../../ConfigForm.css'

const AgentSettingsEditor: React.FC = () => {
  const { t } = useI18n()
  const navigate = useNavigate()
  const { showMessage } = useConfigMessage()
  const personaName = usePersonaPublicName()
  const agentTitle = settingsAgentLabel(t.config.agent, personaName)
  const agent = useAgentDomain(t.config)
  const editor = useConfigEditor([agent], showMessage, t.config)
  const { isDirty, saving, save, load, reset } = editor

  const contentReady = agent.ready && !agent.loading && !agent.error
  useEffect(() => {
    if (contentReady) emitAppEvent('config-loaded')
  }, [contentReady])

  useEffect(() => {
    if (!contentReady || typeof window === 'undefined') return
    const guide = new URLSearchParams(window.location.search).get('guide')
    if (!guide) return
    scheduleScrollToSettingGuide(guide)
  }, [contentReady])

  useEffect(() => {
    const handleSaveEvent = () => void save()
    const handleResetEvent = () => void reset('agent')
    window.addEventListener('request-config-save', handleSaveEvent)
    window.addEventListener('config-reset', handleResetEvent)
    return () => {
      window.removeEventListener('request-config-save', handleSaveEvent)
      window.removeEventListener('config-reset', handleResetEvent)
    }
  }, [reset, save])

  if (agent.loading || (!agent.ready && !agent.error)) {
    return (
      <div className="modern-config-loading" role="status" aria-live="polite">
        <Spinner size="lg" color="primary" />
      </div>
    )
  }

  if (agent.error) {
    return (
      <div
        className="modern-config-error"
        role="alert"
        aria-live="assertive"
        aria-labelledby="agent-settings-load-error-title"
      >
        <div className="modern-config-error-card">
          <div className="modern-config-error-visual" aria-hidden="true">
            <span className="modern-config-error-icon">
              <FaExclamationTriangle />
            </span>
          </div>
          <div className="modern-config-error-copy">
            <h2 id="agent-settings-load-error-title">
              {t.config.loadConfigFailed}
            </h2>
            <p>{t.config.loadConfigFailedDesc}</p>
          </div>
          <div className="modern-config-error-actions">
            <SettingsButton
              variant="secondary"
              size="md"
              onClick={() => navigate('/config')}
            >
              {t.nav.config}
            </SettingsButton>
            <SettingsButton
              variant="primary"
              size="md"
              icon={<LuRefreshCw />}
              onClick={() => void load()}
            >
              {t.common.retry}
            </SettingsButton>
          </div>
        </div>
      </div>
    )
  }

  return (
    <motion.div
      className="modern-config-container"
      initial={SETTINGS_PAGE_MOTION.initial}
      animate={SETTINGS_PAGE_MOTION.animate}
      exit={SETTINGS_PAGE_MOTION.exit}
      transition={SETTINGS_PAGE_MOTION.transition}
    >
      <div
        className="config-shell config-shell--solo"
        data-mobile-pane="desktop"
      >
        <div className="config-content" data-tour="config-content">
          <SettingsPageActionsProvider
            value={{
              resetCurrentPage: () => reset('agent'),
              canResetCurrentPage: true,
            }}
          >
            <AgentConfigSection
              configFields={agent.draft.aiFields}
              updateValue={agent.updateAiFieldValue}
              uiConfigFields={agent.draft.uiFields}
              updateUiFieldValue={agent.updateUiFieldValue}
              title={agentTitle}
              icon={<MyriadConfigIcon kind="agent" />}
              description={t.config.agentDesc}
              sectionId="agent"
              onLeave={() => navigate('/config')}
              leaveLabel={t.nav.config}
            />
          </SettingsPageActionsProvider>
        </div>
      </div>

      {isDirty && (
        <div className="floating-save-container">
          <SettingsButton
            variant="primary"
            className="floating-save-btn"
            onClick={() => void save()}
            aria-label={t.config.saveConfigLabel}
            disabled={saving}
            loading={saving}
            icon={
              <svg
                fill="none"
                stroke="currentColor"
                viewBox="0 0 24 24"
                width="20"
                height="20"
              >
                <path
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  strokeWidth={2}
                  d="M5 13l4 4L19 7"
                />
              </svg>
            }
          >
            {saving ? t.config.savingConfig : t.config.saveConfig}
          </SettingsButton>
        </div>
      )}
    </motion.div>
  )
}

export default function AgentSettingsForm() {
  const sessionKey = useConfigSessionKey()
  return (
    <ConfigDefaultsProvider>
      <AgentSettingsEditor key={sessionKey} />
    </ConfigDefaultsProvider>
  )
}

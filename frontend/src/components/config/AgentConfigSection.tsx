import type { OnboardingPageChrome } from '../agent/onboarding/onboardingTypes'
import type { AgentChannelId } from './agentChannels'
import {
  LuChevronLeft,
  LuMessageSquare,
  LuNotebookPen,
  LuRefreshCw,
} from '@lib/icons'

import React, { useCallback, useEffect, useMemo, useState } from 'react'
import { useConfigI18n as useI18n } from '../../contexts/I18nContext'
import {
  FACE_UPDATED_EVENT,
} from '../../features/merope/events'
import SiteMotionWorkbench from '../../features/merope/SiteMotionWorkbench'
import { agentService } from '../../services/agent'
import { emitAppEvent } from '../../utils/appEvents'
import { invalidatePublicConfigCache } from '../../utils/requestDedup'
import { siteMediaUrl } from '../../utils/siteMediaUrl'
import { showStickyToast } from '../../utils/toastManager'
import { userFacingError } from '../../utils/userFacingError'
import {
  activityKey,
  ADDRESSEE_UPDATED_EVENT,
  moodBand,
} from '../agent/meropeVitals'
import { parseFlattenedPersona } from '../agent/onboarding/onboardingTypes'
import PersonaOnboardingPage from '../agent/onboarding/PersonaOnboardingPage'
import {
  AutoHeight,
  InfoActionCard,
  SettingGroup,
  SettingsButton,
  SettingSection,
  SettingTitleTag,
  useSettingGuide,
} from '../settings'
import { clearAgentChannelValues, visibleAgentChannels } from './agentChannels'
import {
  AgentChannelAddTrigger,
  AgentChannelSources,
} from './AgentChannelSources'
import AgentOptionsPanel, {
  AgentNestedSection,
} from './AgentOptionsPanel'
import { useAddedSlug } from './useAddedCard'
import { useAiSubpage } from './usePersonaPage'

interface ConfigField {
  key: string
  label: string
  field_type: string
  value: string
  placeholder: string
  required: boolean
}

interface AgentConfigSectionProps {
  configFields: ConfigField[]
  updateValue: (key: string, value: string) => void
  uiConfigFields: Array<{ key: string; value: string }>
  updateUiFieldValue: (key: string, value: string) => void
  title: string
  icon: React.ReactNode
  description: string
  sectionId?: string
  onLeave?: () => void
  leaveLabel?: string
}

export const AgentConfigSection: React.FC<AgentConfigSectionProps> = ({
  configFields,
  updateValue,
  uiConfigFields,
  updateUiFieldValue,
  title,
  icon,
  description,
  sectionId,
  onLeave,
  leaveLabel,
}) => {
  const { t } = useI18n()
  const { catalog: g, bindGuide } = useSettingGuide()
  const [personaChrome, setPersonaChrome] = useState<OnboardingPageChrome | null>(
    null,
  )
  const {
    page: aiSubpage,
    navDir: aiPaneNav,
    openPage: openAiSubpage,
    closePage: closeAiSubpage,
  } = useAiSubpage((page) => {
    if (page === 'merope-setup') setPersonaChrome(null)
  })
  const setupPage = aiSubpage === 'merope-setup'
  const meropePage = aiSubpage === 'merope'
  const subpageOpen = aiSubpage != null

  const [revealedChannels, setRevealedChannels] = useState<AgentChannelId[]>(
    [],
  )

  const getFieldValue = useCallback(
    (key: string, defaultValue = '') => {
      return configFields.find((f) => f.key === key)?.value || defaultValue
    },
    [configFields],
  )

  const liteEnabled = useMemo(() => {
    const val = getFieldValue('lite_enabled', 'false')
    return val === 'true' || val === '1'
  }, [getFieldValue])

  const agentPersonaEnabled = useMemo(
    () =>
      uiConfigFields.find((field) => field.key === 'merope_enabled')
        ?.value === 'true',
    [uiConfigFields],
  )

  const agentPersonaSpeechEnabled = useMemo(
    () =>
      uiConfigFields.find((field) => field.key === 'merope_speech_enabled')
        ?.value === 'true',
    [uiConfigFields],
  )

  const proEnabled = useMemo(() => {
    const val = getFieldValue('pro_enabled', 'false')
    return val === 'true' || val === '1'
  }, [getFieldValue])

  const visibleChannels = useMemo(
    () => visibleAgentChannels(getFieldValue, revealedChannels),
    [getFieldValue, revealedChannels],
  )
  const addedChannel = useAddedSlug(visibleChannels)

  const addChannel = useCallback((id: AgentChannelId) => {
    setRevealedChannels((current) =>
      current.includes(id) ? current : [...current, id],
    )
  }, [])

  const removeChannel = useCallback(
    (id: AgentChannelId) => {
      setRevealedChannels((current) => current.filter((item) => item !== id))
      clearAgentChannelValues(id, updateValue)
    },
    [updateValue],
  )

  const o = t.agentPersona.onboarding
  const paneKey = aiSubpage ?? 'agent'
  const personaGuide = bindGuide('agent.agentPersona', g.agent.agentPersona)
  const meropeOn = agentPersonaEnabled && proEnabled
  const [savedPersonaName, setSavedPersonaName] = useState('')
  const [hasSavedPersona, setHasSavedPersona] = useState(false)
  const [mood, setMood] = useState(70)
  const [arousal, setArousal] = useState(48)
  const [activity, setActivity] = useState('idle')
  const [personality, setPersonality] = useState('')
  const [portraitUrl, setPortraitUrl] = useState<string | null>(null)
  const [vitalsReady, setVitalsReady] = useState(false)
  const [personaBusy, setPersonaBusy] = useState(false)

  const handleDeletePersona = useCallback(async () => {
    setPersonaBusy(true)
    try {
      await agentService.deletePersona()
      invalidatePublicConfigCache()
      emitAppEvent('arael-persona-updated')
    } catch (error) {
      showStickyToast({
        message: userFacingError(error, t.config.agentPersonaDeleteFailed),
        type: 'error',
        replaceKey: 'config-persona',
      })
    } finally {
      setPersonaBusy(false)
    }
  }, [t.config.agentPersonaDeleteFailed])

  useEffect(() => {
    if (!meropeOn) {
      setSavedPersonaName('')
      setHasSavedPersona(false)
      setMood(70)
      setArousal(48)
      setActivity('idle')
      setPersonality('')
      setPortraitUrl(null)
      setVitalsReady(false)
      return
    }
    let cancelled = false
    const load = () => {
      void agentService
        .getPersona()
        .then((persona) => {
          if (cancelled || !persona) return
          const name = persona.name?.trim() ?? ''
          setSavedPersonaName(name)
          setHasSavedPersona(
            persona.hasCustomPersona === true ||
              name.length > 0 ||
              Boolean(persona.personality?.trim()),
          )
          setMood(typeof persona.mood === 'number' ? persona.mood : 70)
          setArousal(
            typeof persona.arousal === 'number' ? persona.arousal : 48,
          )
          setActivity(persona.activity ?? 'idle')
          setPersonality(persona.personality?.trim() ?? '')
          setPortraitUrl(
            typeof persona.portraitAssetId === 'string' &&
              persona.portraitAssetId.trim()
              ? persona.portraitAssetId
              : null,
          )
          setVitalsReady(true)
        })
        .catch(() => {
          if (!cancelled) {
            setSavedPersonaName('')
            setHasSavedPersona(false)
            setPersonality('')
            setPortraitUrl(null)
            setVitalsReady(false)
          }
        })
    }
    load()
    window.addEventListener('arael-persona-updated', load)
    window.addEventListener(FACE_UPDATED_EVENT, load)
    window.addEventListener(ADDRESSEE_UPDATED_EVENT, load)
    return () => {
      cancelled = true
      window.removeEventListener('arael-persona-updated', load)
      window.removeEventListener(FACE_UPDATED_EVENT, load)
      window.removeEventListener(ADDRESSEE_UPDATED_EVENT, load)
    }
  }, [meropeOn])
  const personaGateLead = !proEnabled
    ? t.config.agentPersonaNeedsPro
    : meropeOn && !liteEnabled
      ? t.config.agentPersonaNeedsLite
      : t.config.agentPersonaHint

  const personaCardCopy = useMemo(() => {
    if (!meropeOn || !hasSavedPersona) return null
    const summary = parseFlattenedPersona(personality).summary.replaceAll(/\s+/g, ' ').trim()
    return {
      summary,
      mood: vitalsReady ? o.mood[moodBand(mood, arousal)] : '—',
      activity: vitalsReady ? o.activity[activityKey(activity)] : '—',
    }
  }, [
    activity,
    arousal,
    hasSavedPersona,
    meropeOn,
    mood,
    o,
    personality,
    vitalsReady,
  ])

  return (
    <SettingSection
      sectionId={sectionId}
      className={setupPage ? 'setting-section--persona' : undefined}
      title={
        setupPage
          ? (personaChrome?.title ?? o.step1Title)
          : meropePage
            ? t.merope.adminTitle
            : title
      }
      icon={subpageOpen ? undefined : icon}
      description={
        setupPage
          ? (personaChrome?.description ?? o.step1Lead)
          : meropePage
            ? t.merope.adminDescription
            : description
      }
      detail={
        setupPage
          ? (personaChrome?.description ?? o.step1Lead)
          : meropePage
            ? t.merope.adminDescription
            : undefined
      }
      detailTone={setupPage ? personaChrome?.detailTone : undefined}
      showResetPage={subpageOpen ? false : undefined}
      {...(setupPage ? personaGuide : {})}
      headerActions={
        setupPage && personaChrome?.action ? (
          <SettingsButton
            variant="secondary"
            size="sm"
            icon={<LuRefreshCw size={14} />}
            loading={personaChrome.action.busy}
            disabled={personaChrome.action.disabled}
            onClick={personaChrome.action.onClick}
          >
            {personaChrome.action.label}
          </SettingsButton>
        ) : null
      }
      headerLeading={
        subpageOpen ? (
          <button
            type="button"
            className="section-header-back"
            onClick={() =>
              setupPage
                ? (personaChrome?.onBack ?? closeAiSubpage)()
                : closeAiSubpage()
            }
            disabled={setupPage ? personaChrome?.backDisabled : false}
            aria-label={
              setupPage
                ? (personaChrome?.backAria ?? t.common.back)
                : t.common.back
            }
          >
            <LuChevronLeft size={18} aria-hidden />
            <span>{t.common.back}</span>
          </button>
        ) : onLeave ? (
          <button
            type="button"
            className="section-header-back"
            onClick={onLeave}
            aria-label={leaveLabel ?? t.common.back}
          >
            <LuChevronLeft size={18} aria-hidden />
            <span>{leaveLabel ?? t.common.back}</span>
          </button>
        ) : undefined
      }
      headerBetweenPinned={
        subpageOpen ? undefined : (
          <AgentChannelAddTrigger
            visible={visibleChannels}
            onAdd={addChannel}
          />
        )
      }
    >
      <AutoHeight contentKey={paneKey} animate={false}>
        <div key={paneKey} data-nav={aiPaneNav} className="agent-pane sm-pane">
          {setupPage ? (
            <PersonaOnboardingPage
              onBack={closeAiSubpage}
              onFinished={() => openAiSubpage('merope')}
              onChromeChange={setPersonaChrome}
              meropeOn={meropeOn}
              gateLead={personaGateLead}
            />
          ) : meropePage ? (
            <SiteMotionWorkbench
              mood={mood}
              arousal={arousal}
              activity={activity}
            />
          ) : (
            <>
      <SettingGroup
        title={t.config.agentPersona}
        icon={<LuNotebookPen />}
        description={personaGateLead}
        titleExtra={
          <SettingTitleTag variant="beta">
            {t.config.agentPersonaBeta}
          </SettingTitleTag>
        }
        switch={{
          checked: meropeOn,
          onChange: (value) =>
            updateUiFieldValue('merope_enabled', value ? 'true' : 'false'),
          disabled: !proEnabled,
          ariaLabel: t.config.agentPersona,
          tourAnchor: 'config-ai-persona-toggle',
        }}
        {...bindGuide('agent.agentPersona', g.agent.agentPersona)}
      >
        {meropeOn ? (
          <div data-tour="config-ai-persona-card">
          <InfoActionCard
            copyable={false}
            tone={!liteEnabled ? 'info' : 'default'}
            title={
              hasSavedPersona
                ? savedPersonaName || 'Arael'
                : t.config.agentPersonaEmpty
            }
            preview={
              portraitUrl && hasSavedPersona ? (
                <img src={siteMediaUrl(portraitUrl)} alt={savedPersonaName || 'Arael'} />
              ) : (
                <span className="info-action-card-preview-empty is-mosaic">
                  <img src="/merope/clothing/everyday.png" alt="" />
                  <img src="/merope/clothing/fantasy.png" alt="" />
                  <img src="/merope/clothing/japanese.png" alt="" />
                  <img src="/merope/clothing/sci-fi.png" alt="" />
                </span>
              )
            }
            actions={
              hasSavedPersona
                ? [
                    {
                      key: 'face',
                      label: t.merope.faceOpen,
                      onClick: () => openAiSubpage('merope'),
                    },
                    {
                      key: 'delete',
                      label: t.config.agentPersonaDelete,
                      onClick: () => void handleDeletePersona(),
                      disabled: personaBusy,
                      loading: personaBusy,
                      variant: 'danger' as const,
                      confirm: t.config.agentPersonaDeleteConfirm,
                    },
                  ]
                : [
                    {
                      key: 'setup',
                      label: o.openPage,
                      onClick: () => openAiSubpage('merope-setup'),
                      disabled: personaBusy,
                    },
                  ]
            }
          >
            {personaCardCopy ? (
              <>
                {personaCardCopy.summary ? (
                  <p className="info-action-card-lede">{personaCardCopy.summary}</p>
                ) : null}
                <p className="info-action-card-meta">
                  <span>{personaCardCopy.mood}</span>
                  <span className="info-action-card-meta-dot" aria-hidden>
                    ·
                  </span>
                  <span>{personaCardCopy.activity}</span>
                </p>
              </>
            ) : (
              <p className="info-action-card-lede">
                {t.config.agentPersonaEmptyLead}
              </p>
            )}
          </InfoActionCard>
          </div>
        ) : null}
        <AgentNestedSection
          title={t.config.agentPersonaSpeech}
          description={t.config.agentPersonaSpeechHint}
          {...bindGuide('agent.agentPersonaSpeech', g.agent.agentPersonaSpeech)}
          tourAnchor="config-ai-persona-speech"
          toggle={{
            checked: agentPersonaSpeechEnabled,
            onChange: (value) =>
              updateUiFieldValue(
                'merope_speech_enabled',
                value ? 'true' : 'false',
              ),
            disabled: !meropeOn,
            ariaLabel: t.config.agentPersonaSpeech,
            title: t.config.agentPersonaSpeechHint,
          }}
        />
      </SettingGroup>
      <SettingGroup
        title={t.config.agentChannelsTitle}
        icon={<LuMessageSquare />}
        description={t.config.agentChannelsDesc}
        {...bindGuide('agent.channels', g.agent.channels)}
      >
        <AgentChannelSources
          visible={visibleChannels}
          justAdded={addedChannel}
          getFieldValue={getFieldValue}
          updateValue={updateValue}
          onRemove={removeChannel}
        />
      </SettingGroup>
      <AgentOptionsPanel />
            </>
          )}
        </div>
      </AutoHeight>
    </SettingSection>
  )
}

export default AgentConfigSection

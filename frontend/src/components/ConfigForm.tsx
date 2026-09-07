/**
 * 设置页壳层：导航 / 搜索 / 统一保存 / Section 渲染。
 * 状态与写路径拆到 `config/form/*` hooks。
 */
import {
  FaExclamationTriangle,
  FaSearch,
  FaStar,
  FaTimes,
  LuRefreshCw,
} from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import React, { useCallback, useEffect, useRef } from 'react'
import { useAuth } from '../contexts/AuthContext'
import { useI18n } from '../contexts/I18nContext'
import { testSpeechService } from '../lib/api'
import { userFacingError } from '../utils/userFacingError'
import {
  AboutConfigSection,
  AdvancedConfigSection,
  AiConfigSection,
  ConfigTipsBanner,
  FederationConfigSection,
  ModuleConfigSection,
  NotificationConfigSection,
  OAuthConfigSection,
  PermissionsConfigSection,
  PlatformsConfigSection,
  TripoConfigSection,
  UiConfigSection,
  UsersConfigSection,
} from './config'
import {
  ConfigNavItem,
  DEFAULT_AUTO_FETCH_CONFIG,
  useConfigBagState,
  useConfigDirty,
  useConfigMessage,
  useConfigNavigation,
  useConfigReset,
  useConfigSave,
  useConfigSearch,
  useConfigSideDrafts,
} from './config/form'
import MyriadConfigIcon from './config/MyriadConfigIcon'
import {
  SectionSwitch,
  SETTINGS_PAGE_MOTION,
  SETTINGS_SIDEBAR_MOTION,
  SettingsButton,
  SettingsPageActionsProvider,
} from './settings'
import { Spinner } from './Spinner'
import Toast from './Toast'
import './ConfigForm.css'

const ModernConfigForm: React.FC = () => {
  const { t, locale } = useI18n()
  const { user, isAdmin } = useAuth()
  const speechTestAudioRef = useRef<{
    audio: HTMLAudioElement
    url: string
  } | null>(null)

  const { message, messageType, showMessage } = useConfigMessage()

  const bag = useConfigBagState(showMessage, t.config.loadConfigFailed)
  const {
    config,
    setConfig,
    initialConfig,
    setInitialConfig,
    loading,
    saving,
    setSaving,
    notifyDirtyState,
    loadConfig,
    updateFieldValue,
    updateAiFieldValue,
    updateTripoFieldValue,
    updateUiFieldValue,
    togglePlatform,
    updateAutoFetchConfig,
    reorderPlatform,
  } = bag

  const drafts = useConfigSideDrafts(isAdmin, showMessage, t)
  const nav = useConfigNavigation(isAdmin, t)
  const {
    activeSection,
    setActiveSection,
    sectionDir,
    mobilePane,
    setMobilePane,
    isMobileLayout,
    platformFocus,
    setPlatformFocus,
    favorites,
    savedFavorites,
    setSavedFavorites,
    quickAccessItems,
    handleSectionChange: navSectionChange,
    scrollSettingsToTop,
    handleMobileBackToNav,
    toggleFavorite,
    getSectionProps,
  } = nav

  const { searchQuery, setSearchQuery, filteredContent } = useConfigSearch(
    config,
    t,
    locale,
    isAdmin,
  )

  const handleSectionChange = useCallback(
    (section: string, options?: { guidePath?: string | null }) => {
      setSearchQuery('')
      navSectionChange(section, options)
    },
    [navSectionChange, setSearchQuery],
  )

  const { isConfigDirty, isLibrarySourceDirty } = useConfigDirty({
    config,
    initialConfig,
    librarySourceDraft: drafts.librarySourceDraft,
    savedLibrarySourcePreferences: drafts.savedLibrarySourcePreferences,
    moduleVisibilityDraft: drafts.moduleVisibilityDraft,
    savedModuleVisibilityPreferences: drafts.savedModuleVisibilityPreferences,
    hitokotoDraft: drafts.hitokotoDraft,
    savedHitokotoConfig: drafts.savedHitokotoConfig,
    reportSettingsDraft: drafts.reportSettingsDraft,
    savedReportSettings: drafts.savedReportSettings,
    permissionConfig: drafts.permissionConfig,
    savedPermissionConfig: drafts.savedPermissionConfig,
    notificationDraft: drafts.notificationDraft,
    savedNotificationPreferences: drafts.savedNotificationPreferences,
    oauthDraft: drafts.oauthDraft,
    savedOAuthSettings: drafts.savedOAuthSettings,
    federationPolicyDraft: drafts.federationPolicyDraft,
    savedFederationPolicy: drafts.savedFederationPolicy,
    isAdmin,
    favorites,
    savedFavorites,
    notifyDirtyState,
  })

  const handleSave = useConfigSave({
    config,
    initialConfig,
    setInitialConfig,
    setSaving,
    notifyDirtyState,
    showMessage,
    t,
    isAdmin,
    userId: user?.id,
    librarySourceDraft: drafts.librarySourceDraft,
    savedLibrarySourcePreferences: drafts.savedLibrarySourcePreferences,
    setLibrarySourceDraft: drafts.setLibrarySourceDraft,
    setSavedLibrarySourcePreferences: drafts.setSavedLibrarySourcePreferences,
    setLibrarySourceSaveRevision: drafts.setLibrarySourceSaveRevision,
    moduleVisibilityDraft: drafts.moduleVisibilityDraft,
    savedModuleVisibilityPreferences: drafts.savedModuleVisibilityPreferences,
    setModuleVisibilityDraft: drafts.setModuleVisibilityDraft,
    setSavedModuleVisibilityPreferences:
      drafts.setSavedModuleVisibilityPreferences,
    hitokotoDraft: drafts.hitokotoDraft,
    savedHitokotoConfig: drafts.savedHitokotoConfig,
    setHitokotoDraft: drafts.setHitokotoDraft,
    setSavedHitokotoConfig: drafts.setSavedHitokotoConfig,
    reportSettingsDraft: drafts.reportSettingsDraft,
    savedReportSettings: drafts.savedReportSettings,
    setReportSettingsDraft: drafts.setReportSettingsDraft,
    setSavedReportSettings: drafts.setSavedReportSettings,
    permissionConfig: drafts.permissionConfig,
    savedPermissionConfig: drafts.savedPermissionConfig,
    setSavedPermissionConfig: drafts.setSavedPermissionConfig,
    notificationDraft: drafts.notificationDraft,
    savedNotificationPreferences: drafts.savedNotificationPreferences,
    setNotificationDraft: drafts.setNotificationDraft,
    setSavedNotificationPreferences: drafts.setSavedNotificationPreferences,
    oauthDraft: drafts.oauthDraft,
    savedOAuthSettings: drafts.savedOAuthSettings,
    setOAuthDraft: drafts.setOAuthDraft,
    setSavedOAuthSettings: drafts.setSavedOAuthSettings,
    federationPolicyDraft: drafts.federationPolicyDraft,
    savedFederationPolicy: drafts.savedFederationPolicy,
    setSavedFederationPolicy: drafts.setSavedFederationPolicy,
    favorites,
    savedFavorites,
    setSavedFavorites,
  })

  const { handleReset, handleResetCurrentPage } = useConfigReset({
    config,
    setConfig,
    setInitialConfig,
    setSaving,
    setPlatformFocus,
    notifyDirtyState,
    showMessage,
    t,
    activeSection,
    isAdmin,
    userId: user?.id,
    oauthDraft: drafts.oauthDraft,
    side: {
      setLibrarySourceDraft: drafts.setLibrarySourceDraft,
      setSavedLibrarySourcePreferences: drafts.setSavedLibrarySourcePreferences,
      setLibrarySourceSaveRevision: drafts.setLibrarySourceSaveRevision,
      setModuleVisibilityDraft: drafts.setModuleVisibilityDraft,
      setSavedModuleVisibilityPreferences:
        drafts.setSavedModuleVisibilityPreferences,
      setHitokotoDraft: drafts.setHitokotoDraft,
      setSavedHitokotoConfig: drafts.setSavedHitokotoConfig,
      setReportSettingsDraft: drafts.setReportSettingsDraft,
      setSavedReportSettings: drafts.setSavedReportSettings,
      setOAuthDraft: drafts.setOAuthDraft,
      setSavedOAuthSettings: drafts.setSavedOAuthSettings,
      setFederationPolicyDraft: drafts.setFederationPolicyDraft,
      setSavedFederationPolicy: drafts.setSavedFederationPolicy,
      setPermissionConfig: drafts.setPermissionConfig,
      setSavedPermissionConfig: drafts.setSavedPermissionConfig,
      setNotificationDraft: drafts.setNotificationDraft,
      setSavedNotificationPreferences: drafts.setSavedNotificationPreferences,
    },
  })

  useEffect(() => {
    void loadConfig()
    void drafts.loadPermissionConfig()
    void drafts.loadModuleVisibilityPreferences()
    void drafts.loadHitokotoSettings()
    void drafts.loadReportSettings()
    void drafts.loadNotificationSettings()
    void drafts.loadOAuthSettings()
    void drafts.loadFederationPolicy()
    // 初始加载一次；各 load* 内部稳定
  }, [])

  // Discord 一键授权回调（section 深链由 useConfigNavigation 统一处理）
  useEffect(() => {
    if (typeof window === 'undefined') return
    const params = new URLSearchParams(window.location.search)
    const oauth = params.get('discord_oauth')
    const platformQ = params.get('platform')
    if (!oauth && platformQ !== 'discord') return

    setMobilePane('section')
    setActiveSection('platforms')
    if (platformQ === 'discord' || oauth) {
      setPlatformFocus('Discord')
    }

    if (oauth === 'ok') {
      showMessage(t.config.discordOAuthSuccess, 'success')
      void loadConfig()
    } else if (oauth === 'error' || (oauth && oauth !== 'ok')) {
      const reason = params.get('reason') || 'unknown'
      showMessage(
        `${t.config.discordOAuthFailed}${reason !== 'unknown' ? ` (${reason})` : ''}`,
        'error',
      )
    }

    params.delete('discord_oauth')
    params.delete('reason')
    params.delete('platform')
    const qs = params.toString()
    const next = `${window.location.pathname}${qs ? `?${qs}` : ''}${window.location.hash}`
    window.history.replaceState({}, '', next)
  }, [
    loadConfig,
    setActiveSection,
    setMobilePane,
    setPlatformFocus,
    showMessage,
    t.config.discordOAuthFailed,
    t.config.discordOAuthSuccess,
  ])

  useEffect(() => {
    const handleSaveEvent = () => void handleSave()
    const handleResetEvent = () => void handleReset()
    window.addEventListener('request-config-save', handleSaveEvent)
    window.addEventListener('config-reset', handleResetEvent)
    return () => {
      window.removeEventListener('request-config-save', handleSaveEvent)
      window.removeEventListener('config-reset', handleResetEvent)
    }
  }, [handleSave, handleReset])

  const releaseSpeechTestAudio = useCallback(() => {
    const current = speechTestAudioRef.current
    if (!current) return
    speechTestAudioRef.current = null
    current.audio.pause()
    current.audio.removeAttribute('src')
    current.audio.load()
    URL.revokeObjectURL(current.url)
  }, [])

  useEffect(() => () => releaseSpeechTestAudio(), [releaseSpeechTestAudio])

  const handleSpeechTest = useCallback(async (): Promise<{
    success: boolean
    message: string
  }> => {
    if (!config) {
      return { success: false, message: t.config.speechTestFailed }
    }
    try {
      const result = await testSpeechService()
      if (result.audio) {
        try {
          const binary = atob(result.audio)
          const bytes = new Uint8Array(binary.length)
          for (let i = 0; i < binary.length; i += 1) {
            bytes[i] = binary.charCodeAt(i)
          }
          const blob = new Blob([bytes], {
            type:
              bytes.length >= 12 &&
              bytes[0] === 0x52 &&
              bytes[1] === 0x49 &&
              bytes[2] === 0x46 &&
              bytes[3] === 0x46
                ? 'audio/wav'
                : 'audio/mpeg',
          })
          releaseSpeechTestAudio()
          const url = URL.createObjectURL(blob)
          const audio = new Audio(url)
          speechTestAudioRef.current = { audio, url }
          const release = () => {
            if (speechTestAudioRef.current?.url !== url) return
            releaseSpeechTestAudio()
          }
          audio.addEventListener('ended', release, { once: true })
          void audio.play().catch(release)
        } catch {
          // Playback is best-effort; the API result still stands.
        }
      }
      if (result.success) {
        if (result.tts_skipped) {
          return {
            success: true,
            message: userFacingError(
              result.error,
              t.config.speechOpenRouterTtsHint,
            ),
          }
        }
        return { success: true, message: t.config.speechTestSuccess }
      }
      return {
        success: false,
        message: userFacingError(result.error, t.config.speechTestFailed),
      }
    } catch {
      return { success: false, message: t.config.speechTestFailed }
    }
  }, [config, releaseSpeechTestAudio, t])

  const handleModuleMessage = useCallback(
    (msg: string, type: 'success' | 'error' | 'info' = 'info') =>
      showMessage(msg, type),
    [showMessage],
  )

  const renderActiveSection = (section: string) => {
    if (!config) return null
    const props = getSectionProps(section)

    switch (section) {
      case 'platforms': {
        const analyticsField = config.ui_config.config_fields.find(
          (f) => f.key === 'analytics_enabled',
        )
        const analyticsEnabled =
          !analyticsField || analyticsField.value !== 'false'
        return (
          <PlatformsConfigSection
            platforms={config.platforms}
            autoFetch={config.auto_fetch || DEFAULT_AUTO_FETCH_CONFIG}
            onUpdateField={updateFieldValue}
            onToggle={togglePlatform}
            onReorder={reorderPlatform}
            onAutoFetchChange={updateAutoFetchConfig}
            showMessage={showMessage}
            openOAuthSection={() => handleSectionChange('oauth')}
            focusPlatform={platformFocus}
            onFocusPlatformConsumed={() => setPlatformFocus(null)}
            analyticsEnabled={analyticsEnabled}
            onAnalyticsEnabledChange={(enabled) =>
              updateUiFieldValue(
                'analytics_enabled',
                enabled ? 'true' : 'false',
              )
            }
            getUiFieldValue={(key) => {
              const field = config.ui_config.config_fields.find(
                (f) => f.key === key,
              )
              return field?.value ?? ''
            }}
            onUiFieldChange={updateUiFieldValue}
            {...props}
          />
        )
      }
      case 'ai':
        return (
          <AiConfigSection
            configFields={config.ai_config.config_fields}
            updateValue={updateAiFieldValue}
            uiConfigFields={config.ui_config.config_fields}
            updateUiFieldValue={updateUiFieldValue}
            onSpeechTest={handleSpeechTest}
            {...props}
          />
        )
      case 'tripo':
        return (
          <TripoConfigSection
            configFields={config.tripo_config.config_fields}
            updateValue={updateTripoFieldValue}
            {...props}
          />
        )
      case 'basic':
        return (
          <UiConfigSection
            configFields={config.ui_config.config_fields}
            updateValue={updateUiFieldValue}
            {...props}
          />
        )
      case 'oauth':
        return (
          <OAuthConfigSection
            configFields={config.ui_config.config_fields}
            providers={drafts.oauthDraft.providers}
            loading={drafts.oauthLoading}
            onProvidersChange={(providers) =>
              drafts.setOAuthDraft((current) => ({ ...current, providers }))
            }
            {...props}
          />
        )
      case 'federation':
        if (!isAdmin) return null
        return (
          <FederationConfigSection
            policyDraft={drafts.federationPolicyDraft}
            onPolicyChange={drafts.updateFederationPolicy}
            onMessage={(msg, type = 'info') => showMessage(msg, type)}
            {...props}
          />
        )
      case 'permissions':
        return (
          <PermissionsConfigSection
            permissionConfig={drafts.permissionConfig}
            updatePermissionConfig={drafts.updatePermissionConfig}
            loading={drafts.permissionLoading}
            {...props}
          />
        )
      case 'modules':
        return (
          <ModuleConfigSection
            sourceDraft={drafts.librarySourceDraft}
            setSourceDraft={drafts.setLibrarySourceDraft}
            visibilityDraft={drafts.moduleVisibilityDraft}
            setVisibilityDraft={drafts.setModuleVisibilityDraft}
            isSourceDirty={isLibrarySourceDirty}
            saveRevision={drafts.librarySourceSaveRevision}
            onSourcePreferencesLoaded={
              drafts.handleLibrarySourcePreferencesLoaded
            }
            hitokotoDraft={drafts.hitokotoDraft}
            setHitokotoDraft={drafts.setHitokotoDraft}
            reportSettingsDraft={drafts.reportSettingsDraft}
            setReportSettingsDraft={drafts.setReportSettingsDraft}
            uiConfigFields={config.ui_config.config_fields}
            updateUiFieldValue={updateUiFieldValue}
            onMessage={handleModuleMessage}
            {...props}
          />
        )
      case 'notifications':
        return (
          <NotificationConfigSection
            preferences={drafts.notificationDraft}
            sources={drafts.notificationSources}
            events={drafts.notificationEvents}
            loading={drafts.notificationLoading}
            onChange={drafts.setNotificationDraft}
            {...props}
          />
        )
      case 'users':
        return (
          <UsersConfigSection
            onMessage={(msg, type = 'info') => showMessage(msg, type)}
            allowRegister={drafts.oauthDraft.allowLocalRegistration}
            allowRegisterLoading={drafts.oauthLoading}
            onAllowRegisterChange={(allowLocalRegistration) =>
              drafts.setOAuthDraft((current) => ({
                ...current,
                allowLocalRegistration,
              }))
            }
            privateTappInstallPreset={(() => {
              const d = drafts.oauthDraft
              if (d.privateTappInstallCleanup === 'logout') return 'logout'
              // Keep exact day count (including non-preset values like 30)
              return String(d.privateTappInstallInactivityDays)
            })()}
            privateTappInstallLoading={drafts.oauthLoading}
            onPrivateTappInstallPresetChange={(preset) =>
              drafts.setOAuthDraft((current) => {
                if (preset === 'logout') {
                  return {
                    ...current,
                    privateTappInstallCleanup: 'logout',
                    privateTappInstallInactivityDays: 14,
                  }
                }
                const days = Number(preset)
                return {
                  ...current,
                  privateTappInstallCleanup: 'inactivity',
                  privateTappInstallInactivityDays:
                    Number.isFinite(days) && days >= 1 ? Math.min(365, days) : 14,
                }
              })
            }
            {...props}
          />
        )
      case 'advanced':
        return (
          <AdvancedConfigSection
            onReset={handleReset}
            uiConfigFields={config.ui_config.config_fields}
            updateUiFieldValue={updateUiFieldValue}
            onMessage={(msg, type = 'info') => showMessage(msg, type)}
            {...props}
          />
        )
      case 'about':
        return <AboutConfigSection {...props} />
      default:
        return null
    }
  }

  if (loading) {
    return (
      <div className="modern-config-loading" role="status" aria-live="polite">
        <Spinner size="lg" color="primary" />
      </div>
    )
  }

  if (!config) {
    return (
      <div
        className="modern-config-error"
        role="alert"
        aria-live="assertive"
        aria-labelledby="config-load-error-title"
      >
        <div className="modern-config-error-card">
          <div className="modern-config-error-visual" aria-hidden="true">
            <span className="modern-config-error-icon">
              <FaExclamationTriangle />
            </span>
          </div>
          <div className="modern-config-error-copy">
            <h2 id="config-load-error-title">{t.config.loadConfigFailed}</h2>
            <p>{t.config.loadConfigFailedDesc}</p>
          </div>
          <div className="modern-config-error-actions">
            <SettingsButton
              variant="primary"
              size="md"
              icon={<LuRefreshCw />}
              onClick={() => void loadConfig()}
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
      {message && <Toast message={message} type={messageType} />}

      <div
        className="config-shell"
        data-mobile-pane={isMobileLayout ? mobilePane : 'desktop'}
      >
        {!(isMobileLayout && mobilePane === 'section') ? (
          <motion.aside
            className="config-sidebar"
            aria-label={t.config.title}
            data-tour="config-sidebar"
            initial={SETTINGS_SIDEBAR_MOTION.initial}
            animate={SETTINGS_SIDEBAR_MOTION.animate}
            transition={SETTINGS_SIDEBAR_MOTION.transition}
          >
            <div className="config-sidebar-header">
              <span className="nav-icon">
                <MyriadConfigIcon kind="basic" />
              </span>
              <div className="config-sidebar-heading">
                <h3 className="nav-title">{t.config.title}</h3>
                <p className="nav-subtitle">{t.config.selectProject}</p>
              </div>
            </div>

            <div className="config-sidebar-search">
              <div className="search-input-wrapper" data-tour="config-search">
                <FaSearch className="search-icon" />
                <input
                  type="search"
                  placeholder={t.config.searchConfig}
                  value={searchQuery}
                  onChange={(e) => setSearchQuery(e.target.value)}
                  className="search-input"
                />
                {searchQuery && (
                  <button
                    onClick={() => setSearchQuery('')}
                    className="search-clear"
                    aria-label="Clear search"
                  >
                    <FaTimes />
                  </button>
                )}
              </div>
            </div>

            {searchQuery ? (
              <div className="config-sidebar-results">
                <div className="config-nav-group sm-stagger">
                  <div className="config-nav-group-title">
                    {t.config.searchResults} ({filteredContent.length})
                  </div>
                  {filteredContent.length > 0 ? (
                    filteredContent.map((item, index) => (
                      <button
                        key={`${item.type}-${item.section}-${item.guidePath ?? item.title}-${index}`}
                        type="button"
                        onClick={() => {
                          handleSectionChange(item.section, {
                            guidePath:
                              item.type === 'guide' ? item.guidePath : null,
                          })
                        }}
                        className={`config-nav-result${item.type === 'guide' ? ' is-guide' : ''}`}
                      >
                        <span className="config-nav-result-text">
                          <span className="config-nav-result-title">
                            {item.type === 'guide' ? (
                              <span className="config-nav-result-badge">
                                {t.config.searchGuideBadge}
                              </span>
                            ) : null}
                            {item.title}
                          </span>
                          <span className="config-nav-result-desc">
                            {item.matchSnippet && item.type === 'guide'
                              ? item.matchSnippet
                              : item.description}
                          </span>
                        </span>
                        <span className="config-nav-result-arrow" aria-hidden>
                          ›
                        </span>
                      </button>
                    ))
                  ) : (
                    <p className="config-nav-empty">
                      {t.config.noMatchingConfig}
                      <span className="config-nav-empty-hint">
                        {t.config.searchEmptyHint}
                      </span>
                    </p>
                  )}
                </div>
              </div>
            ) : (
              <nav className="config-sidebar-scroll">
                <ConfigTipsBanner />
                {favorites.length > 0 && (
                  <div className="config-nav-group config-nav-group--favorites">
                    <div className="config-nav-group-title">
                      <FaStar className="config-nav-group-icon" />
                      {t.config.favorites}
                    </div>
                    {favorites.map((fav) => {
                      const item = quickAccessItems.find((i) => i.id === fav)
                      return item ? (
                        <ConfigNavItem
                          key={item.id}
                          item={item}
                          isActive={activeSection === item.section}
                          isFavorite={true}
                          group="favorites"
                          onSelect={handleSectionChange}
                          onToggleFavorite={toggleFavorite}
                        />
                      ) : null
                    })}
                  </div>
                )}
                <div className="config-nav-group config-nav-group--all">
                  <div className="config-nav-group-title">
                    {t.config.allConfig}
                  </div>
                  {quickAccessItems.map((item) => (
                    <ConfigNavItem
                      key={item.id}
                      item={item}
                      isActive={activeSection === item.section}
                      isFavorite={favorites.includes(item.id)}
                      group="all"
                      onSelect={handleSectionChange}
                      onToggleFavorite={toggleFavorite}
                    />
                  ))}
                </div>
              </nav>
            )}
          </motion.aside>
        ) : null}

        {!(isMobileLayout && mobilePane === 'nav') ? (
          <div className="config-content" data-tour="config-content">
            <SettingsPageActionsProvider
              value={{
                resetCurrentPage: handleResetCurrentPage,
                canResetCurrentPage: ![
                  'about',
                  'updater',
                  'platforms',
                  'oauth',
                  'users',
                ].includes(activeSection),
                // 移动端选项页：标题栏内嵌返回（与平台二级页 section-header-back 同款）
                // 平台二级页通过显式 headerLeading 覆盖，先回列表再回菜单
                onMobileBack: isMobileLayout
                  ? handleMobileBackToNav
                  : undefined,
              }}
            >
              <SectionSwitch
                sectionKey={activeSection}
                direction={sectionDir}
                onCommit={scrollSettingsToTop}
              >
                {(section) => renderActiveSection(section)}
              </SectionSwitch>
            </SettingsPageActionsProvider>
          </div>
        ) : null}
      </div>

      {isConfigDirty && (
        <div className="floating-save-container">
          <SettingsButton
            variant="primary"
            className="floating-save-btn"
            onClick={() => void handleSave()}
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

export default ModernConfigForm

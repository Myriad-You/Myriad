import {
  FaExclamationTriangle,
  FaSearch,
  FaStar,
  FaTimes,
  LuRefreshCw,
} from '@lib/icons'
import { motionShim as motion } from '@lib/motionShim'
import React, { useCallback, useEffect } from 'react'
import { useAuth } from '../contexts/AuthContext'
import { useConfigI18n as useI18n } from '../contexts/I18nContext'
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
import { ConfigDefaultsProvider } from './config/ConfigDefaultsProvider'
import {
  ConfigNavItem,
  DEFAULT_AUTO_FETCH_CONFIG,
  useConfigBagState,
  useConfigDomains,
  useConfigEditor,
  useConfigMessage,
  useConfigNavigation,
  useConfigSearch,
  useConfigSessionKey,
  useFederationEnabled,
} from './config/form'
import MyriadConfigIcon from './config/MyriadConfigIcon'
import { useSpeechTest } from './config/useSpeechTest'
import {
  SectionSwitch,
  SETTINGS_PAGE_MOTION,
  SETTINGS_SIDEBAR_MOTION,
  SettingsButton,
  SettingSection,
  SettingsPageActionsProvider,
} from './settings'
import { Spinner } from './Spinner'
import './ConfigForm.css'

const ModernConfigForm: React.FC = () => {
  const { t, locale } = useI18n()
  const { user, isAdmin } = useAuth()

  const { showMessage } = useConfigMessage()

  const bag = useConfigBagState(t.config)
  const {
    config,
    updateFieldValue,
    updateAiFieldValue,
    updateTripoFieldValue,
    updateUiFieldValue,
    togglePlatform,
    updateAutoFetchConfig,
    reorderPlatform,
  } = bag

  const federationEnabled = useFederationEnabled()
  const drafts = useConfigDomains(
    isAdmin,
    t.config,
    user?.id,
    federationEnabled,
  )
  const nav = useConfigNavigation(isAdmin, t, federationEnabled)
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
    favoritesDomain,
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
    federationEnabled,
  )

  const handleSectionChange = useCallback(
    (section: string, options?: { guidePath?: string | null }) => {
      setSearchQuery('')
      navSectionChange(section, options)
    },
    [navSectionChange, setSearchQuery],
  )

  const editor = useConfigEditor(
    [bag, ...Object.values(drafts), favoritesDomain],
    showMessage,
    t.config,
    activeSection,
  )
  const {
    isDirty: isConfigDirty,
    saving,
    save: handleSave,
    load: loadConfig,
  } = editor
  const handleReset = useCallback(() => editor.reset(), [editor.reset])
  const handleResetCurrentPage = useCallback(
    () => editor.reset(activeSection),
    [editor.reset, activeSection],
  )

  const contentReady =
    bag.ready &&
    !bag.loading &&
    !bag.error &&
    Object.values(drafts).every(
      (domain) =>
        !domain.sections.includes(activeSection) ||
        (domain.ready && !domain.loading && !domain.error),
    )
  useEffect(() => {
    if (contentReady) window.dispatchEvent(new CustomEvent('config-loaded'))
  }, [contentReady, activeSection])

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
    } else if (oauth === 'error' || (oauth && oauth !== 'ok')) {
      const reason = params.get('reason') || 'unknown'
      showMessage(
        reason === 'app_not_configured'
          ? t.config.discordOAuthAppMissing
          : t.config.discordOAuthFailed,
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
    setActiveSection,
    setMobilePane,
    setPlatformFocus,
    showMessage,
    t.config.discordOAuthAppMissing,
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

  const handleSpeechTest = useSpeechTest()

  const handleModuleMessage = useCallback(
    (msg: string, type: 'success' | 'error' | 'info' = 'info') =>
      showMessage(msg, type),
    [showMessage],
  )

  const renderActiveSection = (section: string) => {
    if (!config) return null
    const props = getSectionProps(section)

    const unavailable = Object.values(drafts).filter(
      (domain) =>
        domain.sections.includes(section) &&
        (!domain.ready || domain.loading || domain.error),
    )
    if (unavailable.length) {
      const failed = unavailable.some((domain) => domain.error)
      return (
        <SettingSection {...props} showResetPage={false}>
          {failed ? (
            <div role="alert">
              <p>{t.config.loadConfigFailed}</p>
              <SettingsButton
                onClick={() => void loadConfig(section)}
                disabled={unavailable.some((domain) => domain.loading)}
              >
                {t.common.retry}
              </SettingsButton>
            </div>
          ) : (
            <div role="status" aria-label={t.common.loading}>
              <Spinner size="lg" color="primary" />
            </div>
          )}
        </SettingSection>
      )
    }

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
            onSpeechTest={handleSpeechTest}
            {...props}
          />
        )
      case 'lab':
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
            providers={drafts.oauth.draft.providers}
            loading={drafts.oauth.loading}
            onProvidersChange={(providers) =>
              drafts.oauth.setDraft((current) => ({ ...current, providers }))
            }
            {...props}
          />
        )
      case 'federation':
        if (!isAdmin || !federationEnabled) return null
        return (
          <FederationConfigSection
            policyDraft={drafts.federation.draft}
            onPolicyChange={(patch) =>
              drafts.federation.setDraft((current) => ({
                ...current,
                ...patch,
              }))
            }
            onMessage={(msg, type = 'info') => showMessage(msg, type)}
            {...props}
          />
        )
      case 'permissions':
        return (
          <PermissionsConfigSection
            permissionConfig={drafts.permissions.draft}
            updatePermissionConfig={(keyOrPatch, value) =>
              drafts.permissions.setDraft((current) => ({
                ...current,
                ...(typeof keyOrPatch === 'string'
                  ? { [keyOrPatch]: value as boolean | number }
                  : keyOrPatch),
              }))
            }
            loading={drafts.permissions.loading}
            {...props}
          />
        )
      case 'modules':
        return (
          <ModuleConfigSection
            sourceDraft={drafts.library.draft}
            setSourceDraft={drafts.library.setDraft}
            visibilityDraft={drafts.visibility.draft}
            setVisibilityDraft={drafts.visibility.setDraft}
            savedPreferences={drafts.library.saved}
            hitokotoDraft={drafts.hitokoto.draft}
            setHitokotoDraft={drafts.hitokoto.setDraft}
            reportSettingsDraft={drafts.reports.draft}
            setReportSettingsDraft={drafts.reports.setDraft}
            uiConfigFields={config.ui_config.config_fields}
            updateUiFieldValue={updateUiFieldValue}
            onMessage={handleModuleMessage}
            {...props}
          />
        )
      case 'notifications':
        return (
          <NotificationConfigSection
            preferences={drafts.notifications.draft}
            sources={drafts.notifications.catalog.sources}
            events={drafts.notifications.catalog.events}
            loading={drafts.notifications.loading}
            onChange={drafts.notifications.setDraft}
            {...props}
          />
        )
      case 'users':
        return (
          <UsersConfigSection
            onMessage={(msg, type = 'info') => showMessage(msg, type)}
            allowRegister={drafts.oauth.draft.allowLocalRegistration}
            allowRegisterLoading={drafts.oauth.loading}
            onAllowRegisterChange={(allowLocalRegistration) =>
              drafts.oauth.setDraft((current) => ({
                ...current,
                allowLocalRegistration,
              }))
            }
            privateTappInstallPreset={(() => {
              const d = drafts.oauth.draft
              if (d.privateTappInstallCleanup === 'logout') return 'logout'
              // 保留精确天数（含非预设如 30）。
              return String(d.privateTappInstallInactivityDays)
            })()}
            privateTappInstallLoading={drafts.oauth.loading}
            onPrivateTappInstallPresetChange={(preset) =>
              drafts.oauth.setDraft((current) => {
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
                    Number.isFinite(days) && days >= 1
                      ? Math.min(365, days)
                      : 14,
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

  if (bag.loading || (!bag.ready && !bag.error)) {
    return (
      <div className="modern-config-loading" role="status" aria-live="polite">
        <Spinner size="lg" color="primary" />
      </div>
    )
  }

  if (!config || bag.error) {
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
                canResetCurrentPage: nav.canResetCurrentPage,
                // 移动端选项页标题栏内嵌返回；平台二级页用 headerLeading 覆盖，先回列表再回菜单。
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

export default function ConfigForm() {
  const sessionKey = useConfigSessionKey()
  return (
    <ConfigDefaultsProvider>
      <ModernConfigForm key={sessionKey} />
    </ConfigDefaultsProvider>
  )
}

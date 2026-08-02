import type React from 'react'
import type {
  NotificationEventDefinition,
  NotificationPreferences,
  NotificationSourceKey,
} from '../../services/notificationPreferencesApi'
import { useMemo } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { getNotificationCopy } from '../../i18n/notificationCatalog'
import { cloneNotificationPreferences } from '../../services/notificationPreferencesApi'
import { NotificationSourceIcon } from '../notifications/NotificationIcons'
import {
  SegmentedControl,
  SettingGroup,
  SettingGroupGrid,
  SettingSection,
  SettingTitleGuideEntry,
  SwitchItem,
  useSettingGuide,
} from '../settings'
import './NotificationConfigSection.css'

type LocationKey = keyof NotificationPreferences['locations'][NotificationSourceKey]

const LOCATION_KEYS: LocationKey[] = [
  'panel',
  'toast',
  'island',
  'browser',
]

interface NotificationConfigSectionProps {
  title: string
  icon?: React.ReactNode
  description?: string
  sectionId?: string
  preferences: NotificationPreferences
  sources: NotificationSourceKey[]
  events: NotificationEventDefinition[]
  loading?: boolean
  onChange: (preferences: NotificationPreferences) => void
}

export const NotificationConfigSection: React.FC<
  NotificationConfigSectionProps
> = ({
  title,
  icon,
  description,
  sectionId,
  preferences,
  sources,
  events,
  loading = false,
  onChange,
}) => {
  const { locale } = useI18n()
  const { catalog: g, renderGuide, bindGuide } = useSettingGuide()
  const { sources: sourceText, events: eventText, ui } =
    getNotificationCopy(locale)
  const locationsGuide = renderGuide(g.notifications.locations)
  const eventsGuide = renderGuide(g.notifications.events)

  const locationLabels = useMemo<Record<LocationKey, string>>(
    () => ({
      panel: ui.panelLocation,
      toast: ui.toastLocation,
      island: ui.islandLocation,
      browser: ui.browserLocation,
    }),
    [ui.browserLocation, ui.islandLocation, ui.panelLocation, ui.toastLocation],
  )

  const eventsBySource = useMemo(
    () =>
      Object.fromEntries(
        sources.map((source) => [
          source,
          events.filter((event) => event.source === source),
        ]),
      ) as Record<NotificationSourceKey, typeof events>,
    [events, sources],
  )

  const update = (mutate: (draft: NotificationPreferences) => void) => {
    const next = cloneNotificationPreferences(preferences)
    mutate(next)
    onChange(next)
  }

  return (
    <SettingSection
      sectionId={sectionId}
      title={title}
      icon={icon}
      description={description}
    >
      <SettingGroup>
        <SwitchItem
          itemKey="notifications-enabled"
          label={ui.master}
          description={ui.masterDesc}
          {...bindGuide('notifications.master', g.notifications.master)}
          value={preferences.enabled}
          loading={loading}
          onChange={(value) => update((draft) => void (draft.enabled = value))}
        />
        <SwitchItem
          itemKey="notifications-island"
          label={ui.island}
          description={ui.islandDesc}
          {...bindGuide('notifications.island', g.notifications.island)}
          value={preferences.delivery.island}
          disabled={!preferences.enabled}
          loading={loading}
          onChange={(value) =>
            update((draft) => void (draft.delivery.island = value))
          }
        />
        <SwitchItem
          itemKey="notifications-toast"
          label={ui.toast}
          description={ui.toastDesc}
          {...bindGuide('notifications.toast', g.notifications.toast)}
          value={preferences.delivery.toast}
          disabled={!preferences.enabled}
          loading={loading}
          onChange={(value) =>
            update((draft) => void (draft.delivery.toast = value))
          }
        />
        <SwitchItem
          itemKey="notifications-browser"
          label={ui.browser}
          description={ui.browserDesc}
          {...bindGuide('notifications.browser', g.notifications.browser)}
          value={preferences.delivery.browser}
          disabled={!preferences.enabled}
          loading={loading}
          onChange={(value) =>
            update((draft) => void (draft.delivery.browser = value))
          }
        />
      </SettingGroup>

      <SettingGroupGrid
        columns={2}
        variant="card"
        align="stretch"
        minColumnWidth="18rem"
        className="notification-source-grid"
        ariaLabel={ui.sources}
      >
        {sources.map((source) => (
          <SettingGroup
            key={source}
            title={sourceText[source].title}
            description={sourceText[source].description}
            {...bindGuide('notifications.source', g.notifications.source)}
            icon={<NotificationSourceIcon source={source} />}
            className="notification-source-group"
            switch={{
              checked: preferences.sources[source],
              disabled: !preferences.enabled || loading,
              loading,
              ariaLabel: ui.sourceEnabled,
              onChange: (value) =>
                update((draft) => void (draft.sources[source] = value)),
            }}
          >
            <div
              data-guide-path="notifications.locations"
              className={`setting-item setting-vertical notification-choice-group has-guide-anchor${
                !preferences.enabled ||
                !preferences.sources[source] ||
                loading
                  ? ' disabled'
                  : ''
              }`}
            >
              <div className="setting-label">
                <span className="setting-label-text">
                  {ui.locations}
                  <SettingTitleGuideEntry
                    title={ui.locations}
                    guide={locationsGuide}
                  />
                </span>
                <span className="setting-description">{ui.locationsDesc}</span>
              </div>
              <SegmentedControl
                mode="multi"
                size="sm"
                ariaLabel={ui.locations}
                disabled={
                  !preferences.enabled ||
                  !preferences.sources[source] ||
                  loading
                }
                value={LOCATION_KEYS.filter(
                  (key) => preferences.locations[source][key],
                )}
                options={LOCATION_KEYS.map((key) => ({
                  value: key,
                  label: locationLabels[key],
                }))}
                onChange={(selected) =>
                  update((draft) => {
                    const selectedSet = new Set(selected)
                    for (const key of LOCATION_KEYS) {
                      draft.locations[source][key] = selectedSet.has(key)
                    }
                  })
                }
              />
            </div>
            <div
              data-guide-path="notifications.events"
              className={`setting-item setting-vertical notification-choice-group has-guide-anchor${
                !preferences.enabled ||
                !preferences.sources[source] ||
                loading
                  ? ' disabled'
                  : ''
              }`}
            >
              <div className="setting-label">
                <span className="setting-label-text">
                  {ui.events}
                  <SettingTitleGuideEntry
                    title={ui.events}
                    guide={eventsGuide}
                  />
                </span>
              </div>
              <SegmentedControl
                mode="multi"
                size="sm"
                ariaLabel={ui.events}
                className="notification-event-choices"
                disabled={
                  !preferences.enabled ||
                  !preferences.sources[source] ||
                  loading
                }
                value={eventsBySource[source]
                  .filter((event) => preferences.events[event.key])
                  .map((event) => event.key)}
                options={eventsBySource[source].map((event) => ({
                  value: event.key,
                  label: eventText[event.key] || event.key,
                }))}
                onChange={(selected) =>
                  update((draft) => {
                    const selectedSet = new Set(selected)
                    for (const event of eventsBySource[source]) {
                      draft.events[event.key] = selectedSet.has(event.key)
                    }
                  })
                }
              />
            </div>
          </SettingGroup>
        ))}
      </SettingGroupGrid>
    </SettingSection>
  )
}

export default NotificationConfigSection

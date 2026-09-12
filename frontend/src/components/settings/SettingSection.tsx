import type { ReactNode } from 'react'
import type { SettingSectionConfig } from './types'

import { LuChevronLeft } from '@lib/icons'
import React, { useMemo, useState } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { guideDomProps } from './guides/guideAnchor'
import { prefersReducedMotion } from './motion'
import { SettingGroup } from './SettingGroup'
import { SettingsHelpProvider } from './SettingsHelpContext'
import { SettingsHelpToggle } from './SettingsHelpToggle'
import { useSettingsPageActions } from './SettingsPageActionsContext'
import { SettingsPageResetButton } from './SettingsPageResetButton'
import {
  SettingsTocProvider,
  useSettingsToc,
} from './SettingsTocContext'
import { SettingTitleGuideEntry } from './SettingTitleGuideEntry'
import { SettingTitleHelp } from './SettingTitleHelp'
import './settings-motion.css'
import './SettingSection.css'

export interface SettingSectionProps extends SettingSectionConfig {
  helpToggle?: boolean
  showResetPage?: boolean
  headerActions?: ReactNode
  /** between reset and help; omit on other pages */
  headerBetweenPinned?: ReactNode
  headerLeading?: ReactNode
}

function SectionTocNav() {
  const { t } = useI18n()
  const toc = useSettingsToc()
  if (!toc || toc.items.length < 2) return null

  const jump = (id: string) => {
    const el = document.getElementById(id)
    if (!el) return
    el.scrollIntoView({
      behavior: prefersReducedMotion() ? 'auto' : 'smooth',
      block: 'start',
    })
  }

  return (
    <nav className="section-toc" aria-label={t.config.sectionTocAria}>
      {toc.items.map((item) => (
        <button
          key={item.id}
          type="button"
          className="section-toc-chip"
          onClick={() => jump(item.id)}
        >
          {item.label}
        </button>
      ))}
    </nav>
  )
}

export const SettingSection: React.FC<SettingSectionProps> = ({
  sectionId,
  title,
  icon,
  titleExtra,
  detail,
  guide,
  guidePath,
  detailTone = 'default',
  description,
  descriptionVisible = false,
  groups,
  children,
  className = '',
  animated = true,
  helpToggle = true,
  showResetPage,
  headerActions,
  headerBetweenPinned,
  headerLeading,
}) => {
  const { t, format } = useI18n()
  const pageActions = useSettingsPageActions()
  const [showDetails, setShowDetails] = useState(false)

  const helpCtx = useMemo(
    () => ({
      showDetails,
      setShowDetails,
    }),
    [showDetails],
  )

  const canReset =
    showResetPage !== false &&
    pageActions?.canResetCurrentPage !== false &&
    typeof pageActions?.resetCurrentPage === 'function'

  const hasBetweenPinned =
    headerBetweenPinned != null && headerBetweenPinned !== false
  const hasPinnedActions = canReset || helpToggle || hasBetweenPinned
  const hasExtraActions = headerActions != null && headerActions !== false

  const iconClassName = sectionId
    ? `section-icon icon-${sectionId}`
    : 'section-icon'

  const helpContent = detail ?? description
  const showHelp = helpContent != null && helpContent !== ''
  const showDescriptionLine =
    (descriptionVisible || showDetails) &&
    helpContent != null &&
    helpContent !== ''

  const resolvedHeaderLeading: ReactNode =
    headerLeading !== undefined && headerLeading !== null
      ? headerLeading === false
        ? null
        : headerLeading
      : pageActions?.onMobileBack
        ? (
            <button
              type="button"
              className="section-header-back"
              onClick={pageActions.onMobileBack}
              aria-label={t.common.back}
            >
              <LuChevronLeft size={18} aria-hidden />
              <span>{t.common.back}</span>
            </button>
          )
        : null

  const renderIcon = () => {
    if (!icon) return null
    if (typeof icon === 'string') {
      return <span className={iconClassName}>{icon}</span>
    }
    return <span className={iconClassName}>{icon}</span>
  }

  const sectionAnchorProps = guideDomProps(guidePath)

  const content = (
    <SettingsHelpProvider value={helpCtx}>
      <SettingsTocProvider>
        <div
          {...sectionAnchorProps}
          className={`config-section setting-section${
            animated ? ' sm-enter' : ''
          }${guidePath ? ' has-guide-anchor' : ''} ${className}`.trim()}
        >
          <div className="section-header">
            <div className="section-header-left">
              {resolvedHeaderLeading != null ? (
                <div className="section-header-leading">
                  {resolvedHeaderLeading}
                </div>
              ) : null}
              {renderIcon()}
              <div className="section-header-text">
                <h2 className="section-title">
                  {title}
                  {showHelp && !showDetails && (
                    <SettingTitleHelp
                      ariaLabel={format(t.config.detailHelpAriaNamed, {
                        title: String(title),
                      })}
                      tone={detailTone}
                    >
                      {helpContent}
                    </SettingTitleHelp>
                  )}
                  <SettingTitleGuideEntry
                    title={typeof title === 'string' ? title : ''}
                    guide={guide}
                  />
                  {titleExtra != null && titleExtra !== false && (
                    <span className="section-title-extra">{titleExtra}</span>
                  )}
                </h2>
                {showDescriptionLine && (
                  <div
                    className={`section-description${
                      detailTone === 'warning' ? ' is-warning' : ''
                    }`}
                  >
                    {helpContent}
                  </div>
                )}
              </div>
            </div>

            <div className="section-header-right">
              <SectionTocNav />
              {hasExtraActions && (
                <div className="section-header-actions-extra">
                  {headerActions}
                </div>
              )}
              {hasExtraActions && hasPinnedActions && (
                <span
                  className="section-header-actions-divider"
                  aria-hidden
                />
              )}
              {hasPinnedActions && (
                <div className="section-header-actions-pinned">
                  {canReset && (
                    <SettingsPageResetButton
                      onReset={() => pageActions!.resetCurrentPage!()}
                    />
                  )}
                  {hasBetweenPinned && (
                    <div className="section-header-actions-between">
                      {headerBetweenPinned}
                    </div>
                  )}
                  {helpToggle && (
                    <SettingsHelpToggle
                      checked={showDetails}
                      onChange={setShowDetails}
                    />
                  )}
                </div>
              )}
            </div>
          </div>

          <div className="config-form">
            {groups?.map((group, index) => (
              <SettingGroup key={group.title || `group-${index}`} {...group} />
            ))}
            {children}
          </div>
        </div>
      </SettingsTocProvider>
    </SettingsHelpProvider>
  )

  return content
}

SettingSection.displayName = 'SettingSection'

export default SettingSection

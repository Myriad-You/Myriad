/**
 * 设置区块组件
 * 带标题、图标；详细说明默认 ⓘ tooltip；
 * 右上角：本页子分类快速跳转 + 页级特殊操作 + 重置本页 + 显示说明。
 *
 * 帮助三层（勿混用）：
 * - description / detail：短说明。默认 ⓘ tooltip；「显示说明」开启后标题下常显。
 * - guide：长指南。「显示说明」开启后标题旁出现入口，点击以浮窗展示（优先上方，不够则左侧）。
 */

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
  /** 是否显示右上角「显示说明」开关，默认 true */
  helpToggle?: boolean
  /** 是否显示「重置本页」；默认跟随页面 actions context */
  showResetPage?: boolean
  /** 本页特殊右上角操作，渲染在重置 / 显示说明之前（分隔线左侧）。 */
  headerActions?: ReactNode
  /**
   * 常驻区插槽：夹在「重置本页」与「显示说明」之间。
   * 仅个别页需要（如 AI 添加服务商、第三方登录添加方式）；其它页勿传。
   */
  headerBetweenPinned?: ReactNode
  /**
   * 标题栏左侧前缀（如二级页返回），在区块图标之前。
   * 传入节点时覆盖移动端默认「回菜单」返回；传 false 可强制隐藏。
   */
  headerLeading?: ReactNode
}

/** 标题栏右侧：本页顶层 SettingGroup 快速跳转（≥2 才显示） */
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
  const { t } = useI18n()
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

  // 显式 headerLeading 优先（平台二级页 → 列表）；否则移动端回菜单
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

  /* 区块进入交给动效系统的 .sm-enter（与其它设置页动效同一套令牌） */
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
                      ariaLabel={t.config.detailHelpAriaNamed.replace(
                        '{title}',
                        String(title),
                      )}
                      tone={detailTone}
                    >
                      {helpContent}
                    </SettingTitleHelp>
                  )}
                  {/* 点入口展开；面板经 contents 顶到标题上方左侧 */}
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
              {/* 快速跳转：标题栏右侧、操作按钮之前（≥2 子分类才渲染） */}
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

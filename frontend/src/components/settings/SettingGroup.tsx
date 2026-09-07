/**
 * 设置分组组件
 * 用于将相关设置项组织在一起
 *
 * 位于 SettingGroupGrid 内时：
 * - stretch（默认）：卡片 height:100% 与同排同高
 * - rows：额外展开为 subgrid 单元，内部区块跨列对齐
 *
 * 帮助分层：
 * - detail / description：默认 ⓘ tooltip；「显示说明」开启后标题下常显
 * - guide：「显示说明」开启后标题旁入口，点击以浮窗展示（优先上方，不够则左侧）
 */

import type { SettingGroupConfig } from './types'
import React, { useEffect, useMemo } from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { CollapseRegion } from './CollapseRegion'
import { GUIDE_PATH_ATTR, guideAnchorId } from './guides/guideAnchor'
import { ToggleSwitch } from './items/ToggleSwitch'
import { useSettingGroupGrid } from './SettingGroupGrid'
import { SettingItem } from './SettingItem'
import { useSettingsHelp } from './SettingsHelpContext'
import {
  slugifySettingGroupId,
  useSettingsToc,
} from './SettingsTocContext'
import { SettingTitleGuideEntry } from './SettingTitleGuideEntry'
import { SettingTitleHelp } from './SettingTitleHelp'
import './SettingGroup.css'

export interface SettingGroupProps extends SettingGroupConfig {}

export const SettingGroup: React.FC<SettingGroupProps> = ({
  title,
  id: idProp,
  toc = true,
  titleExtra,
  switch: switchConfig,
  detail,
  guide,
  guidePath,
  detailTone = 'default',
  description,
  descriptionVisible = false,
  icon,
  items,
  children,
  collapsible = false,
  defaultExpanded = true,
  className = '',
}) => {
  const { t } = useI18n()
  const gridCtx = useSettingGroupGrid()
  const helpCtx = useSettingsHelp()
  const tocCtx = useSettingsToc()
  const inGrid = Boolean(gridCtx?.inGrid)
  const detailAria =
    typeof title === 'string' && title
      ? t.config.detailHelpAriaNamed.replace('{title}', title)
      : t.config.detailHelpAria
  /** 折叠与 subgrid 冲突，网格 rows 模式忽略 collapsible */
  const useSubgrid = Boolean(gridCtx?.alignRows) && !collapsible
  const expandHelp = Boolean(helpCtx?.showDetails)

  const titleStr = typeof title === 'string' ? title : ''
  const anchorId = useMemo(() => {
    if (idProp) return idProp
    // TOC 用标题 slug 保证同页多组唯一；搜索跳转靠 data-guide-path
    if (titleStr) return slugifySettingGroupId(titleStr)
    if (guidePath) return guideAnchorId(guidePath)
    return ''
  }, [idProp, guidePath, titleStr])

  /** 顶层有 title 的 Group 自动进页内 TOC（网格内卡片不进） */
  const participateToc =
    toc !== false &&
    !inGrid &&
    Boolean(tocCtx) &&
    Boolean(anchorId) &&
    Boolean(titleStr)

  /* 只依赖稳定的 register/unregister，避免 items 变化时全体重注册 */
  const registerToc = tocCtx?.register
  const unregisterToc = tocCtx?.unregister

  useEffect(() => {
    if (!participateToc || !registerToc || !unregisterToc) return
    registerToc(anchorId, titleStr)
    return () => unregisterToc(anchorId)
  }, [participateToc, registerToc, unregisterToc, anchorId, titleStr])

  const [isExpanded, setIsExpanded] = React.useState(defaultExpanded)

  const handleToggle = React.useCallback(() => {
    if (collapsible) {
      setIsExpanded((prev) => !prev)
    }
  }, [collapsible])

  /** 说明内容：detail 优先，否则 description */
  const helpContent = detail ?? description
  const showHelp = helpContent != null && helpContent !== ''
  /** 页级开关开启或显式 descriptionVisible → 标题下常显 */
  const showDescriptionLine =
    (descriptionVisible || expandHelp) && showHelp

  const switchEl = switchConfig ? (
    <div className="setting-group-header-switch">
      <ToggleSwitch
        checked={switchConfig.checked}
        onChange={switchConfig.onChange}
        disabled={!!switchConfig.disabled || !!switchConfig.loading}
        aria-label={
          switchConfig.ariaLabel ||
          (typeof title === 'string' ? title : undefined)
        }
        preview={switchConfig.preview}
      />
    </div>
  ) : null

  const titleHelp =
    showHelp && !expandHelp ? (
      <SettingTitleHelp
        ariaLabel={title ? detailAria : t.config.detailHelpAria}
        tone={detailTone}
      >
        {helpContent}
      </SettingTitleHelp>
    ) : null

  const titleGuide = (
    <SettingTitleGuideEntry
      title={typeof title === 'string' ? title : ''}
      guide={guide}
    />
  )

  const titleActions = (
    <>
      {titleHelp}
      {titleGuide}
    </>
  )

  const titleRow = (withActions: boolean) =>
    title || titleExtra || showHelp || guide ? (
      <h4 className="setting-group-title">
        {icon && (
          <span className="setting-group-icon">
            {typeof icon === 'string' ? icon : icon}
          </span>
        )}
        {title && (
          <span className="setting-group-title-text">
            {title}
            {withActions ? titleActions : null}
          </span>
        )}
        {!title && withActions ? titleActions : null}
        {!collapsible && titleExtra}
      </h4>
    ) : null

  const descriptionEl = showDescriptionLine ? (
    <div
      className={`setting-group-description${
        detailTone === 'warning' ? ' is-warning' : ''
      }`}
    >
      {helpContent}
    </div>
  ) : null

  const showHeader = Boolean(
    title || titleExtra || showHelp || showDescriptionLine || switchConfig,
  )
  const switchOff = Boolean(switchConfig && !switchConfig.checked)

  const itemNodes = useMemo(
    () =>
      items?.map((itemProps, index) => (
        <SettingItem
          key={`${itemProps.itemKey || 'item'}-${index}`}
          {...itemProps}
        />
      )),
    [items],
  )

  const contentChildCount =
    (items?.length ?? 0) + React.Children.toArray(children).length
  const subgridSpan = (showHeader ? 1 : 0) + contentChildCount
  const showContent = !collapsible || isExpanded

  const header = showHeader ? (
    collapsible ? (
      <div
        className={`setting-group-header setting-group-header--collapsible${
          titleExtra || switchEl ? ' setting-group-header--with-extra' : ''
        }`}
      >
        <div
          className="setting-group-header-leading"
          onClick={(event) => {
            const target = event.target as HTMLElement
            if (
              target.closest(
                'a, button, input, select, textarea, [role="button"]',
              )
            ) {
              return
            }
            handleToggle()
          }}
        >
          <button
            type="button"
            className="setting-group-header-toggle"
            onClick={handleToggle}
            aria-expanded={isExpanded}
            aria-label={
              title
                ? (isExpanded
                    ? t.config.collapseGroupAria
                    : t.config.expandGroupAria
                  ).replace('{title}', String(title))
                : undefined
            }
          >
            <div className="setting-group-header-content">
              {titleRow(false)}
              {descriptionEl}
            </div>
          </button>
          <div className="setting-group-header-title-actions">{titleActions}</div>
          <button
            type="button"
            className="setting-group-chevron-hit"
            tabIndex={-1}
            aria-hidden
            onClick={handleToggle}
          >
            <span
              className={`setting-group-chevron ${isExpanded ? 'expanded' : ''}`}
              aria-hidden
            >
              <svg
                className="setting-group-chevron-icon"
                viewBox="0 0 12 10"
                width="10"
                height="8"
                focusable="false"
              >
                {/* 实心全圆角三角（展开指向下） */}
                <path
                  fill="currentColor"
                  d="M2.35 1.15h7.3c.78 0 1.22.88.76 1.52L7.1 7.55c-.52.72-1.68.72-2.2 0L1.59 2.67c-.46-.64-.02-1.52.76-1.52Z"
                />
              </svg>
            </span>
          </button>
        </div>
        {titleExtra && (
          <div className="setting-group-title-extra">{titleExtra}</div>
        )}
        {switchEl}
      </div>
    ) : (
      <div
        className={`setting-group-header${
          switchEl ? ' setting-group-header--with-switch' : ''
        }`}
      >
        <div className="setting-group-header-content">
          {titleRow(true)}
          {descriptionEl}
        </div>
        {switchEl}
      </div>
    )
  ) : null

  return (
    <div
      id={anchorId || undefined}
      {...(guidePath
        ? { [GUIDE_PATH_ATTR]: guidePath.trim() }
        : undefined)}
      className={[
        'setting-group',
        participateToc ? 'has-toc-anchor' : '',
        guidePath || participateToc ? 'has-guide-anchor' : '',
        collapsible ? 'is-collapsible' : '',
        collapsible && !isExpanded ? 'is-collapsed' : '',
        collapsible && isExpanded ? 'is-expanded' : '',
        switchOff ? 'is-switch-off' : '',
        inGrid ? 'setting-group--in-grid' : '',
        useSubgrid ? 'setting-group--subgrid' : '',
        className,
      ]
        .filter(Boolean)
        .join(' ')}
      style={
        useSubgrid && subgridSpan > 0
          ? ({
              gridRow: `1 / span ${subgridSpan}`,
            } as React.CSSProperties)
          : undefined
      }
    >
      {header}

      {useSubgrid ? (
        showContent && (
          <>
            {itemNodes}
            {children}
          </>
        )
      ) : collapsible ? (
        /* 折叠组走高度动画；收起动画播完才卸载内容 */
        <CollapseRegion open={isExpanded}>
          <div className="setting-group-content">
            {itemNodes}
            {children}
          </div>
        </CollapseRegion>
      ) : (
        <div className="setting-group-content">
          {itemNodes}
          {children}
        </div>
      )}
    </div>
  )
}

SettingGroup.displayName = 'SettingGroup'

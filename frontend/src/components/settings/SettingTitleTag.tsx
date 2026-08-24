/**
 * 设置标题旁标签提示
 * 标签 / chip 形态，可静态展示说明，也可点击跳转（如「前往高级配置」）
 * 可选 detail：hover ⓘ 显示详细说明 tooltip
 * 可选 onDismiss：右侧 × 关闭（默认值变更提示等）
 */

import type { ReactNode } from 'react'
import { LuX } from '@lib/icons'
import React, { useCallback } from 'react'
import { SettingTitleHelp } from './SettingTitleHelp'
import './SettingTitleTag.css'

export type SettingTitleTagVariant = 'default' | 'muted' | 'danger' | 'beta'

export interface SettingTitleTagProps {
  /** 标签文案 */
  children: ReactNode
  /** 左侧小图标 */
  icon?: ReactNode
  /** 有 onClick 时渲染为 button，可跳转 / 操作 */
  onClick?: () => void
  /** 原生 title（短提示）；详细说明请用 detail */
  title?: string
  /**
   * 详细说明：默认不展示，标签旁 ⓘ hover 显示 tooltip
   */
  detail?: ReactNode
  disabled?: boolean
  /**
   * default = 品牌色强调；muted = 中性信息；
   * danger = 选项/分组标题旁报错（与站点分析错误标签同款）；
   * beta = 实验功能标记
   */
  variant?: SettingTitleTagVariant
  className?: string
  /** 无障碍：danger 报错默认 role=alert */
  role?: string
  /** 右侧关闭；与整卡 onClick 互斥优先渲染为 span + 关闭按钮 */
  onDismiss?: () => void
  /** 关闭按钮 aria-label */
  dismissAriaLabel?: string
}

export const SettingTitleTag: React.FC<SettingTitleTagProps> = ({
  children,
  icon,
  onClick,
  title,
  detail,
  disabled = false,
  variant = 'default',
  className = '',
  role,
  onDismiss,
  dismissAriaLabel = 'Dismiss',
}) => {
  const classes = [
    'setting-title-tag',
    variant === 'muted' ? 'setting-title-tag--muted' : '',
    variant === 'danger' ? 'setting-title-tag--danger' : '',
    variant === 'beta' ? 'setting-title-tag--beta' : '',
    onDismiss ? 'setting-title-tag--dismissible' : '',
    onClick ? 'setting-title-tag--actionable' : '',
    className,
  ]
    .filter(Boolean)
    .join(' ')
  const a11yRole = role ?? (variant === 'danger' ? 'alert' : undefined)

  const handleClick = useCallback(
    (e: React.MouseEvent) => {
      // 折叠组标题是 button，阻止冒泡以免触发展开/收起
      e.stopPropagation()
      e.preventDefault()
      if (!disabled) onClick?.()
    },
    [disabled, onClick],
  )

  const handleDismiss = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation()
      e.preventDefault()
      if (!disabled) onDismiss?.()
    },
    [disabled, onDismiss],
  )

  const dismissBtn = onDismiss ? (
    <button
      type="button"
      className="setting-title-tag-dismiss"
      aria-label={dismissAriaLabel}
      disabled={disabled}
      onClick={handleDismiss}
    >
      <LuX aria-hidden />
    </button>
  ) : null

  const mainInner = (
    <>
      {icon && (
        <span className="setting-title-tag-icon" aria-hidden>
          {icon}
        </span>
      )}
      <span className="setting-title-tag-label">{children}</span>
    </>
  )

  // 可点 + 可关：主区域 button + 独立 ×，避免 button 嵌套
  if (onClick && onDismiss) {
    return (
      <span className={classes} title={title} role={a11yRole}>
        <button
          type="button"
          className="setting-title-tag-main"
          disabled={disabled}
          onClick={handleClick}
        >
          {mainInner}
        </button>
        {detail != null && detail !== '' && (
          <SettingTitleHelp>{detail}</SettingTitleHelp>
        )}
        {dismissBtn}
      </span>
    )
  }

  if (onClick) {
    return (
      <button
        type="button"
        className={classes}
        title={title}
        disabled={disabled}
        onClick={handleClick}
        role={a11yRole}
      >
        {mainInner}
        {detail != null && detail !== '' && (
          <SettingTitleHelp>{detail}</SettingTitleHelp>
        )}
      </button>
    )
  }

  return (
    <span className={classes} title={title} role={a11yRole}>
      {mainInner}
      {detail != null && detail !== '' && (
        <SettingTitleHelp>{detail}</SettingTitleHelp>
      )}
      {dismissBtn}
    </span>
  )
}

SettingTitleTag.displayName = 'SettingTitleTag'

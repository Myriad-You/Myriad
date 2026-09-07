/**
 * 标题旁短说明 ⓘ：默认 hover / focus 以 tooltip 显示。
 * Tooltip 通过 Portal 挂到 document.body + fixed 定位，避免被 overflow 裁切。
 *
 * 仅用于 detail / description 短文案。「显示说明」开启后由父组件改为标题下常显。
 * 结构化长指南请用 SettingTitleGuideEntry（点击展开），不要塞进本组件。
 */

import type { ReactNode } from 'react'
import type { SettingHoverTooltipTone } from './useSettingHoverTooltip'
import { LuInfo } from '@lib/icons'
import React from 'react'
import { useI18n } from '../../contexts/I18nContext'
import { useSettingHoverTooltip } from './useSettingHoverTooltip'
import './SettingTitleHelp.css'

export type SettingTitleHelpTone = SettingHoverTooltipTone

export interface SettingTitleHelpProps {
  /** 详细说明内容（支持富文本 / 链接） */
  children: ReactNode
  /** 触发器无障碍名 */
  ariaLabel?: string
  /** 视觉语气：warning 用于阻断性提示 */
  tone?: SettingTitleHelpTone
  /** 首选方向；空间不足时自动翻转 */
  placement?: 'top' | 'bottom'
  className?: string
}

export const SettingTitleHelp: React.FC<SettingTitleHelpProps> = ({
  children,
  ariaLabel: ariaLabelProp,
  tone = 'default',
  placement = 'bottom',
  className = '',
}) => {
  const { t } = useI18n()
  const ariaLabel = ariaLabelProp ?? t.config.detailHelpAria
  const { triggerRef, tooltip, open, show, hide, tooltipId } =
    useSettingHoverTooltip<HTMLButtonElement>({
      content: children,
      placement,
      tone,
    })

  if (children == null || children === false || children === '') return null

  return (
    <span
      className={[
        'setting-title-help',
        `setting-title-help--${tone}`,
        className,
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <button
        ref={triggerRef}
        type="button"
        className="setting-title-help-trigger"
        aria-label={ariaLabel}
        aria-describedby={open ? tooltipId : undefined}
        aria-expanded={open}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
      >
        <LuInfo className="setting-title-help-icon" aria-hidden />
      </button>
      {tooltip}
    </span>
  )
}

SettingTitleHelp.displayName = 'SettingTitleHelp'

export default SettingTitleHelp

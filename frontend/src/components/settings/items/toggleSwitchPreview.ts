/**
 * ToggleSwitch 悬停预告：根据开关状态选出要显示的后果文案。
 * 无 I/O，供组件与单测共用。
 */

import type { ReactNode } from 'react'

export interface ToggleSwitchPreview {
  /** 关着时：开启以后会怎样 */
  on?: ReactNode
  /** 开着时：关闭以后会怎样 */
  off?: ReactNode
  /** 禁用时：为什么点不了 */
  disabled?: ReactNode
}

export interface ToggleSwitchPreviewResolved {
  body: ReactNode
  /** 是否显示「开启后 / 关闭后」短标签；禁用说明不加 */
  showKicker: boolean
}

function isUsableCopy(node: ReactNode): boolean {
  return node != null && node !== false && node !== ''
}

export function resolveToggleSwitchPreview(
  preview: ToggleSwitchPreview | undefined,
  checked: boolean,
  disabled: boolean,
): ToggleSwitchPreviewResolved {
  if (!preview) return { body: null, showKicker: false }

  if (disabled && isUsableCopy(preview.disabled)) {
    return { body: preview.disabled, showKicker: false }
  }

  const node = checked ? preview.off : preview.on
  if (!isUsableCopy(node)) return { body: null, showKicker: false }
  return { body: node, showKicker: true }
}

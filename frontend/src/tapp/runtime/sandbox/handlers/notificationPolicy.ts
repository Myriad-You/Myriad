import type { UserRole } from '../../../types'
import type { TappNotificationOptions } from '../types'

export type TappNotificationType = 'success' | 'error' | 'warning' | 'info'

export interface PreparedTappNotification {
  delivery: 'session' | 'durable'
  title: string
  message: string
  type: TappNotificationType
}

export function prepareTappNotification(
  options: TappNotificationOptions,
  role: UserRole,
): PreparedTappNotification {
  const requestedType = String(options.type || 'info')
  const type: TappNotificationType = [
    'success',
    'error',
    'warning',
    'info',
  ].includes(requestedType)
    ? (requestedType as TappNotificationType)
    : 'info'

  return {
    delivery: role === 'guest' ? 'session' : 'durable',
    title: String(options.title || 'Tapp 通知').slice(0, 120),
    message: String(options.message || '').slice(0, 1000),
    type,
  }
}

export function canMutateDynamicContent(role: UserRole): boolean {
  return role !== 'guest'
}

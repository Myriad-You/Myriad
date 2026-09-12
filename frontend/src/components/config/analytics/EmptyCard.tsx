import type { ReactNode } from 'react'
import { LuInbox } from '@lib/icons'
import React from 'react'

interface EmptyCardProps {
  text: string
  icon?: ReactNode
  loading?: boolean
  tall?: boolean
}

export const EmptyCard: React.FC<EmptyCardProps> = ({
  text,
  icon,
  loading = false,
  tall = false,
}) => (
  <div
    className={`site-analytics-empty${tall ? ' site-analytics-empty--tall' : ''}`}
    role={loading ? 'status' : undefined}
  >
    <span className="site-analytics-empty-icon" aria-hidden>
      {icon ?? <LuInbox size={18} />}
    </span>
    <span>{loading ? '…' : text}</span>
  </div>
)

export default EmptyCard

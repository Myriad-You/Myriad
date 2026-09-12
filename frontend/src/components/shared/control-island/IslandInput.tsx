import { Spinner } from '../../Spinner'
import {
  ISLAND_BTN_PRIMARY,
  ISLAND_INPUT,
  ISLAND_INPUT_STYLE,
} from './constants'

export interface IslandInputProps {
  value: string
  onChange: (v: string) => void
  onSubmit: () => void
  placeholder: string
  loading?: boolean
  buttonLabel: string
  icon?: React.ReactNode
  children?: React.ReactNode
  isMobile?: boolean
}

export function IslandInput({
  value,
  onChange,
  onSubmit,
  placeholder,
  loading = false,
  buttonLabel,
  icon,
  children,
  isMobile = false,
}: IslandInputProps) {
  return (
    <div className="flex items-center gap-1.5">
      <div
        className={`flex items-center gap-2 px-3 h-9 ${isMobile ? 'flex-1' : ''}`}
      >
        {icon || (
          <svg
            className="w-4 h-4 text-gray-400 dark:text-gray-500 shrink-0"
            fill="none"
            stroke="currentColor"
            viewBox="0 0 24 24"
          >
            <path
              strokeLinecap="round"
              strokeLinejoin="round"
              strokeWidth={2}
              d="M15.232 5.232l3.536 3.536m-2.036-5.036a2.5 2.5 0 113.536 3.536L6.5 21.036H3v-3.572L16.732 3.732z"
            />
          </svg>
        )}
        <input
          type="text"
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && onSubmit()}
          placeholder={placeholder}
          className={`${isMobile ? 'flex-1 min-w-0' : 'w-64'} ${ISLAND_INPUT}`}
          style={ISLAND_INPUT_STYLE}
        />
      </div>
      {children}
      <button
        type="button"
        onClick={onSubmit}
        disabled={loading || !value.trim()}
        className={ISLAND_BTN_PRIMARY}
      >
        {loading ? (
          <Spinner size="xs" color="white" />
        ) : (
          buttonLabel
        )}
      </button>
    </div>
  )
}

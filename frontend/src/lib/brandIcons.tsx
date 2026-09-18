import type { IconType } from 'react-icons'
import { withIconA11y } from './iconA11y'

const MyriadStoreIconRaw: IconType = ({ size, style, title, ...props }) => {
  const iconSize = size ?? '1em'

  return (
    <svg
      viewBox="0 0 24 24"
      width={iconSize}
      height={iconSize}
      xmlns="http://www.w3.org/2000/svg"
      style={{ verticalAlign: 'middle', ...style }}
      {...props}
    >
      {title ? <title>{title}</title> : null}
      <g
        fill="none"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        transform="translate(12 12) scale(1.16) translate(-12 -12)"
      >
        <path
          d="M6.55 5.35h10.9a1.85 1.85 0 0 1 1.78 1.35l.68 2.55H4.09l.68-2.55a1.85 1.85 0 0 1 1.78-1.35Z"
          strokeWidth="1.75"
        />
        <path
          d="M4.6 9.25v1.15a2.35 2.35 0 0 0 4.7 0V9.25"
          strokeWidth="1.75"
        />
        <path d="M9.3 9.25v1.15a2.7 2.7 0 0 0 5.4 0V9.25" strokeWidth="1.75" />
        <path
          d="M14.7 9.25v1.15a2.35 2.35 0 0 0 4.7 0V9.25"
          strokeWidth="1.75"
        />
        <path
          d="M5.75 13.25v4.45a1.95 1.95 0 0 0 1.95 1.95h8.6a1.95 1.95 0 0 0 1.95-1.95v-4.45"
          strokeWidth="1.75"
        />
      </g>
    </svg>
  )
}

/** Shell / welcome chrome. Keep this module free of react-icons barrels. */
export const MyriadStoreIcon = withIconA11y(MyriadStoreIconRaw)

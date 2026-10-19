import type { IconType } from 'react-icons'
import { withIconA11y } from './iconA11y'
import { navIslandItem } from './navIslandItems'

/** 和导航岛的 Tapp 条目同一个图标（`shared/nav_island.json`）。 */
const STORE = navIslandItem('tapp').icon

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
        strokeWidth={STORE.strokeWidth}
        transform={STORE.transform}
      >
        {STORE.paths.map((d) => (
          <path key={d} d={d} />
        ))}
      </g>
    </svg>
  )
}

/** Shell / welcome chrome. Keep this module free of react-icons barrels. */
export const MyriadStoreIcon = withIconA11y(MyriadStoreIconRaw)

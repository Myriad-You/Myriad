import { SkeletonBone, WidgetSkeleton } from './shared/WidgetSkeleton'
import './MeropeSkeleton.css'

/**
 * 她还没到时，人物那块只是一片主题色的光晕和一道扫过的光，不画人形。
 * 卡片上右下角再留出名牌的位置；弹窗有自己的名字区，不要名牌。
 * 放进 WidgetSkeletonCover 当 children，骨块按骨架容器绝对定位。
 */
export function MeropeFigureBones({
  nameplate = false,
  compact = false,
}: {
  nameplate?: boolean
  compact?: boolean
}) {
  return (
    <>
      <SkeletonBone className="merope-skeleton__glow" rounded="none" />
      {nameplate ? (
        <SkeletonBone
          className={`merope-skeleton__nameplate${compact ? ' merope-skeleton__nameplate--compact' : ''}`}
          rounded="none"
        />
      ) : null}
    </>
  )
}

/** 弹窗名字区在名字和心情读到之前：和真内容同样的大小和位置，到了直接换上不跳。 */
export function MeropeHeadSkeleton({ label }: { label: string }) {
  return (
    <WidgetSkeleton label={label} className="merope-skeleton-head">
      <SkeletonBone className="merope-skeleton__name" rounded="md" />
      <span className="merope-skeleton__pills">
        <SkeletonBone className="merope-skeleton__pill" rounded="full" muted />
        <SkeletonBone className="merope-skeleton__pill" rounded="full" muted />
      </span>
    </WidgetSkeleton>
  )
}

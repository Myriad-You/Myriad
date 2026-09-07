/**
 * 资料库「正在播 / 换歌退场」动效时长（ms）— 一处改、全局对齐。
 * LEAVE_HOLD ≥ 歌词退场 ≥ CSS is-leaving；COVER_EXIT 对齐呼吸收回 transition。
 */
export const LIBRARY_LIVE_MS = {
  /** 换歌后旧卡保留挂载，盖住歌词+光带+封面退场 */
  leaveHold: 560,
  /** LibraryCardLyrics 卸 DOM（对齐 .is-leaving ~0.52s） */
  lyricsUnmount: 520,
  /** 封面呼吸收回后卸 phase */
  coverExit: 600,
} as const

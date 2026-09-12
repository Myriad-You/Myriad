export const TOUR_HINT_AUTO_HIDE_MS = 30_000

export function shouldAutoHideTourHint(
  dev: boolean = import.meta.env.DEV,
): boolean {
  return !dev
}

export const TAPP_SHELL_CHROME_PAD_CLASS = 'px-3 sm:px-4 md:px-6' as const

export const TAPP_SHELL_CHROME_PB_DESKTOP = '1.5rem' as const

export const TAPP_SHELL_CHROME_PB_MOBILE =
  'max(5.5rem, calc(env(safe-area-inset-bottom, 0px) + 4.5rem))' as const

/** 全屏只切 CSS；iframe / 宿主面板保持挂载。 */
export function tappShellContentClass(isFullscreen: boolean, extra?: string): string {
  const base =
    'pointer-events-auto overflow-hidden transition-[border-radius] duration-300 ease-out'
  const mode = isFullscreen
    ? 'fixed inset-0 z-[1] rounded-none'
    : 'absolute inset-0 rounded-b-xl'
  return extra ? `${base} ${mode} ${extra}` : `${base} ${mode}`
}

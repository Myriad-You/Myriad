import { workbenchMediaFormatKey } from './workbench'

/** 媒体库收的类型，和后端 `ALLOWED_IMAGE_MIMES` / `ALLOWED_VIDEO_MIMES` 对齐。 */
export const WORKBENCH_MEDIA_ACCEPT =
  'image/jpeg,image/png,image/gif,image/webp,video/mp4,video/webm,video/quicktime'

/** 分出能传的和不能传的。类型认不出时按扩展名认（拖进来的 .mov 有时不带类型）。 */
export function splitWorkbenchUploads<T extends { type: string; name: string }>(
  files: readonly T[],
): { accepted: T[]; rejected: T[] } {
  const accepted: T[] = []
  const rejected: T[] = []
  for (const file of files) {
    if (workbenchMediaFormatKey({ mime: file.type, name: file.name }) === 'other') rejected.push(file)
    else accepted.push(file)
  }
  return { accepted, rejected }
}

/**
 * 从外面拖进来的文件才接。页面里拖文字、图片元素时浏览器有时也报 `Files`，
 * 所以还要看这次拖动是不是从本页开始的。
 */
export function isOutsideFileDrag(types: readonly string[], fromPage: boolean): boolean {
  return !fromPage && types.includes('Files')
}

/** 一个接一个传，不并发；一个失败不拦后面的。`onStart` 收到的是从 0 数的序号。 */
export async function uploadInOrder<T>(
  items: readonly T[],
  run: (item: T) => Promise<unknown>,
  onStart?: (index: number) => void,
): Promise<{ done: number; failed: Array<{ item: T; error: unknown }> }> {
  let done = 0
  const failed: Array<{ item: T; error: unknown }> = []
  for (const [index, item] of items.entries()) {
    onStart?.(index)
    try {
      await run(item)
      done += 1
    } catch (error) {
      failed.push({ item, error })
    }
  }
  return { done, failed }
}

import { existsSync } from 'node:fs'

/**
 * A file of the shared core, for tests that hold the figure to it. core/ is
 * this repository's submodule; inside Myriad that one stays empty and the
 * site's own core/ sits beside figure/.
 */
export function coreFile(path: string): URL {
  const own = new URL(`../../core/${path}`, import.meta.url)
  return existsSync(own) ? own : new URL(`../../../core/${path}`, import.meta.url)
}

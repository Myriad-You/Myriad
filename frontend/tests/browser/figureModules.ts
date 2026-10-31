import { createRequire } from 'node:module'
import { dirname } from 'node:path'

/**
 * The /@fs URL of the figure's anime25drig/ directory, found through the
 * @myriad/figure package rather than where figure/ sits in this checkout.
 */
export const FIGURE_RUNTIME_MODULES = `/@fs${dirname(
  createRequire(import.meta.url).resolve('@myriad/figure/anime25drig'),
)}/`

import type { ExampleTapp } from './tapps/types'
import { helloWorldTapp } from './tapps/helloWorld'

export { helloWorldTapp } from './tapps/helloWorld'

export type { ExampleTapp } from './tapps/types'

export const EXAMPLE_TAPPS: ExampleTapp[] = [
  helloWorldTapp,
]

export default EXAMPLE_TAPPS

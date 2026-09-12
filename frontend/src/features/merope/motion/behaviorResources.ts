import type { MotionChannel } from './channels'

export type BehaviorResource =
  | 'face.mouth'
  | 'face.expression'
  | 'face.gaze'
  | 'body.head'
  | 'body.torso'
  | 'body.arm.left'
  | 'body.arm.right'
  | 'body.hand.left'
  | 'body.hand.right'
  | 'secondary.hair'
  | 'secondary.clothing'
  | 'secondary.bust'

export type BehaviorResourceGroup =
  | 'face'
  | 'face.mouth'
  | 'face.expression'
  | 'face.gaze'
  | 'body'
  | 'body.head'
  | 'body.torso'
  | 'body.arm'
  | 'body.arm.left'
  | 'body.arm.right'
  | 'body.hand'
  | 'body.hand.left'
  | 'body.hand.right'
  | 'secondary'
  | 'secondary.hair'
  | 'secondary.clothing'
  | 'secondary.bust'

export function resourceInGroup(
  resource: BehaviorResource,
  group: BehaviorResourceGroup,
): boolean {
  return resource === group || resource.startsWith(`${group}.`)
}

export function resourcesConflict(
  left: BehaviorResourceGroup,
  right: BehaviorResourceGroup,
): boolean {
  return (
    left === right ||
    left.startsWith(`${right}.`) ||
    right.startsWith(`${left}.`)
  )
}

/** Channel inventory is this rig; `rig/README.md` is the boundary. */
export function rigChannelsForResources(
  resources: readonly BehaviorResource[],
): MotionChannel[] {
  const channels = new Set<MotionChannel>()
  for (const resource of resources) {
    if (resource === 'face.mouth') channels.add('mouth')
    else if (resource === 'face.expression') channels.add('expression')
    else if (resource === 'face.gaze') channels.add('gaze')
    else channels.add('headBody')
  }
  return Iterator.from(channels).toArray()
}

export function resourcesForRigChannels(
  channels: readonly MotionChannel[],
): BehaviorResource[] {
  const resources = new Set<BehaviorResource>()
  for (const channel of channels) {
    if (channel === 'mouth') resources.add('face.mouth')
    if (channel === 'expression') resources.add('face.expression')
    if (channel === 'gaze') resources.add('face.gaze')
    if (channel === 'headBody') {
      resources.add('body.head')
      resources.add('body.torso')
      resources.add('body.arm.left')
      resources.add('body.arm.right')
    }
  }
  return Iterator.from(resources).toArray()
}

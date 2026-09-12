import type { AnnotationType } from '../../../services/brewliaApi'
import {
  ANNOTATION_TYPE_CONFIG,
  annotationTypeLabel,
} from '../../../services/brewliaApi'

export function annotationChrome(type: string): {
  label: string
  color: string
  bgColor: string
} {
  const known = Object.hasOwn(ANNOTATION_TYPE_CONFIG, type)
    ? (type as AnnotationType)
    : 'term'
  const config = ANNOTATION_TYPE_CONFIG[known]
  return {
    label: annotationTypeLabel(known),
    color: config.color,
    bgColor: config.bgColor,
  }
}

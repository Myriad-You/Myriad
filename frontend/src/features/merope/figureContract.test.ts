import * as root from '@myriad/figure'
import { figureContract } from '@myriad/figure-interface/contract'
import * as anime25drig from '@myriad/figure/anime25drig'
import * as events from '@myriad/figure/events'
import * as motion from '@myriad/figure/motion'
import * as performance from '@myriad/figure/performance'
import * as rig from '@myriad/figure/rig'
import * as speech from '@myriad/figure/speech'

// The figure this site is built with keeps the runtime contract every implementation keeps.
figureContract({ root, anime25drig, events, motion, performance, rig, speech })

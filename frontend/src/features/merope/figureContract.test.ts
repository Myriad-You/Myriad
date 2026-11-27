import { figureContract } from '@myriad-you/figure-interface/contract'
import * as root from '@myriad/figure'
import * as anime25drig from '@myriad/figure/anime25drig'
import * as events from '@myriad/figure/events'
import * as interaction from '@myriad/figure/interaction'
import * as motion from '@myriad/figure/motion'
import * as performance from '@myriad/figure/performance'
import * as rig from '@myriad/figure/rig'
import * as singing from '@myriad/figure/singing'
import * as speech from '@myriad/figure/speech'
import * as workbench from '@myriad/figure/workbench'

// The figure this site is built with keeps the runtime contract every implementation keeps.
figureContract({ root, anime25drig, events, interaction, motion, performance, rig, singing, speech, workbench })

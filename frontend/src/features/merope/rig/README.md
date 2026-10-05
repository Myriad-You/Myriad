# Merope 2.5D motion system

This directory owns the site-wide upper-body character compile path. Live
playback is Anime2.5DRig in `../anime25drig`. There is no clip-stack driver.

## Hard boundary

Rig IR v4 exposes only these semantic roles:

```text
root, torso, head, face, left-eye, right-eye, mouth, handwear
```

There are no shoulder, elbow, wrist, leg, foot, contact-IK, locomotion, or hand
pose bones in the IR. `handwear` is the shared semantic parent; optional
`a25d-handwear-left/right` children hold the two sleeve/partial-forearm/hand
drawings, which swing within ±15 degrees. The only bending inside a drawing
comes from joints a pose model found on the master portrait
(`anchors.skeleton`): past a found elbow the forearm, and past a found wrist
the hand, each turn after the segment above them; a full figure's legs bend at
their found hip, knee and ankle. Without a skeleton a sleeve stays one rigid
piece. The joints bind wherever the arm is posed, hanging, bent or raised:
each vertex takes the forearm's and the hand's turns by which bone it lies
nearer, blended across the joint.

At bind time each split sleeve gets one shoulder joint, found on its own
opaque pixels nearest the anatomical shoulder (never above the neck), and the
rotation fades in across the joint so the sleeve bends into the shoulder rather
than tearing from it. At runtime the sleeve is a damped pendulum: `armY` is a
shared lift that the two arms do not take alike: the arm on the side the head
turns to (or the sway goes, or else the other arm from last time) leads, and
the other comes along with 40% of it a quarter second behind. An arm with a
found elbow takes 40% of its lift at the elbow instead: a little of it turns
the forearm out, most brings it forward towards the viewer, drawn as a forearm
up to 35% shorter with its hand carried up and up to 15% larger; a standing figure's arm over the leg with the weight is pushed out by its
hip. `armPos` swings both one way, the shoulder's actual acceleration
after all primary motion makes the hand trail, and half of a body roll is given
back to gravity. The swing saturates softly into the contract's
`maxRigidArmRotationDegrees`. A sleeve that runs into the portrait crop keeps
its cut edge on the frame line and slides along it instead of lifting off.
A drawing whose hand is raised above its shoulder swings at 30%.

A sleeve that is fabric all the way down (under 5% skin-toned pixels in its
lower half, so no forearm or hand shows) is a drape: below a third of its length
it blends from the arm's swing to its own softer pendulum, which trails the arm
and settles 30% nearer vertical. A visible forearm bends only at a found elbow;
a raised arm never drapes. Ribbons and tassels painted inside a sleeve cannot be separated
from it and move with the sleeve.

This boundary is shared by
`shared/merope_rig_contract.json`, the Rust compiler, TypeScript
validation, the PSD importer, and diagnostics. Do not add a frontend-only
semantic role.

## Asset flow

```text
portrait source
  -> owner-triggered remote See-through API or artist-authored layered PSD
  -> ephemeral PSD download (never activated directly)
  -> ag-psd validation and Anime2.5D name normalization
  -> alpha cleanup, eye-side split, atlas packing, anchors and mesh weights
  -> authenticated backend preview / migration / diagnosis
  -> capability-regression gate
  -> explicit commit of the exact preflighted source and atlas
  -> active immutable rig asset
```

The Myriad backend never launches the decomposition model. The owner may store
a write-only Hugging Face token and explicitly send the current cached master
portrait to `24yearsold/see-through-demo`; the browser never receives the
token or an upstream download URL. The returned PSD is ephemeral and enters
the exact same preflight as a manually selected file. No candidate is
activated directly. See
[`docs/design/merope-25d-pipeline.md`](../../../../../docs/design/merope-25d-pipeline.md)
for the asset-builder boundary and third-party integration policy.

`anime25dImporter.ts` coordinates the transaction while focused compiler
modules own normalization output, generated expressions, high-collar recovery,
atlas packing, skeleton/mesh construction, and final capability validation.
Both Anime2.5DRig names and native See-through tags such as `hairf`, `hairb`,
`eyer`, and side-suffixed eye layers map into stable roles. Unknown decorative
layers stay renderable but do not create new semantic bones.

`anime25dLayerSemantics.ts` owns category and alias resolution for both import
and live binding. See-through's `neckwear`, `eyewear`, `wings`, `tail`, and
`objects` are recognized drawings, not additional articulated rig roles.
Numbered and sided accessory fragments retain independent identities. Existing
packages whose named accessory layers carry `unknown` are resolved in memory;
their atlas, draw order, and stored manifest are not rewritten.

Independent ornaments ride a rigid local frame sampled from the existing
neck/head/garment deformation (`layerAttachment.ts`). They inherit the mount's
translation and rotation, not its scale or shear. This also supplies a bounded
head/body attachment for unknown decoration; it does not infer material or add
wing, tail, or hanging-jewelry physics. `neckwear` is an upstream region that
includes necklaces, ties, scarves, and collars—not evidence of a high collar.
Ordinary neck roots share torso projection, fading it out toward the head.
Binding measures the visible alpha footprint once, ignoring transparent PSD
padding and preferring a same-side host with actual pixel overlap over an empty
bounding rectangle. Ear ornaments use the upper visible root and prefer ears;
detached artwork without overlap retains its semantic surface mount. This does
not split a combined chain/pendant or infer cloth physics. Pixel reads are shared
within binding and are never performed by the frame loop.

High-collar recovery first requires broad upper-neck material differences from
the co-located neck raster, in addition to the existing alpha coverage gate.
Copied skin/shadows and narrow chains cannot establish a collar. Once there is
positive evidence, the established reference/colour partition remains intact;
uniform real collars can still use the geometric partition. Already authored
collar topology is left alone. Multiple candidate necklines are ambiguous for
the single runtime aperture and are left unsplit, not arbitrarily merged.
This remains an image heuristic: near-identical skin/garment colours can be
uncertain. It neither repaints textures nor reconstructs hidden skin or fabric.

Open-neck compositing is a separate runtime binding decision (`neckSurface.ts`).
See-through sometimes inpaints chest skin into `topwear`; drawing it over the
neck truncates the chin shadow into a horizontal seam. Only a unique neck/body
pair with an exposed upper neck, an opaque lower backing, and a broad matching
colour interval can draw the neck over that body. Its bottom fades in rest UV
space, before contaminated cut-edge pixels, without changing the stored atlas
or playback. Compiled collars always bypass this path; covered upper necks,
unsupported joins, and ambiguous pairs leave the original ordering intact.
This cannot remove necklace remnants already baked into the chest texture.

Large `objects`, `wings`, and `tail` drawings have the same non-framing policy as
large unclassified extras. Recognizing a name must not suddenly shrink the
portrait; these drawings remain in the asset. Small extras retain their margins.

Myriad extends the upstream eye-diff path with `eye_dizzy`, `eye_squeeze`, and
`eye_cry` layers. Artist artwork is split per eye like `eye_close`; when it is
absent, import generates character-tinted spiral, inward-chevron, or
asymmetric chevron-and-tear eyes from the independent eye anchors. All three
presentation variants are compiled into the atlas and are never emulated by
warping the open eye.

The vacant `silly` stare splits into two parts per eye: a generated near-round
sclera frame with a thick rim, and the character's own iris drawing resampled
onto its own layer. Only the frame is new artwork, because the round wide-open
shape cannot be reached by warping the authored eye; the iris keeps its drawn
colour, gradient, rim, and catchlights, and a generated disc is used only when
a portrait ships no separate iris layer at all.
Import seeds the two irides at opposite offsets, and the runtime drifts each one
on its own keyframe track, so the eyes lose focus separately.
That divergence is impossible through the shared `eyeX`/`eyeY` gaze, which the
expression deliberately never touches.
Import measures how far an iris can move before it reaches the drawn rim, spends
a fixed share of that room on the resting divergence, and leaves the rest to the
loop, whose drift is bounded by the same figure. Successive looks also start at
different points in the six-second loop, so a face that goes vacant twice in one
conversation does not replay the same animation.

Import also synthesizes optional `anger_mark` and `speechless_sweat` manga
accents from the face scale. They stay out of the neutral analysis reference;
runtime facial deformation remains the primary expression signal and stages
the accents after the brows, gaze, lids, and mouth have begun moving.

Every generated drawing is placed in the portrait's own face frame
(`../expressionShapes/faceFrame.ts`): the iris-to-iris line gives the roll, its midpoint the
origin. Eye, mouth, and heart glyphs are drawn upright and then rotated about
their own pivot by that roll, so a tilted head gets tilted glyphs. The
lovestruck blush sits under the drawn lower lids, and the anger mark and sweat
drop hang off the face drawing's contour at eye height rather than off corners
of the face's bounding box. Authored artwork and the resampled silly iris keep
their drawn orientation. A portrait with no mouth drawing gets a mouth anchor on
the face midline instead of the frozen rigger's fixed-pixel guess. All of this is
baked at import; stored rigs change only when re-imported.

When the square source illustration is available, import reconciles the split
against it and repairs what the illustration proves (`psdReconciliation.ts`,
`psdRepair.ts`). The layers visible at rest are composited in draw order and
compared with the illustration they were decomposed from. Each disagreeing
region is classified by cause: `buried` when a lower layer holds matching art
that a later layer covers (an ordering fault), `missing` when the illustration
shows art that no layer carries (e.g. a dropped earring), `spurious` when a
layer paints over open backdrop, and `mismatch` when the visible layer differs
and nothing beneath fits. Backdrop is flood-filled from the open edges, so pale
skin or white hair enclosed by line art is not mistaken for it. See-through
re-renders colours, so thresholds allow ~15–18 RGB of drift. Below 80%
agreement the PSD is treated as split from another illustration and left
untouched. Scenic backdrops disable the backdrop-dependent verdicts.

Repair then acts on those verdicts. A `buried` region whose covered layer
matches the illustration as a whole (mean distance ≤ 26; faithful reveals
measured 7–23, coincidental ones 31–37) is revealed by erasing the covering
pixels. The reveal grows past the verdict threshold for as long as some covered
layer still beats the composite, so it stops at the covering drawing's own
outline instead of a jagged threshold contour, and the removed drawing's
translucent rim is cleared so no ghost outline remains. Anything else
the illustration shows (`missing`, `mismatch`, weak `buried`) is lifted from
the illustration itself. A morphological closing plus small-hole fill keeps an
accessory whole, so a birdcage earring comes back with the hair seen between
its bars. Each recovered piece then follows what holds it: one whose top end
reaches a rigid accessory is painted into that drawing (a tassel under a hair
ornament becomes part of the ornament, so a head turn cannot separate them),
one hanging from the ears becomes `earwear`, one on hair `headwear`, one at the
neck `neckwear`, and anything else a rigid `objects` layer, each placed just
above the layers it overrides. Joining an accessory needs nothing drawn above
it to cover the piece at rest; otherwise the piece gets its own layer. Beyond
the `MAX_RIG_PARTS` budget, pieces of one body part share a plain `objects`
layer. Layers with expression variants are never edited or covered, and areas
above 2% of the content are too garment-sized to pin as static art; those are
only reported. Surfaces the illustration hides cannot be recovered. Preflight shows
the repairs in green on the difference map alongside what remains. Faults the
runtime corrects later (for example lifting `neckwear` over recovered skin)
are judged before that correction.

The `lovestruck` expression keeps the character's authored irises and overlays
one independently anchored, character-tinted heart pupil per eye. A single
face-local atlas rectangle carries the broad blush, cheek hatching, and three
small sweat drops, while a separate drool layer follows the continuously
deformed mouth corner. The normal eyelids still blink over the heart pupils and
speech retains ownership of articulation instead of switching to a replacement
mouth texture.

See-through's plain `mouth` is treated as the static closed portrait drawing,
not as a speaking phoneme. Import keeps that artwork and generates a small
character-tinted cel-style `mouth_open`, `mouth_wide`, `mouth_round`, and
`mouth_narrow` shapes plus independent `mouth_cry` and face-scaled
`mouth_maniac` glyphs.
A single face-scaled `mouth_silly` shape is generated alongside them. It is not
a viseme: the runtime holds one mesh and presses it onto an omega curve when
closed, then opens it continuously into a small cat mouth, so the vacant face
never crossfades between two mouth drawings. The eyes and the mouth are owned
separately, so a vacant cue arriving mid-reply keeps the stare while speech
keeps the articulating mouth; the omega only takes over once the line ends.
The runtime morphs the speaking meshes through one continuous articulation
envelope. A lip-seal channel preserves short bilabial closures independently
from the slower jaw response. The two strongest visemes form a shared
dominance bridge, while a stateful selector draws exactly one ordinary mouth
texture at a time; crying replaces that mouth stack.
Import also records the six alpha silhouettes and their fifteen pairwise bridge
profiles. Jaw travel is then driven on a separate bounded spring: open and
round visemes use more mandible motion, wide and narrow visemes rely more on
the lip mesh, and short lip seals do not snap the jaw shut. Only the mouth
meshes and the face region below the mouth receive that motion.
Character asset contract v13 requires the independent variants, mouth profile,
and clothing-aware chest profile, so older packages must be reimported rather
than falling back at runtime.

Chest analysis is also import-time only. Its v2 profile records apparent size,
the garment-aware visible deformation ellipse, mechanical support, and how much
local soft-tissue response reaches the outer topwear. A structured or compressed
surface therefore follows the torso more tightly, while loose or rigid outer
layers suppress localized deformation without pretending the underlying size
changed. Male policy still disables this path authoritatively.

At runtime the imported chest centre is treated as an attachment base. A
two-axis relative-velocity spring follows that base, so motion starts in the
same direction as the torso; inertia can cross into the opposite direction only
after the torso slows or turns. Apparent size controls the base response,
support controls frequency and damping, and garment transmission controls the
bounded visible blend. The deformation weights are sampled in rest-mesh space,
preventing the active region from sliding across the clothing during a pose.
Whole-body rotation excites only the relative spring, avoiding duplicate rigid
travel. Flat profiles remain restrained while medium and large profiles open a
continuous higher-gain, lower-damping inertia range.

## Runtime flow

```text
active layered package
  -> ../character/RigCharacter.tsx
  -> Anime25DCharacter / Anime2.5DRig player
```

Agent reply speech enters through `../events/speechEvents.ts`. The lifecycle controller
handles streamed chunks, complete replies, interruption, and proactive lines,
then publishes into `../motion/speechSource.ts`, not directly into a player.
Real audio energy or phoneme events own
the mouth when present; otherwise the bounded local auto-prosody controller is
used. The bridge reuses fixed typed arrays; the jaw and chest paths use fixed
scalar state with constant-time spring arithmetic. Clothing analysis never runs
inside the render loop.

Without a live Anime2.5D package, `RigCharacter` draws the master portrait as a
still image. Manifests do not carry clip stacks. There is no separate global
face overlay.

### Speech delivery ownership

- `../speech/phrasePlan.ts` grounds the director's optional `phrases` in unique
  response fragments. It annotates existing prosody anchors; it does not invent
  another clock. Quotes/code/URLs and ambiguous fragments are not acted from
  these annotations. Missing matches keep local delivery rather than guessing.
- `../motion/speechSource.ts` keeps raw timing separate from annotated delivery.
  Direction is bounded in memory and scoped by source, generation and message;
  consecutive TTS segments retain that message's direction. Actual speech
  events own the utterance lifetime and cancellation.
- `../motion/humanPerformanceRuntime.ts` merges speech, music and performance
  candidates. Its resolved pegs are authoritative. Phrase revisions preserve
  committed beats; same-meaning director coverage can suppress future ones.
- `../anime25drig/behaviorMotion.ts` realizes speech shape shares under the
  existing unit envelope. `speechFormTransition.ts` smooths a changed shape
  from the drawn mixture without adding a new behavior or moving its peak.
  An unseen beat changes directly; cancellation releases the drawn level.
- `../motion/applyFrame.ts` forwards changes in gesture as well as timing.
  `../anime25drig/speechExpression.ts` respects `none` in its TTS accent path,
  while keeping normal speech activity and later unsuppressed accents.

These are separate responsibilities, not interchangeable fallbacks. `none`
suppresses an accent, not all body motion or the mouth. An unmatched fragment,
a phrase crossing TTS segment boundaries, or a revision arriving after
commitment may leave local delivery unchanged. Unit tests establish routing,
timing and numeric continuity; they do not establish live-model interpretation
quality or visually validate every outfit. No extra model call, per-frame
network trace or second renderer is needed for this path.

## Module ownership

| Area                                               | Owner                                                                                                                                                                                                                                                                                           |
| -------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Shared limits and semantic IR                      | `contract.ts`, `types.ts`, `semantics.ts`, `shared/merope_rig_contract.json`                                                                                                                                                                                                                    |
| Import orchestration and normalization             | `psdImporter.ts`, `anime25dImporter.ts`, `anime25dImportTypes.ts`                                                                                                                                                                                                                               |
| Expression, collar, atlas and skeleton compilation | `anime25dExpressionCompiler.ts`, `anime25dCollarCompiler.ts`, `anime25dAtlasCompiler.ts`, `anime25dSkeletonCompiler.ts`                                                                                                                                                                         |
| Raster, capability and asset validation            | `anime25dRaster.ts`, `anime25dCapabilities.ts`, `anime25dAssetValidation.ts`, `diagnostics.ts`                                                                                                                                                                                                  |
| Asset transaction                                  | `../assets/pipeline.ts`, `../assets/compiler.ts`                                                                                                                                                                                                                                                |
| Runtime orchestration and performance registry     | `../anime25drig/player.ts`, `../anime25drig/driver.ts`, `../anime25drig/expressionRegistry.ts`, `../events/performanceContract.ts`                                                                                                                                                                     |
| Runtime WebGL, deformation and fallback policy     | `../anime25drig/webglRuntime.ts`, `../anime25drig/mouthRuntime.ts`, `../anime25drig/collarRuntime.ts`, `../anime25drig/atlasUv.ts`, `../anime25drig/layerTransform.ts`, `../anime25drig/layerDeformationPolicy.ts`, `../anime25drig/runtimePolicy.ts`, `../anime25drig/performanceTelemetry.ts` |
| Procedural expression shapes (import and runtime)  | `../expressionShapes/` (eyes, mouths, symbols, face frame)                                                                                                                                                                                                                                      |
| Presentation                                       | `../character/RigCharacter.tsx` (live rig or still portrait)                                                                                                                                                                                                                                    |
| Owner authoring UI                                 | `../workbench/` (settings workbench; depends on all of the above)                                                                                                                                                                                                                               |

## Verification

```sh
cd frontend
pnpm exec tsx --test "src/features/merope/**/*.test.ts" "src/features/merope/*.test.ts"
pnpm exec eslint src/features/merope
pnpm typecheck
```

Focused Anime2.5DRig behavior is documented in
[`ANIME25D_MAINLINE.md`](ANIME25D_MAINLINE.md).

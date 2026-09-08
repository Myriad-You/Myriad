# Anime2.5DRig mainline regression

Source of truth: [`852wa/Anime2.5DRig`](https://github.com/852wa/Anime2.5DRig),
specifically its `README.md`, `lib/rigger.js`, and the WebGL runtime in
`index.html`. Myriad keeps its own Merope shell and asset transaction flow;
only the layered rig behavior is adopted.

The later [`izumix77/Anime2.5DRig`](https://github.com/izumix77/Anime2.5DRig)
fork is used selectively at revision
`1644759cd451ab82065e2bf57b21fe806e2be334`: Myriad adopts the ellipsoid
head/hair shell, authored side-profile curve, separate front/back hair depth,
feathered hairline pin, and the delayed vertical-cylinder projection for
`topwear` / `bottomwear`. Its chest curve and near/far response inform one
garment-aware field derived from Myriad's existing chest profile. The fork's
manual breast/nipple/sternum editors, demo UI, local-storage model, camera, and
recording flow remain outside Myriad.

The upstream project is MIT licensed (Copyright © 2026 hakoniwa); this isolated
profile retains that attribution while integrating with Myriad's existing
compiler, renderer, motion clocks, and Merope UI.

## Mainline gap comparison

| Anime2.5DRig behavior                                                                                                                                                                                                 | Merope before this regression                                                                                                   | Mainline gap                                                                                                                                |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| Accepts a flat see-through PSD with `face` as the only hard requirement                                                                                                                                               | Requires a humanoid body, head, separate upper arms, forearms, hands, thighs, calves, and feet                                  | Add a first-class layered-portrait import path; do not manufacture a 29-bone humanoid                                                       |
| Keeps `face / eyewhite / irides / eyelash / eye_close / eyebrow / mouth_open / mouth_close / nose / ears / earwear / neck / topwear / bottomwear / handwear / headwear / front hair / back hair` as distinct drawings | Collapses face features into two eye parts, clothing into `body`, and accessories into one generic part                         | Preserve source identity and numbered groups, then render with upstream semantic depth and stable PSD-order ties                            |
| Normalizes see-through names, including `mouth` to `mouth_open`, copy suffixes, NFKC text, and numbered hair layers                                                                                                   | Normalizes spelling, but strips numbered layers into one semantic part and treats `mouth_open` as a generic mouth variant       | Match upstream naming while retaining independent numbered hair groups                                                                      |
| Removes low-alpha dust and splits paired eye/eyebrow layers by connected-component centroid around the face center                                                                                                    | Crops at a fixed canvas midpoint and retains low-alpha pixels in the packed atlas                                               | Use content-aware cleanup and side masks anchored to the detected face                                                                      |
| Detects face, eye, iris, mouth, and neck anchors from pixels                                                                                                                                                          | Uses rectangular part centers and humanoid pivots                                                                               | Derive layered-portrait anchors and bind each feature to the correct local pivot                                                            |
| Crossfades open/closed eyelash and mouth drawings while deforming the opening                                                                                                                                         | Presentation slots can crossfade, but the importer discards the separate open-eye stack and requires authored fallback variants | Map `eyelash/eye_close` and `mouth_open/mouth_close` directly to stable slots; synthesize missing close drawings only as a bounded fallback |
| Keeps irises inside eyewhites with stencil clipping                                                                                                                                                                   | Draws all parts with one ordinary alpha pass                                                                                    | Add per-eye stencil masks for `eyewhite` then clip `irides` during the same sorted draw                                                     |
| Uses a depth table plus head-relative parallax/shear for pseudo-3D turns                                                                                                                                              | Rotates/translates a head bone with no layer-depth separation                                                                   | Apply name-based head depth offsets without changing the Merope UI into the upstream demo                                                   |
| Detects up to six hair strands per hair layer and applies a stiff-root/soft-tip double spring                                                                                                                         | Uses one generic secondary spring per imported hair part                                                                        | Build independent root/tip strand chains from each numbered front/back hair layer and feed the existing bounded spring solver               |
| Breath raises/scales `topwear`, the head follows with phase lag, and the chest has a damped bounce                                                                                                                    | Breath is primarily a torso transform and does not know the see-through clothing roles                                          | Add layered topwear/chest weighting and preserve the existing speech/rest damping envelopes                                                 |
| `handwear` may contain separate left/right sleeve, partial-forearm, and hand drawings                                                                                                                                 | Import and regression expect upper-arm/forearm/hand chains and optional relaxed/open-palm/salute/point drawings                 | Attach rigid left/right drawings below one semantic `handwear` parent, clamp final rotation to ±15°, and add no shoulder/elbow/wrist chain  |
| Idle combines small head turns, gaze, breath, blink, mouth motion, and secondary hair                                                                                                                                 | Idle exists, but its visible result depends on the humanoid mapping                                                             | Make the layered portrait path consume the same bounded clocks directly                                                                     |

## Acceptance order

1. PSD naming, cleanup, side split, anchors, and independent layer packing.
2. Blink and mouth crossfades, iris clipping, and speech close timing.
3. Hair root/tip spring response and stable resume at expanded/collapsed cadence.
4. Topwear breathing/chest response and depth-separated head turn.
5. rigid left/right `handwear` follow, quiet idle preview, and ±15° action limit.

Humanoid clip/IK/gesture coverage is **outside Rig IR v4**, not a dormant
alternate profile. Existing assets must pass the v4 migration/diagnostic path
or fall back to their portrait; new imports cannot declare limb chains. In
particular, no work in this mainline adds salute, pointing, open-palm, hand
contact, wrist rotation, locomotion, or gesture constraints.

## Implemented path

### September 2026 upstream correctness fixes

The original runtime reference remains pinned to `d488258`. Import fixes from
`8deb51b7f93984191dfd5805becb349bbe58f90f` are adopted selectively: numbered
semantic layers contribute to shared anchors without merging their artwork;
close-eye synthesis repairs each side independently; narrow hair strands stop
at the minimum spacing; resampling/compositing respects transparent RGB; empty
layers cannot supply invalid anchors. The production importer still requires
visible face pixels even though the standalone reference permits a fallback.
Source/ancestor opacity is baked once into copied import pixels, not multiplied
again during playback. Existing stored assets are not recompiled automatically.

The renderer builds left/right eye masks before painting using independent
stencil bits; collar reconstruction has a separate bit and cannot erase or
satisfy an eye mask. Invisible ordinary eye whites remain valid masks during
expression fades; inactive alternate whites do not contribute stale geometry.
`renderer.test.ts` executes this against a small software stencil buffer across
paint orders and consecutive frames. Import tests check both the standalone
reference and the production atlas path. These tests do not claim GPU pixel or
live visual acceptance. The demo's alternate long-blink policy is not adopted.

The next selective batches add Worker-owned PSD import: signature/RGB8/dimension
checks and metadata bounds precede pixel expansion; buffers are transferred and
workers terminate on success, failure, timeout or cancellation. The existing
32 MB / 2048 px / 64 visible-layer limits remain. Source changes and unmounts
invalidate pending imports, and cancelled preparation never starts preview.
Already-submitted backend previews may finish but their results are discarded;
this does not claim server-side cancellation. `psdImport.worker.ts` now owns the
whole local sequence: decoding, source-reference alignment, cleanup/anchors,
expression and collar compilation, atlas PNGs and the final manifest. It calls
the same compiler, with OffscreenCanvas at the canvas boundary; no duplicate
geometry or packing algorithm is introduced. Only the final manifest and PNGs
return to the page. Relative source URLs resolve against the page, and a small
snapshot of import messages preserves the selected UI language in the worker.
Failed/cancelled imports retain the last successful preflight.

Ordinary blink reopening now drives a bounded 0.52-second iris-only squash and
rebound, inspired by the upstream September curve with smaller amplitude and
an exact identity endpoint. It shares the player/blink clock and invalidates
the eye geometry cache on activation and reset. Special eye drawings are not
deformed by it; special-expression transitions suppress the ordinary rebound.
No new asset fields, director parameters, textures or long-blink policy are added.
Rebound retains the subtle 0.045 scale / 0.025 squash coefficients. Its onset
waits briefly for the filtered eyelid to reopen visibly, with a smooth attack;
hidden or suppressed blinks cannot replay later.

Authored `eye_close2` / `eyeclose2` drawings now import as independent per-eye
layers sharing the existing `closed` slot and `eyeClose` fade. The live player
reads composed eye closure before automatic blinking: deliberate closure selects
the alternate drawing through a short blend, while ordinary blinks use the first
drawing. Each eye checks its own available artwork; missing alternate artwork
leaves the ordinary drawing active. Both drawings share scale/angle deformation
and yield to the same special-expression weights. No alternate artwork is
generated. Existing assets without it keep their current closed-eye drawing;
an authored PSD containing the additional art must be imported to use it.
Tests bundle the production worker for an isolated-thread parser/error check,
exercise termination and late-result rejection, and check rebound bounds,
cache invalidation and geometry isolation without computer use. The standalone
`tests/browser/rigImport.spec.ts` additionally compiles real synthetic PSD bytes
in Chromium's Worker/OffscreenCanvas path and compares manifests and decoded
PNG pixel hashes against the same page-side compiler. Ordinary clothing, a
real high-collar fixture and a necklace fixture must match; packing cancellation,
successful retry and selected-language errors are tested too. This is an import
parity gate, not live GPU animation or acceptance of a particular user's PSD.

- `anime25dImporter.ts` owns import sequencing and normalization. Dedicated
  expression, collar, atlas, skeleton, raster, and validation modules own their
  respective compile stages, so image segmentation no longer shares a module
  boundary with GPU-facing mesh construction.
- `../anime25drig` owns live playback: bind, deform, blink/mouth crossfade,
  depth parallax, hair springs, clothing-aware base/response chest motion, and
  ±15° handwear composition.
- `diagnostics.ts` identifies the profile from `a25d-*` parts and excludes the
  explicitly abandoned articulated gates while retaining all layered-portrait
  quality gates.

### Shell and chest-profile migration

The playback contract is v7 and persists both versioned profiles during import.
Neutral yaw/pitch remains an exact identity, and activation is ramped on player
startup, so enabling the shell does not rewrite or visibly snap the frontal
asset. Preview-to-commit copies the profiles as part of the manifest
transaction, keeping reviewed and activated geometry identical. The torso
cylinder reuses the existing face width and neck pivot, follows head/body yaw
through a low-pass response, and applies fully to `topwear` / `bottomwear`.
A split high collar reuses its alpha contour only to place the front mesh and
neck stencil. Front collar, rear collar, and stencil then share one row-coherent
vertical field: the upper edge follows the neck/head, the lower edge follows
the body, and torso-shell weight is the exact inverse of neck-follow. No
left/right attachment split or MLS fit remains, so narrow lace, trim, and bow
artwork cannot shear or invert while both seams stay on the same motion field.
The alpha-derived neck stencil closes at the first opaque center row. That same
stencil mesh carries the neck UVs and inverse torso-shell offset, so the
aperture and its fill cannot diverge during yaw.

Authored shell profiles retain the fork's per-model rectangular hairline pin.
Anchor-derived profiles do not inherit its demo rectangle: they build a smooth
attachment field from the imported strand roots, with a release spanning at
least two hair-mesh rows. This keeps the crown attached to the head shell
without clamping a horizontal band of bangs or introducing a one-cell spring
discontinuity. Hair-layer bounds gently calibrate the scalp ellipsoid; crown
wrap remains zero unless at least four sufficiently distributed strand roots
confirm that the upper layer is scalp hair rather than an ornament.

The v2 chest profile remains the only persisted chest contract. Dynamic
topwear response and yaw-projected volume sample one asymmetric upper/peak/lower
field, including the same geometry weights. Existing `supportScale` and
`garmentMotionScale` derive the central bridge, near/far depth, silhouette,
damping, and bounded breathing transmission. No nipple positions or manual
curve points are added, so existing v7 assets require no migration.

The importer regression fixture proves that See-through-style layers compile
blink, mouth, gaze, chest, hair-strand, depth-turn, and rigid side-handwear paths
without creating shoulder, elbow, wrist, leg, foot, contact, or hand-pose chains. Real PSD
acceptance still requires a generated-material preflight and rendered motion
review; the synthetic fixture is only an architectural gate.

Merope additionally compiles `dizzy`, `squeeze`, and `cry` per-eye presentation
variants. These are Myriad extensions rather than upstream Anime2.5DRig
features: authored `eye_dizzy` / `eye_squeeze` / `eye_cry` layers win,
otherwise the importer generates independent character-tinted spiral,
inward-chevron, or asymmetric chevron-and-tear artwork at the detected
left/right eye anchors. Contract v13 also preserves a plain See-through `mouth`
as the closed portrait drawing and generates separate flat `mouth_open`,
`mouth_wide`, `mouth_round`, `mouth_narrow`, and `mouth_cry` variants from its
bounds and dark-line palette. Runtime speech morphs every ordinary mouth mesh
through one continuous width/open/roundness envelope. A hysteretic state
machine keeps exactly one ordinary mouth texture visible and switches it only
after the next shape is decisively dominant. Crying replaces the ordinary
mouth stack. These
variants have no old-rig runtime fallback; reimport is required.

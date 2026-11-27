# Third-party models

This document lists the open model weights in Myriad's character layering path
only, and the pose model the backend runs on the same portraits, with the
licences their publishers state. Myriad ships none of these weights. Hosted
image-generation and 3D services that the figure pipeline calls have their own
terms and are not covered here.

## Character layering (See-through)

Character layering splits a portrait into layers and estimates their depth. The
backend does not load these models: it sends the portrait to a See-through Space
on Hugging Face (`backend/src/services/see_through.rs`; `DEFAULT_SPACE` unless
the site owner names another Space), and the models run there. Read from the
default Space's code at commit
[`13bb810`](https://huggingface.co/spaces/SomekawaHitomi/see-through-demo/tree/13bb810e731d26217513dd88a0fb71af0194cb5d),
it loads at runtime:

| Model | What the Space loads, and when | Role |
| --- | --- | --- |
| [See-through LayerDiff 3D](https://huggingface.co/layerdifforg/seethroughv0.0.2_layerdiff3d) | The whole repository (`unet`, `trans_vae`, `vae`, `text_encoder`, `text_encoder_2`, both tokenizers, `scheduler`) at start-up (`REPO_LAYERDIFF` in `app.py`) | Layer decomposition |
| [See-through Marigold Depth](https://huggingface.co/layerdifforg/seethroughv0.0.1_marigold) | The whole repository at start-up (`REPO_DEPTH` in `app.py`, which names it `24yearsold/seethroughv0.0.1_marigold`; Hugging Face redirects that name here) | Depth of each layer |
| [Marigold Depth v1.1](https://huggingface.co/prs-eth/marigold-depth-v1-1) `text_encoder` and `tokenizer` | On the first depth pass, to encode the empty prompt (`encode_empty_text` in `common/modules/marigold/marigold_depth_pipeline.py`); the result is cached in the Space's `workspace/` | Empty-prompt embedding for the depth model |
| [AnimeMangaInpainting](https://huggingface.co/dreMaz/AnimeMangaInpainting) `lama_large_512px.ckpt` | During decomposition on a GPU, to fill the hidden parts of layers (`load_lama_mpe` in `annotators/lama_inpainter/__init__.py`, called from `_inpaint` in `common/utils/inference_utils.py`); also by the `refine` endpoint | Inpainting |
| [Real-ESRGAN](https://github.com/xinntao/Real-ESRGAN) `RealESRGAN_x4plus_anime_6B.pth` | When the `upscale` endpoint is called (`upscale.py`), which the backend does to enlarge a full-body picture before layering | Super-resolution |

The Space's code also contains loaders for other weights (Segment Anything,
TinyViT, Hyper-SD LoRAs, Animagine XL 4.0, Qwen-Image-Layered) in research
scripts under `inference/scripts/` and in `common/modules/sam/`; none of them is
reached from `app.py`.

### Licences

| Model | Licence as stated by its publisher | Licence text |
| --- | --- | --- |
| See-through LayerDiff 3D | Its `LICENSE` names four parts: Part A, Apache-2.0 for the See-through Authors' own contributions; Part B, CreativeML Open RAIL++-M (a Derivative of Animagine XL 4.0, fine-tuned from Stable Diffusion XL 1.0); Part C, CreativeML Open RAIL-M (weights derived from LayerDiffuse); Part D, MIT (the VAE, SDXL-VAE-FP16-Fix). It states that the use-based restrictions of Parts B and C apply to every use. Model card: `license: openrail++`. | [`LICENSES/LicenseRef-See-through-LayerDiff3D-weights.txt`](../../LICENSES/LicenseRef-See-through-LayerDiff3D-weights.txt) |
| See-through Marigold Depth | Its `LICENSE` names three parts: Part A, Apache-2.0 for the See-through Authors' own contributions; Part B, the Open RAIL++-M License as published by ETH Zürich (Marigold Depth v1.1), whose Attachment A is an unfilled template; Part C, the CreativeML Open RAIL++-M License of Stable Diffusion 2. It states that the use-based restrictions of Part C apply to every use. Model card: `license: openrail++`. | [`LICENSES/LicenseRef-See-through-Marigold-weights.txt`](../../LICENSES/LicenseRef-See-through-Marigold-weights.txt) |
| Marigold Depth v1.1 (text encoder and tokenizer) | Model card: `license: openrail++`, linking the CreativeML Open RAIL++-M License of Stable Diffusion 2. The repository has no `LICENSE` file. | Not copied; the CreativeML Open RAIL++-M text of Stable Diffusion 2 is reproduced as Part C of the See-through Marigold licence |
| AnimeMangaInpainting `lama_large_512px.ckpt` | Model card: `license: mit`. The card describes it as [big LaMa](https://github.com/advimman/lama) (code Apache-2.0) fine-tuned on "300k manga and anime style data"; it does not say where that data came from or under what terms. Big LaMa itself was trained on Places2. The repository has no `LICENSE` file. | Not copied |
| Real-ESRGAN `RealESRGAN_x4plus_anime_6B` | BSD-3-Clause (the licence of the Real-ESRGAN repository, which publishes the weights in its releases). The repository does not say what data the anime model was trained on. | Not copied |

See [MODEL_USE_RESTRICTIONS.md](MODEL_USE_RESTRICTIONS.md) for the use-based
restrictions.

The models the See-through weights derive from:

| Model | Licence as stated by its publisher |
| --- | --- |
| [Animagine XL 4.0](https://huggingface.co/cagliostrolab/animagine-xl-4.0) (Cagliostro Labs) | CreativeML Open RAIL++-M |
| [Stable Diffusion XL base 1.0](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0) (Stability AI) | CreativeML Open RAIL++-M, [`LICENSES/LicenseRef-CreativeML-OpenRAIL-PlusPlus-M.txt`](../../LICENSES/LicenseRef-CreativeML-OpenRAIL-PlusPlus-M.txt) |
| [LayerDiffuse](https://huggingface.co/lllyasviel/LayerDiffuse_Diffusers) (Lvmin Zhang) | The model card declares `license: openrail`; the See-through LayerDiff 3D licence names it CreativeML Open RAIL-M, [`LICENSES/CreativeML-OpenRAIL-M.txt`](../../LICENSES/CreativeML-OpenRAIL-M.txt) |
| [SDXL-VAE-FP16-Fix](https://huggingface.co/madebyollin/sdxl-vae-fp16-fix) (madebyollin) | MIT |
| [Marigold Depth v1.1](https://huggingface.co/prs-eth/marigold-depth-v1-1) (PRS, ETH Zürich) | Open RAIL++-M |
| Stable Diffusion 2 (Stability AI) | CreativeML Open RAIL++-M |

The See-through code ([shitagaki-lab/see-through](https://github.com/shitagaki-lab/see-through))
is licensed under Apache-2.0.

## Pose estimation

| Model | Where it runs | Licence |
| --- | --- | --- |
| [DWPose](https://huggingface.co/yzd-v/DWPose) `dw-ll_ucoco_384` | The Myriad backend, to find joints on the portrait (`backend/src/services/pose_estimation.rs`) | Apache-2.0 (as tagged by its publisher). The weights were trained on whole-body keypoint data (COCO-WholeBody and UBody) whose annotations are licensed for research and non-commercial use by their owner. The tag describes the weights' stated licence only. |

## Licence text sources

| File | Source |
| --- | --- |
| `LICENSES/LicenseRef-CreativeML-OpenRAIL-PlusPlus-M.txt` | [`LICENSE.md`](https://huggingface.co/stabilityai/stable-diffusion-xl-base-1.0/blob/462165984030d82259a11f4367a4eed129e94a7b/LICENSE.md) of Stable Diffusion XL base 1.0 |
| `LICENSES/CreativeML-OpenRAIL-M.txt` | [`LICENSE`](https://github.com/CompVis/stable-diffusion/blob/69ae4b35e0a0f6ee1af8bb9a5d0016ccb27e36dc/LICENSE) of CompVis/stable-diffusion |
| `LICENSES/LicenseRef-See-through-LayerDiff3D-weights.txt` | [`LICENSE`](https://huggingface.co/layerdifforg/seethroughv0.0.2_layerdiff3d/blob/4477e6ce529bc6a141732e1a55a4932176db3b89/LICENSE) of See-through LayerDiff 3D |
| `LICENSES/LicenseRef-See-through-Marigold-weights.txt` | [`LICENSE`](https://huggingface.co/layerdifforg/seethroughv0.0.1_marigold/blob/4f4ffc46050b6feb764b628859968a26e76e6b6a/LICENSE) of See-through Marigold Depth |

Each file is the source byte for byte.

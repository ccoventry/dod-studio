# HD asset build scripts

These scripts make the upscaled files that `goldsrc-hooks`' HD texture hook (`src/texture_hires.rs`) swaps in while the game loads: map textures, model skins, world sprites, detail textures and skyboxes. Nothing in the game's own files is changed. Everything goes into one folder, `<game>\dod\dodstudio_hd\`, which you can delete to go back to stock.

The hook only runs with `GOLDSRC_HOOKS_TEXTURE_HIRES=1` set; see the crate README.

## What you need

1. **Python 3.11 or newer**, plus three packages:

   ```
   pip install -r requirements.txt
   ```

2. **Real-ESRGAN ncnn-vulkan** (the upscaler). Download `realesrgan-ncnn-vulkan-20220424-windows.zip` from the [Real-ESRGAN releases page](https://github.com/xinntao/Real-ESRGAN/releases/tag/v0.2.5.0) and unzip it into a `realesrgan` folder next to these scripts, so that `realesrgan\realesrgan-ncnn-vulkan.exe` exists. Or unzip it anywhere and set `REALESRGAN` to the `.exe`. It needs a GPU with Vulkan support, which any recent NVIDIA, AMD or Intel card has.

3. **The extra upscaling models**, for every style except `x4plus` and `plain`: put each `.param` + `.bin` pair into `realesrgan\models\`.

   | Style | Files | Where |
   |---|---|---|
   | `ultrasharp` (default) | `ultrasharp-4x` | [Upscayl](https://github.com/upscayl/upscayl), `resources/models` |
   | `remacri` | `remacri-4x` | Upscayl, `resources/models` |
   | `siax` | `4x_NMKD-Siax_200k` | [Upscayl custom models](https://github.com/upscayl/custom-models), `models` |
   | `generalv3` | `RealESRGAN_General_x4_v3` | Upscayl custom models, `models` |
   | `x4plus` | `realesrgan-x4plus` | already in the Real-ESRGAN zip |

   These models aren't included in this repo on purpose: several carry their own licences (4x-UltraSharp, for one, is non-commercial), which differ from this repo's MIT licence. Check each one's licence before you share what you make with it.

`plain` needs none of the above except Python: it's a plain enlargement with light sharpening, no AI.

## Which game folder

The scripts write into the Half-Life folder that DoD Studio launches. They look for it in this order:

1. `HD_GAME`, or `--game` for `build_all.py`
2. the `hl.exe` DoD Studio is set to launch
3. the only Steam install with a `dod` folder (if you have several, you'll be asked to pick one)

## Build everything

```
python build_all.py                       all 7 styles, all types
python build_all.py ultrasharp            one style
python build_all.py --types sprites,sky   only some types
```

- **Resumable.** Files that already exist are skipped, so re-running after adding maps or models only builds what's new. Progress goes to `dodstudio_hd\build_all.log`.
- **Time and space.** For about 220 maps with all their models, sprites, details and skies: roughly 12 GB and 30–60 minutes per style on a mid-range GPU. `plain` and `blend` take a few minutes.
- **`blend`** is made from `x4plus` and `plain` (50/50), so build those first. The default order does.
- **`--also <another Half-Life folder>`** also builds that install's models and sprites. Use it if you keep a stock install next to a modded one: its versions get HD copies too, so they're ready if you ever copy them over. Files are matched by their pixels, so both versions can share one folder.
- **`--extra-models <folder>`** adds any other folder of `.mdl` files.

## Upscale your own files

Each type's script takes an output folder and what to build. Put the output in the style you use, or in `overrides`, which wins over any style:

```
set OUT=C:\...\Half-Life\dod\dodstudio_hd

python world_hd.py   %OUT%\world\ultrasharp   dod_mymap dod_othermap
python models_hd.py  %OUT%\models\ultrasharp  C:\path\to\v_mycustomgun.mdl
python models_hd.py  %OUT%\models\ultrasharp  C:\path\to\a\folder\of\models
python sprites_hd.py %OUT%\sprites\ultrasharp C:\path\to\mysprite.spr
python detail_hd.py  %OUT%\detail\ultrasharp  C:\path\to\folder\of\detail\tgas
python sky_hd.py     %OUT%\sky\ultrasharp     dod_mymap
```

Set `HD_STYLE` to build a style other than `ultrasharp` (for example `set HD_STYLE=remacri`). Run any script with no arguments for its usage.

**Replacing one texture by hand.** Name it the way the hook expects and drop it into `overrides`:

- **Map textures:** `<name>_<hash>.tga`
- **Model skins:** `<skin name>_<hash>.tga`
- **Sprites:** `<sprite>_<frame>_<hash>.tga`
- **Detail textures and skies:** the same names as in `gfx\detail` and `gfx\env`

The easiest way to get the right name is to build the texture once with any style and copy the file name. In game, `dodstudio_hd_misses` also names the file a texture would need.

## Picking a style

```
python compare.py compare.png
```

This makes one sheet with the original and every built style side by side, for a few sample textures, model skins, sprites, detail textures and a sky. Pass your own samples to compare something specific; run it with no arguments for the format.

In game, `dodstudio_hd_style <name>` in `movie.cfg` picks the style. It's read once per game session.

## Files

| File | What it does |
|---|---|
| `build_all.py` | runs everything below, for each style |
| `world_hd.py` | map textures (embedded in the BSP or from its wads) |
| `models_hd.py` | model skins |
| `sprites_hd.py` | world sprite frames (HUD sprites are never replaced, so they're skipped) |
| `detail_hd.py` | detail textures |
| `sky_hd.py` | skybox faces |
| `compare.py` | the style comparison sheet |
| `styles.py` | the style list and the upscaler call |
| `goldsrc.py` | BSP, WAD, model and sprite readers |
| `hdcommon.py` | game folder lookup, and the hashing and naming the hook matches |
| `valve_models.txt` | Half-Life models DoD borrows (breakable-object gibs, `gordon`, `skeleton`) |

The file name hash is FNV-1a over the original's 8-bit pixels and its 768-byte palette. It must stay byte-for-byte identical to `texture_hires.rs`, or the hook won't find the files.

# HD asset build scripts

These scripts make the upscaled files that `goldsrc-hooks`' HD texture hook (`src/texture_hires.rs`) swaps in while the game loads: map textures, model skins, world sprites, detail textures and skyboxes. Nothing in the game's own files is changed. Everything goes into one folder, `<game>\dod\dodstudio_hd\`, which you can delete to go back to stock.

The hook turns itself on when it finds a `dodstudio_hd` folder; `dodstudio_hd_enabled 0` in the game console (or `movie.cfg`) turns it off.

**DoD Studio can run these scripts for you:** its HD Textures page downloads the upscaler, uses your own Python if it has NumPy, Pillow and SciPy (or fetches a private copy for DoD Studio's own use), and has a Build button with progress and Cancel. It runs exactly these scripts, so both routes make the same files, and a build started in one carries on in the other. The steps below are for the command line.

## Step by step (no coding needed)

You'll type a few commands into a Command Prompt window. Copy each line, paste it in (right-click pastes), and press Enter.

**1. Install Python (once).** Download it from [python.org/downloads](https://www.python.org/downloads/) and run the installer. On its first screen, tick **"Add python.exe to PATH"** before clicking Install.

**2. Open a Command Prompt in this folder.** In File Explorer, open this folder (`goldsrc-hooks\tools\hd` inside the DoD Studio download). Click the address bar at the top, type `cmd` and press Enter. A black window opens, already in the right folder.

**3. Install what the scripts use (once).** Paste these two lines, one at a time:

```
pip install -r requirements.txt
python setup_tools.py
```

The first installs three Python packages. The second downloads the upscaler (about 45 MB) and the style models into a `realesrgan` folder here. If either prints an error, see "What you need" below.

**4. Build the HD files.** For the default style:

```
python build_all.py ultrasharp
```

This finds your DoD install by itself (the one DoD Studio launches) and fills `dod\dodstudio_hd`. It can take an hour for a large map collection. It keeps going if you leave the PC; if it stops or you close the window, run the same line again and it carries on where it left off.

**5. Start the game from DoD Studio.** That's it: DoD Studio's hook DLL sees the new `dodstudio_hd` folder and turns HD on by itself. In game, `dodstudio_debug_status` in the console shows how many textures were replaced.

To switch it off or on, use the console or a line in `movie.cfg`:

```
dodstudio_hd_enabled 0
dodstudio_hd_enabled 1
```

A change applies to what loads next: walls, detail textures and skies from the next demo, models and sprites already loaded after a game restart. Put it in `movie.cfg` to have it from the start.

### More examples

**Build another style too** (then pick it with `dodstudio_hd_style remacri` in `movie.cfg`):

```
python build_all.py remacri
```

**Build all seven styles** (roughly 7x the time and disk space):

```
python build_all.py
```

**Only rebuild one kind of file**, e.g. after adding new sprites:

```
python build_all.py --types sprites ultrasharp
```

**Your DoD folder wasn't found**, or you have more than one Half-Life install. Say which one (the folder that contains `hl.exe`; keep the quotes):

```
python build_all.py --game "C:\Program Files (x86)\Steam\steamapps\common\Half-Life" ultrasharp
```

**You downloaded a new custom map** called `dod_mymap`:

```
python world_hd.py "C:\Program Files (x86)\Steam\steamapps\common\Half-Life\dod\dodstudio_hd\world\ultrasharp" dod_mymap
python sky_hd.py   "C:\Program Files (x86)\Steam\steamapps\common\Half-Life\dod\dodstudio_hd\sky\ultrasharp" dod_mymap
```

(`python build_all.py ultrasharp` does the same, for every map that's new.)

**You have a custom weapon model or sprite** somewhere on your PC:

```
python models_hd.py  "C:\Program Files (x86)\Steam\steamapps\common\Half-Life\dod\dodstudio_hd\models\ultrasharp" "C:\Users\me\Downloads\v_garand.mdl"
python sprites_hd.py "C:\Program Files (x86)\Steam\steamapps\common\Half-Life\dod\dodstudio_hd\sprites\ultrasharp" "C:\Users\me\Downloads\muzzleflash1.spr"
```

**See the styles side by side** before choosing one; this writes `compare.png` here, which you can open:

```
python compare.py compare.png
```

**Go back to stock:** put `dodstudio_hd_enabled 0` in `movie.cfg`, or delete the `dod\dodstudio_hd` folder.

## What you need

1. **Python 3.11 or newer**, plus three packages: `pip install -r requirements.txt`.

2. **Real-ESRGAN ncnn-vulkan** (the upscaler) and **the extra style models**. `python setup_tools.py` downloads both into a `realesrgan` folder here. DoD Studio's **HD Textures** page can download the same files into its own folder instead; the page shows the `set REALESRGAN=...` line that points these scripts at that copy. To do it by hand instead:
   - Unzip `realesrgan-ncnn-vulkan-20220424-windows.zip` from the [Real-ESRGAN releases page](https://github.com/xinntao/Real-ESRGAN/releases/tag/v0.2.5.0) into `realesrgan\`, so that `realesrgan\realesrgan-ncnn-vulkan.exe` exists. Or unzip it anywhere and set `REALESRGAN` to the `.exe`.
   - Put each model's `.param` + `.bin` pair into `realesrgan\models\`.

   It needs a GPU with Vulkan support, which any recent NVIDIA, AMD or Intel card has.

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

Build into your **movie copy** of Half-Life, the one DoD Studio launches. The HD files are plain images and harmless on their own, but they only show up when DoD Studio's hook DLL is loaded into the game, and that DLL must never be loaded into the copy you play online with. See [`docs/vac_safety.md`](../../../docs/vac_safety.md).

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

## Only the maps you use

Map textures and skies are the bulk of the build, and a public-server map you never film is wasted time and disk. To build only some maps:

1. Copy `hd_maps.example.txt` to `hd_maps.txt` in the game's `dod\dodstudio_hd` folder (e.g. `...\Half-Life\dod\dodstudio_hd\hd_maps.txt`), next to the files it decides about.
2. List the maps, one per line. `*` matches anything and `?` one character, like a Windows folder search: `dod_railroad2*` covers every railroad2 build. A name without them matches only that map.

`build_all.py` then builds map textures and skies for those maps only, and says how many it picked at the top of `build_all.log`. A line that matches no map is reported rather than ignored. Model skins, sprites and detail textures aren't tied to a map, so they're always built in full. Delete `hd_maps.txt` to build every map again.

A map left out simply shows its original textures in game, and `dodstudio_debug_hd_misses` lists it.

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

The easiest way to get the right name is to build the texture once with any style and copy the file name. In game, `dodstudio_debug_hd_misses` also names the file a texture would need.

## Picking a style

```
python compare.py compare.png
```

This makes one sheet with the original and every built style side by side, for a few sample textures, model skins, sprites, detail textures and a sky. Pass your own samples to compare something specific; run it with no arguments for the format.

To see your own maps instead of the fixed samples, `--map dod_anzio` takes that map's most detailed textures that have an HD file, plus its sky, and `--auto` picks a few of your maps for you. `--styles ultrasharp,plain` keeps the sheet to the styles you're choosing between. DoD Studio's HD Textures page makes the same sheet under **Compare styles**.

In game, `dodstudio_hd_style <name>` in `movie.cfg` picks the style. It's read once per game session.

## Make your own style

The game loads whatever style folder you name, so you can make as many as you like without touching any code:

DoD Studio's HD Textures page has a form for this (Your own styles) that writes the same file. By hand:

1. Copy `my_styles.example.txt` to `my_styles.txt` in the game's `dod\dodstudio_hd` folder and open it in Notepad.
2. Add one line per style. There are three kinds:

   ```
   anime   = realesrgan-x4plus-anime        an AI model (file name in realesrgan\models, no extension)
   crisp   = plain 150                      no AI, sharpened 0 (none) to 500 (very strong); plain is 60
   sharp70 = blend ultrasharp plain 70      two styles you've built, mixed: 70% ultrasharp, 30% plain
   ```

3. Build it and try it:

   ```
   python build_all.py crisp
   ```

   Then put `dodstudio_hd_style crisp` in `movie.cfg` and restart the game. `python compare.py compare.png` includes your styles too.

**Other AI models.** Anything in ncnn format works (a `.param` + `.bin` pair): drop the two files into `realesrgan\models\` and name the file in a line. [Upscayl's custom models](https://github.com/upscayl/custom-models) has dozens ready to use. Models from [OpenModelDB](https://openmodeldb.info/) come as `.pth` or `.safetensors`, and [chaiNNer](https://chainner.app/) can convert them to ncnn. Use 4x models: the scripts expect 4x.

**Blends** take no GPU time: they mix files that already exist, so build both of their styles first. `build_all.py` always builds blends last, so `python build_all.py ultrasharp plain sharp70` works in one go.

Style names are lowercase letters, digits, `-` and `_`, because they become folder names.

## Where your lists live

`hd_maps.txt` and `my_styles.txt` belong to one game install, so they live in its `dod\dodstudio_hd` folder rather than here. A second install (a stock one next to a movie one) keeps its own, and updating or re-downloading DoD Studio never touches them. Deleting `dodstudio_hd` deletes them too. `HD_MAPS` and `HD_MY_STYLES` point at a file anywhere else.

Lists saved in this folder by an older version still work while the install has none of its own: the build says so at the top of `build_all.log`, with the path to move each one to.

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
| `setup_tools.py` | downloads the upscaler and the style models |
| `styles.py` | the style list and the upscaler call |
| `my_styles.example.txt` | template for your own styles (copy it to `dodstudio_hd\my_styles.txt`) |
| `hd_maps.example.txt` | template for building only some maps (copy it to `dodstudio_hd\hd_maps.txt`) |
| `goldsrc.py` | BSP, WAD, model and sprite readers |
| `hdcommon.py` | game folder lookup, and the hashing and naming the hook matches |
| `valve_models.txt` | Half-Life models DoD borrows (breakable-object gibs, `gordon`, `skeleton`) |

The file name hash is FNV-1a over the original's 8-bit pixels and its 768-byte palette. It must stay byte-for-byte identical to `texture_hires.rs`, or the hook won't find the files.

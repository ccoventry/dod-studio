// strings/hd.js: the app's text for the HD Textures page.
// Part of the STRINGS catalog that ../strings.js assembles (#604).

export const HD_STRINGS = {
  // ── hd_pane.js: the HD Textures page (#372) ─────────────────────────────
  HD: {
    // #430: whether hl.exe gets 2 GB or 4 GB of address space.
    ADDRESS_SPACE_4GB: 'This hl.exe gets 4 GB of memory, room for the biggest HD textures.',
    ADDRESS_SPACE_2GB: 'This hl.exe gets 2 GB of memory (the pre-Anniversary build isn\'t marked for more), so very large HD textures or a long session over many maps can run it out.',
    STATUS_TITLE: "What's built",
    REFRESH_BUTTON: 'Refresh',
    REFRESHING: 'Checking...',
    checkedAt: (time) => `Checked at ${time}.`,
    TABLE_STYLE: 'Style',
    USE_TITLE: 'Use it in the game',
    USE_HINT: "HD turns itself on when the game finds the dodstudio_hd folder. Put these lines in movie.cfg to pick the style from the first map. The gl_max_size line is the size the game shows textures at: the game's own default is 256, which would shrink every HD file back down.",
    STYLE_LABEL: 'Style:',
    COPY_BUTTON: 'Copy',
    SETUP_TITLE: 'Tools',
    LICENCE_NOTE: "Download fetches Real-ESRGAN ncnn-vulkan from its GitHub releases and four style models from the Upscayl project, about 180 MB in all, into DoD Studio's own folder. When this PC has no Python the build can use, it also fetches one for DoD Studio's own use (Python 3.12 with NumPy, Pillow and SciPy, about 70 MB), without installing anything system-wide. The models have their own licences, which differ from DoD Studio's: 4x-UltraSharp, for one, is non-commercial. Check each one's licence before you share what you make with it.",
    SETUP_BUTTON: "Download what's missing",
    NOTHING_MISSING_BUTTON: 'Nothing to download',
    CANCEL_BUTTON: 'Cancel',
    BUILD_TITLE: 'Build',
    BUILD_HINT: 'Pick the styles and kinds of files to build. Files already built are skipped, so a stopped build carries on where it left off. Map textures take the longest: a few minutes per style for a few dozen maps, and an hour or more for every map in a large collection.',
    BUILD_STYLES_LABEL: 'Styles:',
    BUILD_TYPES_LABEL: 'Files:',
    BUILD_CAP_LABEL: 'Largest size:',
    CAP_NAMES: { 1024: '1024 (the usual)', 2048: '2048', 4096: '4096' },
    // Each cap's cost, since the number alone says nothing about it.
    CAP_HINTS: {
      1024: 'Every file is 4x its original, up to 1024 a side. Most map textures are 128 or 256 to begin with, so they end at 512 or 1024 either way.',
      2048: 'Only files whose 4x is bigger than 1024 change: detail textures (most are 512, so they go from 1024 to 2048), a few large map textures and model skins. A 2048 file is 16 MB, four times a 1024 one, and the game is 32-bit: try one map before building everything.',
      4096: 'Only files whose 4x is bigger than 2048 change, and almost none are: nothing in the game is over 512 a side except a few skies. This is here for files you upscale yourself. A 4096 file is 64 MB.',
    },
    BUILD_BUTTON: 'Build',
    SCRIPTS_SUMMARY: 'Build from a Command Prompt instead',
    SCRIPTS_HINT: 'The same scripts run from goldsrc-hooks\\tools\\hd (their README has the steps). To make them use the upscaler downloaded here, run this first in the same Command Prompt:',
    PYTHON_PICK_BUTTON: 'Choose python.exe...',
    PYTHON_RESET_BUTTON: 'Find it automatically',
    PYTHON_PICK_TITLE: 'Choose python.exe',
    // Where the Python the build would use came from.
    pythonUsing: (source, exe, version) => ({
      chosen: `Python: ${version}, the one you chose (${exe}).`,
      app: `Python: ${version}, DoD Studio's own copy.`,
      found: `Python: ${version}, found on this PC (${exe}).`,
    })[source],
    pythonChosenProblem: (exe, version, missing) => version
      ? `The Python you chose (${exe}, ${version}) ${missing.length ? `has no ${missing.join(', ')}` : 'is older than 3.10'}, so it isn't used.`
      : `The Python you chose (${exe}) didn't run, so it isn't used.`,
    pythonFoundUnusable: (exe, version, missing) => `Python ${version} is installed (${exe}) but ${missing.length ? `has no ${missing.join(', ')}` : 'is older than 3.10'}. Install ${missing.length ? 'them' : 'Python 3.10 or newer'} there (pip install numpy pillow scipy), choose another python.exe, or let Download fetch DoD Studio's own copy.`,
    PYTHON_NONE: "Python: none found. Download fetches DoD Studio's own copy, or choose a python.exe with NumPy, Pillow and SciPy.",
    NO_SCRIPTS: 'This copy of DoD Studio has no build scripts, so it can only show what is built.',
    buildStep: (step, steps, style, type, elapsed) => `Step ${step} of ${steps}: ${style}, ${type} (${elapsed} so far)`,
    buildDone: (steps, elapsed, log) => `Done: ${steps} step${steps === 1 ? '' : 's'} in ${elapsed}. Every step's counts are in ${log}.`,
    BUILD_CANCELLED: 'Stopped. Files already built are kept; Build again to carry on.',
    BUILD_STARTING: 'Starting the build…',
    STYLE_NEEDS_UPSCALER: ' (needs Download)',
    // Asset types, as the hook's folder names.
    TYPE_NAMES: { world: 'Map textures', models: 'Model skins', sprites: 'Sprites', decals: 'Decals', detail: 'Detail textures', sky: 'Skies' },
    NOTHING_BUILT: 'Nothing yet',
    cellSummary: (files, size) => `${files.toLocaleString()} files, ${size}`,
    // #426: how much of the game a style covers.
    cellSummaryOf: (files, most, size) => `${files.toLocaleString()} of ${most.toLocaleString()} files, ${size}`,
    CELL_OF_TITLE: 'Fewer files than the fullest style has for this type. Wherever this style has none, the game shows the stock texture.',
    largestSize: (width, height) => `up to ${width}×${height}`,
    LARGEST_SIZE_TITLE: "The size of this style's biggest file of this type.",
    TYPE_NAMES_LOWER: { world: 'map textures', models: 'model skins', sprites: 'sprites', decals: 'decals', detail: 'detail textures', sky: 'skies' },
    someOf: (files, most, type) => `${files} of ${most} ${type}`,
    styleGaps: (style, none, some) => {
      const list = (items, word) => (items.length < 2 ? items.join('') : `${items.slice(0, -1).join(', ')} ${word} ${items[items.length - 1]}`);
      const parts = [];
      if (none.length) parts.push(`no ${list(none, 'or')}`);
      if (some.length) parts.push(`only ${list(some, 'and')}`);
      return `${style} covers only part of the game: it has ${parts.join(', and ')}. Wherever it has none, the game shows the stock texture.`;
    },
    cfgGapsComment: (sentence) => `// ${sentence}`,
    // #426: which maps a style has map textures for, and the warning when
    // the maps being built include one it hasn't.
    COVERAGE_CHECKING: 'Reading the maps…',
    coverageChecking: (done, total) => `Reading the maps: ${done} of ${total}`,
    coverageFailed: (err) => `Could not check which maps are covered: ${err}`,
    coverageAll: (style, maps, listed) => `${style} has every map texture for ${maps === 1 ? 'the 1 map' : `all ${maps} maps`}${listed ? ' in your map list' : ''}.`,
    coverageCounts: (style, full, part, none, listed) => {
      const maps = (n) => `${n} map${n === 1 ? '' : 's'}`;
      const parts = [];
      if (full) parts.push(`every map texture for ${maps(full)}`);
      if (part) parts.push(`${parts.length ? 'some' : 'some of the map textures'} for ${maps(part)}`);
      if (none) parts.push(`${parts.length ? 'none' : 'no map textures'} for ${maps(none)}`);
      const last = parts.pop();
      return `${style}${listed ? ', in your map list' : ''}: ${parts.length ? `${parts.join(', ')} and ${last}` : last}.`;
    },
    COVERAGE_SHOW: 'Click to see which maps',
    COVERAGE_SHOW_EVERY_MAP: 'Show every map, not only the ones in your map list',
    coverageGroupFull: (n) => `Every map texture (${n})`,
    coverageGroupPart: (n) => `Some of the map textures (${n})`,
    coverageGroupNone: (n) => `No map textures (${n})`,
    coveragePartMap: (map, built, total) => `${map} (${built} of ${total})`,
    coverageUnreadable: (n) => `${n} map file${n === 1 ? '' : 's'} could not be read.`,
    mapListWarning: (style, kind, first, more, listed) => {
      const which = `${first}${more ? ` and ${more} more map${more === 1 ? '' : 's'}` : ''}${listed ? ' in your map list' : ''}`;
      return {
        none: `${style} has no map textures for ${which}; those maps will show stock textures.`,
        some: `${style} has only some of the map textures for ${which}; those maps will show stock textures wherever it has none.`,
        missing: `${style} is missing map textures for ${which}; those maps will show stock textures wherever it has none.`,
      }[kind];
    },
    // #680: the gl_max_size the selected install runs with (native::hd::gl_max_size).
    glMaxSizeSource: (source) => ({
      initial_commands: "Studio's Initial Commands",
      config: `${source?.file} line ${source?.line}`,
      engine_default: "nothing sets it, so it's the game's default",
    })[source?.kind] || '',
    glMaxSizeOk: (value, source, px) => `This install runs with gl_max_size ${value} (${source}): HD textures show at up to ${px} px.`,
    glMaxSizeLow: (value, source, px, cap, fix) => `This install runs with gl_max_size ${value} (${source}), so HD textures show at ${px} px at most, smaller than the ${cap} they are built at. ${fix}`,
    GL_MAX_SIZE_FIX_CFG: 'Copy the gl_max_size line above into movie.cfg, then restart the game.',
    GL_MAX_SIZE_FIX_INITIAL: 'Raise it in Initial Commands (Configuration), or take that line out and set it in movie.cfg.',
    // #1155: a style built with an older version of the app's scripts (native::hd::outdated).
    outdatedSteps: { killfeed_icons: 'the new kill-feed icons (about 5 minutes)' },
    outdatedStyle: (style, what) => `${style} was built with an older version: build it again to get ${what}.`,
    hdRootFound: (path) => `HD folder: ${path}`,
    hdRootMissing: (path) => `No HD folder yet. The build creates ${path}.`,
    stylesBuilt: (styles) => `Built styles: ${styles.join(', ')}.`,
    NO_STYLES_BUILT: 'No style is built yet.',
    NOT_BUILT_SUFFIX: ' (not built yet)',
    DEFAULT_SUFFIX: ' (default)',
    // Where the upscaler folder a build would use came from.
    upscalerUsing: (source, dir) => ({
      chosen: `Upscaler: the folder you chose (${dir}).`,
      app: `Upscaler: DoD Studio's own (${dir}).`,
      scripts: `Upscaler: found beside the build scripts (${dir}).`,
      env: `Upscaler: found through REALESRGAN (${dir}).`,
    })[source],
    UPSCALER_MISSING: "Upscaler: none found. Download fetches it, or choose the folder you already have it in. The plain style needs no upscaler.",
    UPSCALER_PICK_BUTTON: 'Choose upscaler folder...',
    UPSCALER_PICK_TITLE: 'Choose the folder with realesrgan-ncnn-vulkan.exe',
    upscalerChosenUnused: (dir) => `The folder you chose (${dir}) isn't used: another one has more of the style models.`,
    modelsMissing: (styles) => `Missing models for: ${styles.join(', ')}.`,
    ALL_MODELS_PRESENT: 'All style models are present.',
    bytesOf: (done, total) => `${done} of ${total}`,
    SETUP_CHECKING: "Checking what's already installed…",
    progressLine: (item, step, steps, done) => `Downloading ${item} (${step} of ${steps}): ${done}`,
    unpackingLine: (item, step, steps) => `Unpacking ${item} (${step} of ${steps})…`,
    setupDone: (count) => count ? `Done: downloaded ${count} file${count === 1 ? '' : 's'}.` : 'Everything was already there.',
    SETUP_CANCELLED: 'Cancelled. Files that finished downloading are kept.',
    COPIED: 'Copied to the clipboard.',
    footerSummary: (styles, size) => `HD styles built: ${styles || 'none'} | ${size} on disk`,
    // The style comparison: compare.py's sheet.
    PREVIEW_TITLE: 'Compare styles',
    PREVIEW_HINT: 'The original and each built style side by side, at 1:1 pixels so nothing is averaged away: the most detailed textures of your maps, and a few model skins, sprites and detail textures. Click the sheet to fit it to the page.',
    PREVIEW_MAP_LABEL: 'Map:',
    PREVIEW_AUTO_MAP: 'A few of your maps, picked for you',
    PREVIEW_BUTTON: 'Show comparison',
    PREVIEW_WORKING: 'Making the sheet...',
    PREVIEW_NOTHING_BUILT: 'Build a style first: the sheet compares built files.',
    previewDone: (samples, maps) => `${samples} sample${samples === 1 ? '' : 's'}${maps.length ? `, from ${maps.join(', ')}` : ''}.`,
    previewSkipped: (skipped) => `Left out: ${skipped.join('; ')}.`,
    // The map list: the install's hd_maps.txt.
    MAPS_TITLE: 'Maps to build',
    MAPS_HINT: "Map textures and skies are built only for the maps you pick here, which saves hours on a big map collection. Model skins, sprites and detail textures aren't tied to a map, so they're always built. The command-line scripts read the same list.",
    mapsFile: (path, active) => (active ? `The list is ${path}.` : `Saved as ${path} when you pick maps.`),
    mapsOldPlace: (old, path) => `Builds read ${old} for now; saving moves the list to ${path}.`,
    MAPS_EVERY: 'Every map',
    mapsEvery: (count) => `Every map (${count})`,
    MAPS_SOME: 'Only the maps I pick',
    MAPS_LIST_LABEL: 'Picked by:',
    MAPS_LIST_EMPTY: 'Nothing yet: tick maps below, or add a pattern.',
    MAPS_ADD_BUTTON: 'Add as a line',
    MAPS_PATTERN_HELP: 'The search is read as a line: * matches any run of characters (*anzio* is every anzio map), ? one character, and a bare name is that one map. Add as a line keeps what you typed. A green tick is a map a pattern picked: click it to see which chip, and change or remove that line to leave the map out.',
    patternCount: (count) => (count ? `${count} map${count === 1 ? '' : 's'}` : 'matches no map'),
    PATTERN_ALREADY: 'Already in the list.',
    PATTERN_BAD: "A map name can't hold # or spaces.",
    patternAdded: (pattern, count) => `Added ${pattern} as a line${count ? '' : ' (it matches no map yet)'}. Save to keep it.`,
    MAPS_REMOVE_BUTTON: 'Remove',
    MAPS_SEARCH_LABEL: 'Your maps:',
    MAPS_SEARCH_PLACEHOLDER: 'Search as a line: *anzio*',
    MAPS_PICKED_ONLY: 'Picked only',
    // `patterns`: every wildcard line matching the map; `own`: it has a line of its own too.
    mapPickedByTitle: (patterns, own) => `Matched by ${own ? 'its own line and by ' : ''}the pattern${patterns.length > 1 ? 's' : ''} ${patterns.join(' and ')}. To leave this map out, change or remove ${patterns.length > 1 ? 'those lines' : 'that line'}${own ? '; unticking alone would not' : ''}.`,
    MAPS_NONE_FOUND: 'No maps found in dod\\maps. Check the Half-Life Executable on the Configuration page.',
    MAPS_NO_MATCH: 'No map matches the search.',
    mapsSummaryEvery: (count) => `All ${count} maps get map textures and skies.`,
    mapsSummary: (picked, count) => `${picked} of ${count} maps get map textures and skies.`,
    MAPS_UNSAVED: 'Not saved yet: builds still use the saved list.',
    MAPS_TICK_OWN: 'its own line',
    MAPS_TICK_PATTERN: 'a pattern',
    MAPS_SAVE_BUTTON: 'Save',
    MAPS_UNDO_BUTTON: 'Undo changes',
    MAPS_SAVED_EVERY: 'Saved: every map is built. Your list is kept, and comes back when you pick maps again.',
    mapsSaved: (picked) => `Saved: ${picked} map${picked === 1 ? '' : 's'} picked.`,
    MAPS_EMPTY: 'Pick at least one map, or choose Every map.',
    // The custom-style form: lines in the install's my_styles.txt.
    MY_STYLES_TITLE: 'Your own styles',
    MY_STYLES_HINT: 'Make a style of your own from an AI model, a plain enlargement with more or less sharpening, or a mix of two styles. Build it above like any other style. The command-line scripts read the same file.',
    myStylesFile: (path, exists) => (exists ? `Saved in ${path}.` : `Saved in ${path}, made on the first save.`),
    myStylesOldPlace: (old, path) => `Builds read ${old} for now; the first save copies it to ${path}, and builds use that from then on.`,
    myStylesError: (error) => `my_styles.txt can't be read, so builds won't start until it's fixed: ${error}`,
    MY_STYLES_NONE: 'None yet.',
    STYLE_NAME_LABEL: 'Name:',
    STYLE_NAME_PLACEHOLDER: 'e.g. crisp',
    STYLE_KIND_LABEL: 'Made by:',
    STYLE_KINDS: { plain: 'A plain enlargement, sharpened', ai: 'An AI model', blend: 'Mixing two styles' },
    STYLE_SHARPENING_LABEL: 'Sharpening (0 to 500; plain is 60):',
    STYLE_MODEL_LABEL: 'Model:',
    styleModelHint: (dir) => `To use another model, put its .param and .bin files in ${dir}\\models, then Refresh.`,
    STYLE_NO_MODELS: '(no models found)',
    STYLE_MIX_LABEL: 'Mix:',
    STYLE_MIX_OF: '% of',
    STYLE_MIX_REST: 'and the rest',
    STYLE_SAVE_BUTTON: 'Save style',
    STYLE_EDIT_BUTTON: 'Edit',
    STYLE_REMOVE_BUTTON: 'Remove',
    STYLE_BAD_NAME: 'Names are 1 to 32 lowercase letters, digits, - and _.',
    styleBuiltIn: (name) => `${name} is a built-in style; pick another name.`,
    STYLE_BLENDS_ITSELF: "A style can't mix itself.",
    styleSaved: (name) => `Saved ${name}. Build it above to make its files.`,
    styleRemoved: (name) => `Removed ${name} from my_styles.txt. Files it already built are kept.`,
    // How each kind of custom style reads in the list.
    styleDescription: (def) => ({
      ai: () => `AI model ${def.model}`,
      plain: () => `plain enlargement, sharpening ${def.sharpening}`,
      blend: () => `${def.percent}% ${def.a}, ${100 - def.percent}% ${def.b}`,
    })[def.kind](),
    STYLE_NEEDS_MODEL: ' (needs its model)',
    // The misses view: what ds_debug_hd_misses last wrote to the hook log.
    MISSES_TITLE: 'Textures that kept their original',
    MISSES_BUTTON: "Read the game's log",
    MISSES_READING: 'Reading...',
    MISSES_HINT: "After playing the demos you want to check, type this in the game's console, then press Read the game's log. The game keeps the list only until it closes.",
    MISSES_SHOW_ON_PURPOSE: 'Show textures left alone on purpose',
    MISSES_NONE: "No list in the game's log yet. Type the command above in the game's console first.",
    // `textures` is null when the hook's summary doesn't give a count: show its own words then.
    missesFrom: (date, time, style, textures, summary, mapCount) => `From ${date} at ${time}${style ? `, style ${style}` : ''}: ${
      textures === 0 ? 'every texture was replaced.'
        : textures == null ? summary
          : `${textures} texture${textures === 1 ? '' : 's'} kept their original.${mapCount > 1 ? ' One used on several maps is listed under each.' : ''}`}`,
    missesMap: (map, total, onPurpose) => `${map}: ${total} kept their original${onPurpose ? `, ${onPurpose} of them on purpose` : ''}`,
    MISSES_ONLY_ON_PURPOSE: 'Only textures left alone on purpose.',
    missesLoads: (loads, type) => (type === 'sprite' ? `${loads} frames` : `${loads} loads`),
    missesAlsoOn: (count) => `also on ${count} other map${count === 1 ? '' : 's'}`,
    // What to do about each reason, after the hook's own heading.
    MISSES_ADVICE: {
      wrong_version: 'The texture has changed since it was built (or another map has one with the same name). Build this style again: files already built are skipped, so only these are made.',
      no_file: "Nothing is built for these in this style. Build it, and check hd_maps.txt if you narrowed the maps. A decal that isn't in decals.wad is a player's spray, which has no HD version.",
      failed: "A file was found but couldn't be used; each line says why.",
      on_purpose: 'Tool textures, blank sprite frames and formats HD never replaces.',
    },
    // The hook's asset kinds in the miss list (singular, unlike the folders).
    MISS_TYPE_NAMES: { world: 'Map texture', model: 'Model skin', sprite: 'Sprite', decal: 'Decal', detail: 'Detail texture', sky: 'Sky' },
  },
};

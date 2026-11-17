# Relinking a VEGAS project after the clips moved (#1514)

A VEGAS project you are already editing points at each clip's file. Rename or move the clips afterwards (to sort them, or onto another drive) and VEGAS opens the project with every clip offline. Exporting a fresh EDL doesn't help: it builds a new timeline and loses the editing.

This swaps every moved, renamed or re-rendered clip on the existing project for its new file and keeps the edit: every event stays where it is, with its trims, speed, fades, effects and Pan/Crop, and the nameplates above stay too. It does what **Project Media > right-click > Replace** does by hand, for every clip at once.

## Steps

1. **Rename and move the clips** however you like.
2. In DoD Studio, open the project and click **Find Moved Videos** on the Render tab. It finds each video in its new place and points the project at it. Studio remembers where each one used to be.
3. **Save the project** (File > Save).
4. Click **Export VEGAS Relink List…** and save the list **beside your `.veg` project** (the script then finds it by itself; anywhere else, it asks for it). The default name is `dod_highlights.relink.json`.

   Clips moved before this Studio version kept old places: use **From a Project Backup…** instead and pick a copy of the project file saved before the move (Studio only reads it). Studio matches each render in the copy to the same render now and lists its old path. The list also gets the moves Studio remembered.
5. In VEGAS, **open the `.veg`**. When it says files are missing, choose to leave them offline (don't search).
6. Tools > Scripting > **DoD Studio Relink Clips** (after **Add Scripts to VEGAS** on the Render tab and Tools > Scripting > Rescan Script Menu Folder; or Run Script… and pick `vegas/DoD Studio Relink Clips.cs`).
7. Read the box at the end, check the clips, then **File > Save As** a new `.veg`. The script never saves; Ctrl+Z undoes every swap in one step.

### The box at the end

- how many media files were swapped, and how many already pointed at their new file;
- each new file that isn't there (its clip is left as it was): run Find Moved Videos again and export the list again;
- each clip whose new file is a different length from the old one, with both lengths: a re-render can start or end somewhere else, so check its trims by eye;
- each file VEGAS couldn't swap, with its error;
- old entries left unused in Project Media (Project Media > Remove All Unused Media clears them);
- the relink lists and clip lists it read, and the VEGAS version.

### Re-renders (optional)

**Also swap older renders of a highlight for its newest render** (unticked by default) adds a line for every older render of a highlight, to the newest render of it (same stream: a HUD pass maps to the newest HUD pass). A recapture of the same highlights counts. Studio reads both files' lengths and the toast warns about each pair whose lengths differ by more than about 3 frames, as the script's box does.

### The nameplate clip list

The script also updates the clip paths in each DoD Studio clip list (`*.dodstudio.json`, the nameplate script's) it finds beside the relink list, beside the project, and in the new clips' folders and the folders above them, so a later nameplate run works on the relinked edit. It keeps each one as it was beside it (`….dodstudio.json.before-relink`, then `-2`, `-3`…). Ctrl+Z in VEGAS doesn't put these back: copy the `.before-relink` file back over the list to undo. A clip list somewhere else: move it next to the new clips (or re-export the EDL to write a fresh one).

### Older VEGAS

The script needs `Media.ReplaceWith`, which VEGAS Pro 21's scripting has (checked on 21.0.0.315). A VEGAS whose scripting lacks it shows a compile error when the script runs: relink by hand with Project Media > right-click > Replace, using the list as a guide.

## The relink list's format

For other scripts or editors. JSON, UTF-8, written by `studio/src/vegas_relink_map.js`:

```json
{
  "format": "dodstudio-relink-map",
  "version": 1,
  "moves": [
    { "from": "E:\\Unsorted\\dyelife_row11.mov", "to": "H:\\Movie\\Armory\\dyelife.mov", "why": "moved" },
    { "from": "D:\\Clips\\row12.mov", "to": "D:\\Clips\\row12_2.mov", "why": "re-rendered", "seconds_from": 4.2, "seconds_to": 5.0 }
  ]
}
```

- One line per old path: every path a render ever had maps to where it is now. Paths compare without case and with either slash.
- `why` is `moved` or `re-rendered`. `seconds_from` and `seconds_to` are on a re-render when Studio could read the files' lengths.
- A path a newer render was saved to after the video there moved away is left out: that path holds the newer render now.
- The format only grows: readers ignore fields they don't know, and `version` goes up only if a field changes meaning.

Studio keeps the old paths on each render in the project file (`takeRenders`, `moved_from: [...]`). Older Studio builds read and save a render's entry whole, so they keep the field.

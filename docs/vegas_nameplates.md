# Nameplates in VEGAS Pro (#429)

Every highlight clip is one player's kills. This puts that player's nameplate picture (a PNG, for example their name with an outline) on its own video track above each of their clips in VEGAS Pro 21, lined up with the clip. It stays a separate event, so you can move, resize, fade or delete it per clip. VEGAS draws it at the project's resolution when it renders, so a large PNG stays sharp in a 4K render of 1440p footage.

Studio does not burn the nameplate into the clip. Burning it in, as an option for clips shared straight from Studio, is still to come (#429).

## 1. Pick a picture per player

Configuration > **Players**. Add each player (from the queue's demos, or by SteamID), then click **Pick a PNG…** in their **Nameplate** column and pick the picture. It can be anywhere on disk; Studio saves the path, not a copy, so leave the file where it is.

- A nameplate follows the player's SteamID, like **Use as**, so a player who changed names keeps one nameplate. A player added by name has no SteamID, so their Nameplate button is greyed out.
- A picture that has gone from where it was shows as `name.png (missing)`. Pick it again, or put the file back.
- × takes the nameplate off the row. The file itself stays.

## 2. Render, then export

Capture and render the highlights as usual. Then on the Render tab click **Export VEGAS EDL…** and save the EDL. Studio writes two files:

- `dod_highlights.edl`: the finished renders, one after another (#110).
- `dod_highlights.dodstudio.json`: the clip list. For each clip: its file, whose highlight it is (SteamID, the **Use as** name or else the in-game name) and that player's nameplate path.

Save the EDL in the clips' folder or the folder above it, and the script finds the clip list by itself. Anywhere else, it asks for it.

A clip gets no nameplate when its player has none set, when Studio doesn't know whose it is (a take captured before Studio kept track of takes, or a project that isn't loaded), or when one take holds two players' highlights.

## 3. Run the script in VEGAS

The script is [`vegas/DoD Studio Nameplates.cs`](../vegas/DoD%20Studio%20Nameplates.cs).

1. Put the clips on the timeline: File > Import > EDL Text File with the EDL (or drag the clips in yourself; the script goes by each clip's file, not its place).
2. Tools > Scripting > **Run Script…**, and pick `DoD Studio Nameplates.cs`.
   To have it in the Tools > Scripting menu, copy it into `Documents\Vegas Script Menu` and use Tools > Scripting > Rescan Script Menu Folder.
3. With some clips selected it does only those; with none selected, every video clip on the timeline.

It adds a track called **Nameplates** above the topmost of those clips, and on it one still per clip: the player's nameplate, from the clip's start to its end, faded in and out, in the bottom-left corner. A clip you slowed down or trimmed in VEGAS before running it gets a nameplate as long as it is now. A box then says how many it added and why any clip got none.

It never saves the project. Everything it adds is one undo step: Ctrl+Z takes it all away. Running it again adds a second track, so undo the first run (or delete its track) first.

### Changing the defaults

The defaults are at the top of the script, with a comment each:

| Setting | Default | What it is |
|---|---|---|
| `NAMEPLATE_HEIGHT` | `0.20` | its height, as a share of the frame's height |
| `MARGIN` | `0.04` | the gap to the frame's edges, as a share of the frame's height |
| `CORNER` | `"BottomLeft"` | `BottomLeft`, `BottomRight`, `TopLeft` or `TopRight` |
| `FADE_IN_SECONDS`, `FADE_OUT_SECONDS` | `0.25` | fades; `0` for none, never more than a third of a short clip |
| `TRACK_NAME` | `"Nameplates"` | the new track's name |

Edit them in any text editor and run the script again. The placement is each event's Pan/Crop, so one clip's nameplate can be moved on its own afterwards (Event Pan/Crop), or all of them at once with the track's Track Motion.

## The clip list's format

For other scripts or editors. JSON, UTF-8:

```json
{
  "format": "dodstudio-clip-manifest",
  "version": 1,
  "edl": "dod_highlights.edl",
  "clips": [
    {
      "name": "m00cat_3k_anzio",
      "path": "D:\\Clips\\m00cat_3k_anzio.mov",
      "seconds": 4.2,
      "player": { "steam_id": "76561197972576011", "name": "Chris", "in_game_name": "dicE[: :]m00cat :D" },
      "nameplate": "D:\\Nameplates\\m00cat.png"
    }
  ]
}
```

- `clips` are in the EDL's order. `seconds` is the length Studio placed the clip at.
- `player` is `null` when Studio doesn't know one player for the clip; `nameplate` is `null` when there is none. `player.name` is the Use as name when one is set, else the in-game name.
- The format only grows: readers ignore fields they don't know, and `version` goes up only if a field changes meaning. The writer is `studio/src/clip_manifest.js`.

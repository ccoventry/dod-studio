# Making DoD Studio simpler: settings to cut, and a review of every page

Drawn on the canvas: page **E** (E4 and E5 are now the slimmed settings) and the new page **F · Every other page**.

## 1. Settings to cut

Today's Configuration has nine tabs and about 60 controls. The slimmed version has **3 cards on this PC** and **4 per project**. Each row below is either made automatic, merged into another control, or moved to the page where it's used.

| Today | Suggestion | Why |
|---|---|---|
| Paths: hl.exe list, Game Folder, FFmpeg override, hook DLL override, HLAE FFmpeg row | Keep **one install picker and HLAE**. Find the rest automatically, and Setup asks only if something's missing | The hook DLL ships with the app. FFmpeg doesn't: Setup links to it like HLAE, and the app uses one copy for both (HLAE's own, the PATH, or the one picked in Setup). Settings always shows an FFmpeg row (found copy, Browse), and Setup links to ffmpeg.org. The Game Folder comes from the movie copy Setup makes |
| Initial Delay | **Remove** | The hook DLL already says when the game is ready (its pipe) |
| Pre-roll, Start Lead, Stop Trail, Post-roll | **Before the first kill** and **After the last kill**. Pre-roll and post-roll become automatic | Four timing numbers that look alike are the most confusing part of the page |
| Fast-forward 4x over 8s | One switch: **Speed up quiet gaps** | The 4x and 8s rarely need changing |
| Capture codec (Ut Video / FFV1 / x264 / uncompressed) | **Remove**: always Ut Video for Video mode | It's an intermediate file that nobody watches |
| Capture FPS, resolution | **Chips**: 60/120/300 fps and 1080p/1440p/4K. Keep a "Custom…" chip | Picking is easier than typing |
| Render codec (11 choices), custom FFmpeg args | **For editing (ProRes 422)** or **Small (H.264)**. The full list and FFmpeg args go behind a "Custom" choice on the Exports page | Most people want one of two things |
| Source FPS for renders | **Remove** | Every take already records its own FPS |
| Flush Decals, Seek Between Clips, Keep the Game Behind Your Windows | **Always on**, with no switch | All three are the right answer for everyone |
| Save Local Patched Copy, auto-clear options | **Remove** from the UI (keep them as dev flags if you need them) | They're debugging tools |
| Demo Scan Workers (1–8), max concurrent renders (1–8) | **Automatic** from the CPU core count | A user can't judge these numbers |
| After a clip is finished (keep / Recycle Bin / delete / only when space is needed) | Three chips: **Stays** / **Goes to the Recycle Bin** / **Is deleted when space runs low** | Corrected 2026-11-04: the Recycle Bin frees nothing until emptied, so "only when space is needed" has to delete for good, as the app does |
| When a batch finishes → Finish clips automatically | **Remove** the setting. The Clips page gets one **Capture and render** button | It's the same choice, made where it's used |
| Six notification switches | One switch: **Tell me when a batch finishes or fails** | |
| Scheduled Commands | Move them **onto the clip or variant** they belong to | A command at tick 1200 is about one clip, not the project |
| Initial Commands | Fold them into the **command profile** (Clean clips, or your own) | Today there are two places to type commands |
| Players tab, spray size 64/96/128 | Move to the **Players and sprays** page. Spray size becomes automatic | It's its own tool now |
| Teams | On the **Demos** page, where the team names show | |
| Clip Name Template | Keep it, with the example shown under it | People do want this one |
| Update channel | Keep it (Settings, or the Help menu) | |
| .dem Open With + "Open in" | One choice: **None / Studio / Analyzer**. None keeps DoD Studio out of Open With entirely | Double-click is never taken over, so another player (e.g. Complexity Demo Player) keeps .dem files |

## 2. Friendliness review of the drawn pages

**Studio**
- **Clips (E1):** three buttons compete in the header (Export to VEGAS, Render N, Capture N). Make it one main button, **Capture and render N**, and move VEGAS to Exports. The take-check line ("⚠ 1 frozen frame") should be clickable and open the nine-frame sheet.
- **Variant drawer (E2):** good. Grey out the switches a demo can't use, with the reason, as the nameplates row already does. The auto-built name should be editable.
- **Demos (E3):** Keep and "+ Add to project" do the same thing now. Drop the separate button so **Keep = in the project**. Maybe stays as a plain mark. Use "first-person" and "spectator (HLTV)" instead of a bare POV/HLTV wherever a newcomer sees them.
- **Settings (E4/E5):** with the cuts above, these fit on one screen each, with no tabs.

**Every other page (F1–F8)**
- They share one pattern: a title, one sentence saying what the page is for, **one main button at the top right**, and tabs only when the page truly does several jobs (Auditor, HD textures, Analyzer).
- **Exports:** this is only finished work, so it never shows a half-done list.
- **Blender:** the four steps are a checklist with one Run button on the next step, instead of four equal buttons.
- **Players and sprays / Map lighting / HD textures:** each says at the top that it only works while capturing through the hook DLL, so nobody expects them in the normal game.
- **HD textures:** the gl_max_size problem is shown as a warning with the fix, instead of a setting.
- **Overviews:** says it works in the normal game, and where to copy the files.
- **Auditor:** deleting is one button, and the page says every copy is checked byte for byte first.

## 3. Still open
- Per-project settings (#1085) is still a needs-decision issue. E5 assumes you say yes.
- The tool pages use example data, and Map lighting's editor is a proposal: today it's a light file plus a console command, with no page yet.

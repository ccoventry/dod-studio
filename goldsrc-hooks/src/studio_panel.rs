//! `dodstudio_panel`: DoD Studio's own window inside the game (issue #408,
//! plan item 4). A window with real tabs, like the Options and Find Servers
//! windows; its Playback tab drives the demo player the way the VCR bar's
//! buttons do.
//!
//! ## What it is
//!
//! Three kinds of GameUI vgui2 panel, built the way GameUI builds its own
//! Options dialog (`COptionsDialog`, a `PropertyDialog`):
//!
//! - **The window**, a `Frame`: allocated with GameUI's `operator new` (so
//!   vgui's delete at shutdown frees it with the matching allocator),
//!   constructed by `Frame::Frame` with `TaskBar` as its parent (the panel the
//!   main menu and the VCR bar hang from, so it shows and hides with the menu),
//!   laid out by `EditablePanel::LoadControlSettings` from `DodStudio.res`, and
//!   shown by `Frame::Activate`.
//! - **The tab strip**, a `PropertySheet` (`PropertySheet::PropertySheet`, the
//!   object a `PropertyDialog` keeps for its pages), sized to the window's
//!   client area every frame by [`poll`] -- what `PropertyDialog::PerformLayout`
//!   does for its own sheet.
//! - **One `PropertyPage` per tab** in [`PAGES`], each laid out by its own
//!   `.res` and added with `PropertySheet::AddPage(page, title)`, as
//!   `COptionsDialog` adds Keyboard, Mouse and the rest. The sheet draws the
//!   tabs and switches pages itself.
//!
//! Being GameUI's `Frame`, the window gets the ESC fix (#396, the shared
//! `Frame::OnKeyCodeTyped`), resizing and remembered placement (#410) and build
//! mode (Ctrl+Shift+Alt+B on a tab, then Save, which writes that tab's `.res`).
//!
//! ## Buttons
//!
//! A button sends its `Command` to the panel it sits on. `PropertyPage`'s own
//! `OnCommand` is an empty `ret 4` on both builds, so the window and its pages
//! each get a copy of their class's vftable with `OnCommand` (slot
//! [`FRAME_SLOT_ON_COMMAND`]) pointing at our handler. A command can be:
//!
//! - one of the VCR bar's own -- `play`, `pause`, `faster`, `slower`,
//!   `stepf`, `stepb`, `start`, `end`, `stop`, `load`, `events`, `save` --
//!   handed to the open VCR bar (`CDemoPlayerDialog::OnCommand`), so it does
//!   exactly what the bar's button does. With no demo in the demo player
//!   there is no bar, and the console says so;
//! - `engine <command>`, run as a console command;
//! - anything else, passed to the class's own `OnCommand` (`Close` on the
//!   window).
//!
//! ## The layout files
//!
//! `<game>\dod\dodstudio_ui\`: `DodStudio.res` for the window, then one per
//! tab (`Playback.res`, `Demos.res`, `Studio.res`). Our own folder beside
//! `dodstudio_hd` -- never `dod\resource`, which is the user's. Each default
//! (`goldsrc-hooks/ui/`, built into the DLL) is written the first time and
//! never again, so build-mode edits stay. `dodstudio_panel reset` puts the
//! defaults back and rebuilds the window.
//!
//! ## Per build
//!
//! Eight `GameUI.dll` addresses and sizes differ between the pre-Anniversary
//! and 25th Anniversary builds, and the Anniversary `Frame::Frame` takes a
//! fourth argument. [`BUILDS`] names each build by PE timestamp and image
//! size, and anything else is refused. `tools/verify_studio_panel.py` checks
//! every one against how GameUI builds its own Load Demo and Options windows,
//! on both movie installs.

// The window is 32-bit only; a host build compiles the rest for the tests.
#![cfg_attr(not(target_arch = "x86"), allow(dead_code, unused_imports))]

use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::names::console_name;

pub const NAME: &str = console_name!("panel");

/// The window's panel name, which its `.res` entry and #410's saved layout
/// (`GameUI/DodStudio`) are keyed by.
const PANEL_NAME: &CStr = c"DodStudio";
const SHEET_NAME: &CStr = c"Sheet";
/// Our folder under the game directory, as the engine's file system resolves
/// a `.res` path and build mode saves one.
const RES_DIR: &str = "dodstudio_ui";
const WINDOW_RES: (&CStr, &str, &str) = (
    c"dodstudio_ui/DodStudio.res",
    "DodStudio.res",
    include_str!("../ui/DodStudio.res"),
);

/// One tab: its panel name, its title on the tab strip, and its layout file
/// (path for the engine, file name, built-in default).
pub struct Page {
    pub name: &'static CStr,
    pub title: &'static CStr,
    pub res: (&'static CStr, &'static str, &'static str),
}

/// The tabs, in strip order.
pub const PAGES: [Page; 6] = [
    Page {
        name: c"Playback",
        title: c"Playback",
        res: (
            c"dodstudio_ui/Playback.res",
            "Playback.res",
            include_str!("../ui/Playback.res"),
        ),
    },
    Page {
        name: c"Demos",
        title: c"Demos",
        res: (
            c"dodstudio_ui/Demos.res",
            "Demos.res",
            include_str!("../ui/Demos.res"),
        ),
    },
    Page {
        name: c"Console",
        title: c"Console",
        res: (
            c"dodstudio_ui/Console.res",
            "Console.res",
            include_str!("../ui/Console.res"),
        ),
    },
    Page {
        name: c"Settings",
        title: c"Settings",
        res: (
            c"dodstudio_ui/Settings.res",
            "Settings.res",
            include_str!("../ui/Settings.res"),
        ),
    },
    Page {
        name: c"Commands",
        title: c"Commands",
        res: (
            c"dodstudio_ui/Commands.res",
            "Commands.res",
            include_str!("../ui/Commands.res"),
        ),
    },
    Page {
        name: c"Studio",
        title: c"Studio",
        res: (
            c"dodstudio_ui/Studio.res",
            "Studio.res",
            include_str!("../ui/Studio.res"),
        ),
    },
];

/// What the VCR bar's `OnCommand` handles, from the strings it compares
/// against (both builds' `GameUI.dll`).
const VCR_COMMANDS: &[&str] = &[
    "play", "pause", "faster", "slower", "stepf", "stepb", "start", "end", "stop", "load",
    "events", "save",
];

/// One `GameUI.dll` build: its identity and what building the window takes.
pub struct Build {
    pub name: &'static str,
    pub time_date_stamp: u32,
    pub size_of_image: u32,
    /// `operator new(size_t)`, cdecl.
    pub operator_new: usize,
    /// `Frame::Frame(Panel *parent, const char *name, bool showTaskbarIcon
    /// [, bool])`, thiscall.
    pub frame_ctor: usize,
    /// Whether `frame_ctor` takes the Anniversary build's fourth argument.
    pub frame_ctor_fourth_arg: bool,
    /// `EditablePanel::LoadControlSettings(const char *path, const char
    /// *pathID)`, thiscall.
    pub load_control_settings: usize,
    /// `sizeof(Frame)`: the first field of a subclass sits here.
    pub frame_size: usize,
    /// `PropertySheet::PropertySheet(Panel *parent, const char *name)`.
    pub sheet_ctor: usize,
    /// `sizeof(PropertySheet)`, what `PropertyDialog` allocates for its own.
    pub sheet_size: usize,
    /// `PropertyPage::PropertyPage(Panel *parent, const char *name, bool)`.
    pub page_ctor: usize,
    /// `CheckButton`'s vftable: only a control with exactly this one is
    /// treated as a bound check box.
    pub check_button_vftable: usize,
    /// `RichText::SetText(const wchar_t *)`, not virtual. A `.res` "text" is
    /// cut at 511 characters, and the `const char *` overload at 1023 (it
    /// converts through a 0x800-byte buffer), so the Commands tab's text is
    /// set from code through this one, which has no limit.
    pub rich_text_set_text_wide: usize,
    /// `CDemoPlayerFileDialog::CDemoPlayerFileDialog(Panel *parent, const char
    /// *name)`: the Load Demo window, which the Demos tab borrows its list and
    /// Load button from. Allocated `frame_size + 4` bytes, as GameUI does.
    pub file_dialog_ctor: usize,
    /// The Load Demo window's fill: empties its list and lists the demos
    /// again (`FindFirst("*.dem")`), `thiscall`, no arguments.
    pub file_dialog_fill: usize,
}

pub const BUILDS: [Build; 2] = [
    Build {
        name: "pre-Anniversary",
        time_date_stamp: 0x5f28_cefc,
        size_of_image: 0xe_3000,
        operator_new: 0x7_a483,
        frame_ctor: 0x4_a610,
        frame_ctor_fourth_arg: false,
        load_control_settings: 0x4_d6b0,
        frame_size: 0x110,
        sheet_ctor: 0x7_74f0,
        sheet_size: 0xac,
        page_ctor: 0x6_55f0,
        check_button_vftable: 0xa_12f4,
        rich_text_set_text_wide: 0x5_4460,
        file_dialog_ctor: 0x2_06a0,
        file_dialog_fill: 0x2_0910,
    },
    Build {
        name: "25th Anniversary",
        time_date_stamp: 0x6704_99b2,
        size_of_image: 0xd_f000,
        operator_new: 0x8_8104,
        frame_ctor: 0x5_2950,
        frame_ctor_fourth_arg: true,
        load_control_settings: 0x5_4620,
        frame_size: 0x118,
        sheet_ctor: 0x8_54f0,
        sheet_size: 0xb4,
        page_ctor: 0x7_1cd0,
        check_button_vftable: 0xa_aa48,
        rich_text_set_text_wide: 0x5_fab0,
        file_dialog_ctor: 0x2_6f00,
        file_dialog_fill: 0x2_7210,
    },
];

/// Our own Load Demo window's panel name: not the stock one's, so a VCR bar's
/// own Load Demo window is never mistaken for it.
const DEMO_LIST: &str = "DodStudioDemoList";
/// `ListPanel::GetSelectedItem(int)`, `IsValidItemID(int)`, `GetItem(int)`
/// (the same slots #409 reads the selected row through).
const LIST_SLOT_GET_SELECTED_ITEM: usize = 176;
const LIST_SLOT_IS_VALID_ITEM_ID: usize = 170;
const LIST_SLOT_GET_ITEM: usize = 153;
/// `KeyValues::GetString(const char *key, const char *default)`.
const KEYVALUES_SLOT_GET_STRING: usize = 12;
/// The key each row's demo name is stored under.
const ROW_KEY: &CStr = c"demoname";

/// The `viewdemo` line for a row of the Load Demo list: the row as is when it
/// is already quoted or has no space, quoted otherwise.
fn viewdemo_line(row: &str) -> String {
    let row = row.trim();
    if row.starts_with('"') || !row.contains(' ') {
        format!("viewdemo {row}\n")
    } else {
        format!("viewdemo \"{row}\"\n")
    }
}

/// What each page is allocated, comfortably more than `PropertyPage` (the
/// smallest Options page, a subclass with its own fields, is 0xc0 on
/// pre-Anniversary and 0xcc on Anniversary). Extra room is never touched.
const PAGE_ALLOC: usize = 0x400;

/// `Panel::OnCommand(const char *)`; the VCR bar overrides the same slot.
const FRAME_SLOT_ON_COMMAND: usize = 87;
/// `Panel::OnKeyCodeTyped(KeyCode)`, the slot #396's `Frame` patch is in.
const PANEL_SLOT_ON_KEY_CODE_TYPED: usize = 100;
/// `Panel::OnKeyCodePressed(KeyCode)`. The console dialog overrides it:
/// Tab and the arrow keys (type-ahead and command history) are handled
/// there, for keys its input line passes up to its parent.
const PANEL_SLOT_ON_KEY_CODE_PRESSED: usize = 101;
/// vgui2's `KEY_ENTER` and `KEY_PAD_ENTER` (the console's input line maps the
/// second to the first, `cmp 0x33` / `mov 0x40`).
const KEY_ENTER: i32 = 0x40;
const KEY_PAD_ENTER: i32 = 0x33;
/// `Frame::Activate()`: what GameUI calls on a dialog it has just built
/// (`jmp [vftable+0x280]`, both builds).
const FRAME_SLOT_ACTIVATE: usize = 160;
/// `Frame::GetClientArea(int &x, int &y, int &wide, int &tall)`, which
/// `PropertyDialog::PerformLayout` sizes its sheet by.
const FRAME_SLOT_GET_CLIENT_AREA: usize = 186;
/// `PropertySheet::AddPage(Panel *page, const char *title)`, what
/// `PropertyDialog::AddPage` jumps to.
const SHEET_SLOT_ADD_PAGE: usize = 134;
/// How many vftable slots a copy carries. `Frame` has about 190; the rest are
/// never called through these objects.
const VFTABLE_SLOTS: usize = 240;
/// `Panel::GetVPanel()`, the first virtual.
const PANEL_SLOT_GET_VPANEL: usize = 0;

/// `IPanel` (`VGUI_Panel007`) slots, the same table #410 checks.
const IPANEL_SET_POS: usize = 2;
const IPANEL_GET_POS: usize = 3;
const IPANEL_SET_SIZE: usize = 4;
const IPANEL_GET_SIZE: usize = 5;
const IPANEL_SET_VISIBLE: usize = 14;
const IPANEL_IS_VISIBLE: usize = 15;
const IPANEL_GET_CHILD_COUNT: usize = 17;
const IPANEL_GET_CHILD: usize = 18;
const IPANEL_GET_PARENT: usize = 19;
const IPANEL_GET_NAME: usize = 36;
const IPANEL_GET_PANEL: usize = 55;
const IPANEL_GET_MODULE_NAME: usize = 59;
/// `ISurface` (`VGUI_Surface026`) slots.
const SURFACE_GET_POPUP_COUNT: usize = 69;
const SURFACE_GET_POPUP: usize = 70;

const IPANEL_SET_MINIMUM_SIZE: usize = 6;
const IPANEL_GET_ABS_POS: usize = 10;
const IPANEL_MOVE_TO_FRONT: usize = 20;
const IPANEL_SET_KEYBOARD_INPUT_ENABLED: usize = 31;
const IPANEL_SET_PARENT: usize = 16;
/// `PropertySheet::SetActivePage(Panel *page)`, the slot after `AddPage`: it
/// looks the page up in the sheet's list and switches to it.
const SHEET_SLOT_SET_ACTIVE_PAGE: usize = 135;
/// `PropertySheet::GetActivePage()`, two slots on: returns the page field.
const SHEET_SLOT_GET_ACTIVE_PAGE: usize = 137;
/// `IPanel::RequestFocus(VPANEL, int direction)`.
const IPANEL_REQUEST_FOCUS: usize = 48;

/// The GameUI panel the window is parented to, as the VCR bar and the main
/// menu are.
const TASKBAR: &str = "TaskBar";
/// The VCR bar's panel name.
const VCR_BAR: &str = "DemoPlayerDialog";
/// The gap between the window's client area and the tab strip.
const SHEET_MARGIN: i32 = 4;
/// The smallest the window's height may go: the tab strip plus a row.
const MIN_TALL: i32 = 120;

/// A control another GameUI window lends one of our tabs: it is moved onto
/// the tab, into the slot (an empty control in that tab's `.res`) whose place
/// it takes, and handed back when our window closes. It keeps working for its
/// own window: the VCR bar keeps updating its time label and slider through
/// its own pointers, and the slider keeps seeking through the bar; the
/// console keeps printing into its history and running what its input line
/// submits, wherever they are drawn.
pub struct Loan {
    /// The lending window's panel name.
    pub source: &'static str,
    pub control: &'static str,
    pub slot: &'static str,
    /// Which of [`PAGES`] it goes on.
    pub page: usize,
}

pub const LOANS: &[Loan] = &[
    Loan {
        source: VCR_BAR,
        control: "TimeSlider",
        slot: "TimeSliderSlot",
        page: PLAYBACK_PAGE,
    },
    Loan {
        source: VCR_BAR,
        control: "TimeLabel",
        slot: "TimeLabelSlot",
        page: PLAYBACK_PAGE,
    },
    Loan {
        source: CONSOLE,
        control: "ConsoleHistory",
        slot: "ConsoleHistorySlot",
        page: CONSOLE_PAGE,
    },
    Loan {
        source: CONSOLE,
        control: CONSOLE_ENTRY,
        slot: "ConsoleEntrySlot",
        page: CONSOLE_PAGE,
    },
    Loan {
        source: CONSOLE,
        control: "ConsoleSubmit",
        slot: "ConsoleSubmitSlot",
        page: CONSOLE_PAGE,
    },
    // The Demos tab: our own (hidden) Load Demo window's list and Load
    // button. Its own OnCommand is ours, so Load runs viewdemo itself.
    Loan {
        source: DEMO_LIST,
        control: "DemoList",
        slot: "DemoListSlot",
        page: DEMOS_PAGE,
    },
    Loan {
        source: DEMO_LIST,
        control: "LoadButton",
        slot: "DemoLoadSlot",
        page: DEMOS_PAGE,
    },
    // The type-ahead list under the input line: a popup the console places
    // itself, next to its input line wherever that is. It only needs a
    // parent that is showing (the console window is hidden), so no slot.
    Loan {
        source: CONSOLE,
        control: TYPE_AHEAD,
        slot: "",
        page: CONSOLE_PAGE,
    },
];
/// The console's panel name, its input line and its type-ahead list.
const CONSOLE: &str = "GameConsole";
const CONSOLE_ENTRY: &str = "ConsoleEntry";
const TYPE_AHEAD: &str = "CompletionList";
/// Each tab's "use this tab" button, the tab it is on, and the window whose
/// controls it brings over: shown while that window's setting is off.
const ENABLE_BUTTONS: &[(&str, usize, &str)] = &[
    ("EnablePlaybackButton", PLAYBACK_PAGE, VCR_BAR),
    ("EnableConsoleButton", CONSOLE_PAGE, CONSOLE),
];

/// Whether `source`'s controls are lent to our tabs: only while its setting
/// is on.
fn lends(source: &str) -> bool {
    match source {
        VCR_BAR => viewdemo_in_panel(),
        CONSOLE => console_in_panel(),
        _ => true,
    }
}

/// The Playback and Console tabs' places in [`PAGES`].
const PLAYBACK_PAGE: usize = 0;
const DEMOS_PAGE: usize = 1;
const CONSOLE_PAGE: usize = 2;
const SETTINGS_PAGE: usize = 3;
/// A check box named `cvar_<name>` on the Settings tab is bound to cvar
/// `<name>`: it shows the cvar's value and sets it when clicked. Any tab
/// layout can add more in build mode.
const CVAR_BOX_PREFIX: &str = "cvar_";
/// `Button::SetSelected(bool)` (CheckButton's override posts
/// `CheckButtonChecked`) and `Button::IsSelected()`.
const BUTTON_SLOT_SET_SELECTED: usize = 173;
/// `TextEntry::GetText(char *buf, int bufLen)`, as the console reads its own
/// input line (`call [vftable+0x224]`).
const TEXT_ENTRY_SLOT_GET_TEXT: usize = 137;
/// `Label::SetText(const char *)`, as GameUI calls it with its `#GameUI_`
/// strings (`call [vftable+0x21c]`).
const LABEL_SLOT_SET_TEXT: usize = 135;
const BUTTON_SLOT_IS_SELECTED: usize = 174;

/// The Commands tab's text: every console setting and command, grouped.
const COMMANDS_TEXT: &str = include_str!("../ui/Commands.txt");
/// The Commands tab's text box, in `Commands.res`.
const COMMAND_LIST: &str = "CommandList";
const COMMANDS_PAGE: usize = 4;

/// The Demos tab's search box.
const DEMO_FILTER: &str = "DemoFilter";
/// `ListPanel::FirstItem()`, `NextItem(int)`, `SetItemVisible(int, bool)`
/// (the Source `ListPanel` order, which GoldSrc's matches from `GetItem(int)`
/// at 153 to `GetSelectedItem` at 176).
const LIST_SLOT_FIRST_ITEM: usize = 167;
const LIST_SLOT_NEXT_ITEM: usize = 168;
const LIST_SLOT_SET_ITEM_VISIBLE: usize = 171;
/// `AddColumnHeader(int index, const char *name, const char *text, int width,
/// int flags)` and `SetColumnSortable(int, bool)`: what the Demos tab's Map
/// and Date columns are built with.
const LIST_SLOT_ADD_COLUMN_HEADER: usize = 134;
const LIST_SLOT_SET_COLUMN_SORTABLE: usize = 148;
/// `ApplyItemChanges(int itemID)`: re-sorts a row after its values changed.
const LIST_SLOT_APPLY_ITEM_CHANGES: usize = 161;
/// `DeleteAllItems()`: what the Load Demo window's own fill starts with.
const LIST_SLOT_DELETE_ALL_ITEMS: usize = 165;
/// `KeyValues::SetString(const char *key, const char *value)`: the 3-argument
/// `KeyValues` constructor the demo list's rows are made with calls it.
const KEYVALUES_SLOT_SET_STRING: usize = 17;
/// The width the Demos tab gives the demo name column.
const NAME_COLUMN_WIDE: i32 = 250;
/// The Demos tab's columns past the demo's own name: key, heading, width.
const DEMO_COLUMNS: [(&CStr, &CStr, i32); 2] = [(c"map", c"Map", 130), (c"date", c"Date", 120)];
/// Set on a row once its Map and Date are filled in, so a list the Load Demo
/// window refilled on its own (opening a folder) is noticed.
const STAMP_KEY: &CStr = c"dodstudio";

/// Whether a demo row matches what was typed in the search box: every word,
/// anywhere in the name, case ignored. An empty search matches everything.
fn matches_filter(row: &str, filter: &str) -> bool {
    let row = row.to_ascii_lowercase();
    filter
        .split_whitespace()
        .all(|word| row.contains(&word.to_ascii_lowercase()))
}

/// The Demos tab's other filters: map, HLTV / POV, and age.
const MAP_FILTER: &str = "MapFilter";
const SHOW_HLTV: &str = "ShowHltv";
const SHOW_POV: &str = "ShowPov";
const DAYS_FILTER: &str = "DaysFilter";

/// What the start of a demo says about it: the map (header offset 16, 260
/// bytes), and whether an HLTV proxy recorded it. The proxy's connect message
/// ends "Spawn count N (HLTV)", about 1,060 bytes in: true of all 113 HLTV
/// demos among 984 surveyed, and of none of the POV ones. (`HLTV Proxy`, what
/// Studio's `is_hltv_demo` looks for in the first 512 bytes, is in none.)
#[derive(Debug, Clone, PartialEq, Eq)]
struct DemoInfo {
    map: String,
    hltv: bool,
    /// Seconds since the Unix epoch the file was last written.
    modified: u64,
}

fn demo_info(header: &[u8], modified: u64) -> Option<DemoInfo> {
    if header.len() < 16 + 260 || !header.starts_with(b"HLDEMO") {
        return None;
    }
    let map = &header[16..16 + 260];
    let map = &map[..map.iter().position(|&b| b == 0).unwrap_or(map.len())];
    let map = String::from_utf8_lossy(map);
    let map = map
        .trim_start_matches("maps/")
        .trim_end_matches(".bsp")
        .to_string();
    let hltv = header.windows(6).any(|w| w == b"(HLTV)");
    Some(DemoInfo {
        map,
        hltv,
        modified,
    })
}

/// `secs` (Unix time, UTC) as a calendar date and time:
/// (year, month, day, hour, minute).
fn civil(secs: u64) -> (u64, u64, u64, u64, u64) {
    // Howard Hinnant's days-to-civil.
    let days = secs / 86_400;
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day, secs % 86_400 / 3600, secs % 3600 / 60)
}

/// The Date column's text: sorts as text in date order.
fn date_text((year, month, day, hour, minute): (u64, u64, u64, u64, u64)) -> String {
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Everything the Demos tab filters by at once.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DemoFilters {
    search: String,
    map: String,
    hltv: bool,
    pov: bool,
    /// Newer than this many days, or none.
    days: Option<u64>,
}

/// Whether a row passes every filter. `info` is `None` for a row whose
/// header couldn't be read (or a folder): only the search applies to it.
fn passes(row: &str, info: Option<&DemoInfo>, f: &DemoFilters, now: u64) -> bool {
    let haystack = match info {
        Some(i) => format!("{row} {}", i.map),
        None => row.to_string(),
    };
    if !matches_filter(&haystack, &f.search) {
        return false;
    }
    let Some(info) = info else { return true };
    if !f.map.trim().is_empty() && !matches_filter(&info.map, &f.map) {
        return false;
    }
    if (info.hltv && !f.hltv) || (!info.hltv && !f.pov) {
        return false;
    }
    if let Some(days) = f.days
        && now.saturating_sub(info.modified) > days * 86_400
    {
        return false;
    }
    true
}

/// The Playback tab's time box.
const GOTO_BOX: &str = "GotoTime";

/// A time typed into the time box, in seconds of world time -- the clock the
/// VCR bar shows. `75` or `75.5` are seconds; `1:15` is minutes and seconds;
/// `1:15:50` is minutes, seconds and hundredths, as the bar writes it.
fn parse_time(text: &str) -> Result<f64, String> {
    let text = text.trim();
    let bad = || format!("\"{text}\" is not a time -- try 20:33, 20:33:50 or 1233");
    let fields: Vec<&str> = text.split(':').map(str::trim).collect();
    let number = |f: &str| -> Result<f64, String> {
        f.parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v >= 0.0)
            .ok_or_else(bad)
    };
    let seconds = match fields.as_slice() {
        [s] => number(s)?,
        [m, s] => number(m)? * 60.0 + number(s)?,
        [m, s, c] => number(m)? * 60.0 + number(s)? + number(c)? / 100.0,
        _ => return Err(bad()),
    };
    Ok(seconds)
}

/// One line of the saved settings file: `name value`, `name` a plain cvar
/// name and `value` a number, so the file can never run anything else.
fn settings_line(line: &str) -> Option<(&str, &str)> {
    let (name, value) = line.trim().split_once(' ')?;
    let value = value.trim();
    (bound_cvar(&format!("{CVAR_BOX_PREFIX}{name}")).is_some() && value.parse::<f64>().is_ok())
        .then_some((name, value))
}

/// The saved settings, `name value` per line, with `name` set to `value`.
fn settings_with(text: &str, name: &str, value: &str) -> String {
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| settings_line(l).is_some_and(|(n, _)| n != name))
        .map(str::to_string)
        .collect();
    lines.push(format!("{name} {value}"));
    lines.join("\n") + "\n"
}

/// The cvar a Settings check box is bound to, from its name.
fn bound_cvar(control: &str) -> Option<&str> {
    control.strip_prefix(CVAR_BOX_PREFIX).filter(|name| {
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
    })
}

/// What a bound check box needs: `Some(value)` to set the cvar to (the user
/// clicked it), or `None`, where `show` says whether the box must be redrawn
/// to the cvar's value instead.
fn settle(checked: bool, last: Option<bool>, cvar_on: bool) -> (Option<bool>, bool) {
    match last {
        Some(was) if was != checked => (Some(checked), false),
        _ => (None, checked != cvar_on),
    }
}
/// Where the VCR bar waits, off screen, while `dodstudio_viewdemo_in_panel` has
/// our window stand in for it. Off screen rather than hidden: a hidden panel
/// stops thinking, and the bar's think is what updates the time and slider.
const PARKED_AT: i32 = -20_000;
/// How many frames after `viewdemo` to wait for the VCR bar to appear.
const VIEWDEMO_WAIT_FRAMES: u32 = 600;

/// `dodstudio_viewdemo_in_panel 1`: `viewdemo` opens our window on the
/// Playback tab and parks the VCR bar off screen. Not `dodstudio_panel_...`:
/// no name may be the start of another (the console's autocomplete).
pub const VIEWDEMO_NAME: &str = console_name!("viewdemo_in_panel");
/// The fallback toggle, when the cvar could not be registered.
pub static VIEWDEMO_IN_PANEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static VIEWDEMO_CVAR: std::sync::atomic::AtomicPtr<crate::engine::CvarSPartial> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
/// Frames left to wait for the VCR bar after a `viewdemo`, or 0.
static VIEWDEMO_PENDING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Our window's object, or 0 before it is built. #410 accepts it as a GameUI
/// window although its vftable is our copy.
static OBJECT: AtomicUsize = AtomicUsize::new(0);
/// Its tab strip.
static SHEET: AtomicUsize = AtomicUsize::new(0);
/// Its pages, in [`PAGES`] order.
static PAGE_OBJECTS: [AtomicUsize; PAGES.len()] = [const { AtomicUsize::new(0) }; PAGES.len()];
/// The VCR bar's panel while it is parked off screen, or 0.
static PARKED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The window's `Frame` object, for #410's walk.
pub fn object() -> usize {
    OBJECT.load(Ordering::Acquire)
}

/// The VCR bar's panel while our window stands in for it, for #410, which
/// must not save or restore a position we put it at.
pub fn parked_bar() -> u32 {
    PARKED.load(Ordering::Acquire)
}

/// `dodstudio_console_in_panel 1`: the console key (`toggleconsole`) opens our
/// window on its Console tab instead of the console window.
pub const CONSOLE_NAME: &str = console_name!("console_in_panel");
/// The fallback toggle, when the cvar could not be registered.
pub static CONSOLE_IN_PANEL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
static CONSOLE_CVAR: std::sync::atomic::AtomicPtr<crate::engine::CvarSPartial> =
    std::sync::atomic::AtomicPtr::new(std::ptr::null_mut());
/// Frames left to wait for the console window after `toggleconsole`, or 0.
static CONSOLE_PENDING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
/// The engine's own `toggleconsole`, once wrapped.
static REAL_TOGGLECONSOLE: AtomicUsize = AtomicUsize::new(0);
static TOGGLECONSOLE_WRAPPED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn set_console_cvar(cvar: *mut crate::engine::CvarSPartial) {
    CONSOLE_CVAR.store(cvar, Ordering::Release);
}

fn console_in_panel() -> bool {
    let cvar = CONSOLE_CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        CONSOLE_IN_PANEL.load(Ordering::Relaxed)
    } else {
        // Safety: the engine owns the cvar for the session.
        unsafe { (*cvar).value != 0.0 }
    }
}

/// For the fallback toggle's bare-name query.
pub fn console_status() -> String {
    if console_in_panel() {
        "the console key opens the DoD Studio window on its Console tab".to_string()
    } else {
        "the console key opens the console window".to_string()
    }
}

/// Wraps `toggleconsole` (the console key's binding), once the engine has it.
fn wrap_toggleconsole() {
    if TOGGLECONSOLE_WRAPPED.load(Ordering::Relaxed) {
        return;
    }
    let mut found = false;
    crate::cmd_list::for_each(|name, entry| {
        if name.eq_ignore_ascii_case(b"toggleconsole") {
            found |= crate::cmd_list::wrap(entry, &REAL_TOGGLECONSOLE, wrapped_toggleconsole);
        }
    });
    if found {
        TOGGLECONSOLE_WRAPPED.store(true, Ordering::Relaxed);
        unsafe {
            crate::debug::report(
                "studio_panel: wrapped toggleconsole for dodstudio_console_in_panel",
            )
        };
    }
}

/// The console key. With `dodstudio_console_in_panel 1` it closes our window
/// when the Console tab is showing; otherwise it runs the engine's own
/// `toggleconsole` (which also brings up the menu when in game) and [`poll`]
/// then swaps the console window for our Console tab.
unsafe extern "C" fn wrapped_toggleconsole() {
    if console_in_panel() {
        #[cfg(target_arch = "x86")]
        if hook::close_if_on_console() {
            return;
        }
        CONSOLE_PENDING.store(VIEWDEMO_WAIT_FRAMES, Ordering::Release);
    }
    unsafe { crate::cmd_list::call_real(&REAL_TOGGLECONSOLE) };
}

/// Called by `commands.rs` once the cvar is registered.
pub fn set_viewdemo_cvar(cvar: *mut crate::engine::CvarSPartial) {
    VIEWDEMO_CVAR.store(cvar, Ordering::Release);
}

fn viewdemo_in_panel() -> bool {
    let cvar = VIEWDEMO_CVAR.load(Ordering::Acquire);
    if cvar.is_null() {
        VIEWDEMO_IN_PANEL.load(Ordering::Relaxed)
    } else {
        // Safety: the engine owns the cvar for the session.
        unsafe { (*cvar).value != 0.0 }
    }
}

/// For the fallback toggle's bare-name query.
pub fn viewdemo_status() -> String {
    if viewdemo_in_panel() {
        "viewdemo opens the DoD Studio window on its Playback tab".to_string()
    } else {
        "viewdemo opens the stock demo bar".to_string()
    }
}

/// Called by `demo_reload`'s `viewdemo` wrapper for a bare `viewdemo`, which
/// on its own only prints its usage. With `dodstudio_viewdemo_in_panel 1` it
/// opens our window on the Playback tab instead, as `viewdemo` opens the VCR
/// bar, and returns whether it did.
pub fn bare_viewdemo() -> bool {
    if !viewdemo_in_panel() {
        return false;
    }
    #[cfg(target_arch = "x86")]
    {
        let line = match hook::open_on(PLAYBACK_PAGE) {
            Ok(state) => format!("{NAME}: viewdemo opened the window -- {state}"),
            Err(why) => format!("{NAME}: viewdemo could not open the window -- {why}"),
        };
        crate::commands::console_print(&format!("{line}\n"));
        true
    }
    #[cfg(not(target_arch = "x86"))]
    false
}

/// The main menu DoD Studio writes, in `dod_addon` (read only with
/// `-addons`, #412, and over the game's own `dod\resource` one, which is the
/// user's and never written). "DoD Studio" opens our window; in a game or a
/// demo the menu also has Resume and Disconnect; Options and Quit stay.
const GAME_MENU: &str = include_str!("../ui/GameMenu.res");
/// What marks a `dod_addon` menu as DoD Studio's own, to be kept up to date;
/// any other file there is left alone.
const GAME_MENU_MARK: &str = "DoD Studio";

/// What to do with `dod_addon\resource\GameMenu.res`, given what is there.
fn game_menu_action(existing: Option<&str>) -> Option<&'static str> {
    match existing {
        None => Some("wrote"),
        Some(text) if text == GAME_MENU => None,
        Some(text) if text.contains(GAME_MENU_MARK) => Some("updated"),
        Some(_) => None,
    }
}

/// Writes DoD Studio's main menu into `dod_addon`, unless a menu that isn't
/// ours is already there. Runs at load, before GameUI reads the menu.
/// `GOLDSRC_HOOKS_GAME_MENU=0` turns it off.
pub fn write_game_menu() {
    let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
    else {
        return;
    };
    let path = dir.join("dod_addon").join("resource").join("GameMenu.res");
    let existing = std::fs::read_to_string(&path).ok();
    let Some(verb) = game_menu_action(existing.as_deref()) else {
        if existing.is_some_and(|t| !t.contains(GAME_MENU_MARK)) {
            unsafe {
                crate::debug::report(&format!(
                    "studio_panel: {} is not DoD Studio's, left alone",
                    path.display()
                ))
            };
        }
        return;
    };
    let written = path
        .parent()
        .is_some_and(|p| std::fs::create_dir_all(p).is_ok())
        && std::fs::write(&path, GAME_MENU).is_ok();
    unsafe {
        crate::debug::report(&format!(
            "studio_panel: {} the DoD Studio main menu at {}{}",
            if written { verb } else { "could not write" },
            path.display(),
            if written {
                " (shown when the game is launched with -addons)"
            } else {
                ""
            }
        ))
    };
}

/// Called by `demo_reload`'s `viewdemo` wrapper after the engine's own
/// `viewdemo` ran: the bar appears a few frames later, and [`poll`] opens our
/// window then.
pub fn after_viewdemo() {
    if viewdemo_in_panel() {
        VIEWDEMO_PENDING.store(VIEWDEMO_WAIT_FRAMES, Ordering::Release);
    }
}

/// The smallest the window may be: wide enough for every tab, plus the frame's
/// own border, and [`MIN_TALL`] high.
fn minimum_size(tabs_right: i32, frame_wide: i32, client_wide: i32) -> (i32, i32) {
    (
        tabs_right + 2 * SHEET_MARGIN + (frame_wide - client_wide).max(0) + 8,
        MIN_TALL,
    )
}

/// What a button's command asks for.
#[derive(Debug, PartialEq, Eq)]
enum Action<'a> {
    Vcr(&'a str),
    Engine(&'a str),
    /// Jump to the time typed in the Playback tab's time box.
    Goto,
    /// Put every setting saved from the Settings tab back to its default.
    ResetSettings,
    /// Anything else, for the class's own `OnCommand`.
    Own,
}

fn action(command: &str) -> Action<'_> {
    if let Some(line) = command.strip_prefix("engine ") {
        return Action::Engine(line.trim());
    }
    if command.eq_ignore_ascii_case("goto") {
        return Action::Goto;
    }
    if command.eq_ignore_ascii_case("reset_settings") {
        return Action::ResetSettings;
    }
    match VCR_COMMANDS
        .iter()
        .find(|c| c.eq_ignore_ascii_case(command))
    {
        Some(c) => Action::Vcr(c),
        None => Action::Own,
    }
}

/// Where the tab strip goes in a window whose client area is `client`
/// (x, y, wide, tall): all of it, inset by [`SHEET_MARGIN`], less the help
/// line along the bottom.
fn sheet_bounds(client: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = client;
    (
        x + SHEET_MARGIN,
        y + SHEET_MARGIN,
        (w - 2 * SHEET_MARGIN).max(1),
        (h - 2 * SHEET_MARGIN - HELP_TALL).max(1),
    )
}

/// The help line under the tab strip: what the control under the mouse does.
fn help_bounds(sheet: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = sheet;
    (x + 2, y + h + 2, (w - 4).max(1), HELP_TALL - 2)
}

/// The help line's panel name, in `DodStudio.res`.
const HELP_LINE: &str = "HelpLine";
const HELP_TALL: i32 = 22;

/// Every control's `"tooltiptext"` in a `.res` file, by its name: what the
/// help line shows for it. (GameUI reads the key but shows no tooltip, so
/// the window shows it itself.)
fn tooltips(res: &str) -> Vec<(String, String)> {
    #[derive(Clone, PartialEq)]
    enum Token {
        Text(String),
        Open,
        Close,
    }
    // Quoted strings and braces; // comments run to the end of the line.
    let mut tokens = Vec::new();
    let mut chars = res.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => tokens.push(Token::Text(
                chars.by_ref().take_while(|&c| c != '"').collect(),
            )),
            '{' => tokens.push(Token::Open),
            '}' => tokens.push(Token::Close),
            '/' if chars.peek() == Some(&'/') => {
                chars.by_ref().take_while(|&c| c != '\n').for_each(drop);
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    let mut depth = 0;
    let mut control: Option<String> = None;
    let mut i = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Open => depth += 1,
            Token::Close => {
                depth -= 1;
                if depth < 2 {
                    control = None;
                }
            }
            Token::Text(name) if depth == 1 && tokens.get(i + 1) == Some(&Token::Open) => {
                control = Some(name.clone());
            }
            Token::Text(key) if depth == 2 => {
                if key.eq_ignore_ascii_case("tooltiptext")
                    && let (Some(name), Some(Token::Text(tip))) = (&control, tokens.get(i + 1))
                {
                    out.push((name.clone(), tip.clone()));
                }
                i += 1; // the key's value
            }
            Token::Text(_) => {}
        }
        i += 1;
    }
    out
}

fn res_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_default()
        .join("dod")
        .join(RES_DIR)
}

/// Where Settings-tab changes are kept between launches: our own file, never
/// the user's `config.cfg`.
fn settings_path() -> std::path::PathBuf {
    res_dir().join("settings.cfg")
}

/// Each cvar's value before the saved settings were applied, so Reset can
/// put it back.
static DEFAULTS: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
static SETTINGS_APPLIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Applies the saved settings once, as soon as the engine can run commands
/// and our cvars exist, noting each cvar's value first.
fn apply_saved_settings() {
    if SETTINGS_APPLIED.load(Ordering::Relaxed) {
        return;
    }
    let Some(engfuncs) = crate::engine::engfuncs() else {
        return;
    };
    // The cvars are registered with the rest of the hook's surface; wait
    // until ours is there.
    let Ok(probe) = std::ffi::CString::new(VIEWDEMO_NAME) else {
        return;
    };
    if unsafe { (engfuncs.pfn_get_cvar_pointer)(probe.as_ptr()) }.is_null() {
        return;
    }
    SETTINGS_APPLIED.store(true, Ordering::Relaxed);
    let Ok(text) = std::fs::read_to_string(settings_path()) else {
        return;
    };
    let mut defaults = DEFAULTS.lock().unwrap_or_else(|e| e.into_inner());
    let mut applied = Vec::new();
    for (name, value) in text.lines().filter_map(settings_line) {
        let Ok(c_name) = std::ffi::CString::new(name) else {
            continue;
        };
        let before = unsafe { (engfuncs.pfn_get_cvar_float)(c_name.as_ptr()) };
        if !defaults.iter().any(|(n, _)| n == name) {
            defaults.push((name.to_string(), format!("{before}")));
        }
        if let Ok(line) = std::ffi::CString::new(format!("{name} {value}\n")) {
            crate::engine::client_cmd(&line);
        }
        applied.push(format!("{name} {value}"));
    }
    if !applied.is_empty() {
        unsafe {
            crate::debug::report(&format!(
                "studio_panel: applied saved settings from {}: {}",
                settings_path().display(),
                applied.join(", ")
            ))
        };
    }
}

/// Remembers a Settings-tab change for the next launch.
fn save_setting(name: &str, value: &str, default_before: f32) {
    {
        let mut defaults = DEFAULTS.lock().unwrap_or_else(|e| e.into_inner());
        if !defaults.iter().any(|(n, _)| n == name) {
            defaults.push((name.to_string(), format!("{default_before}")));
        }
    }
    let path = settings_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let _ = std::fs::create_dir_all(res_dir());
    if std::fs::write(&path, settings_with(&text, name, value)).is_err() {
        unsafe {
            crate::debug::report(&format!("studio_panel: could not save {}", path.display()))
        };
    }
}

/// Puts every saved setting back to its value before DoD Studio changed it,
/// and forgets them.
fn reset_settings() -> String {
    let defaults = std::mem::take(&mut *DEFAULTS.lock().unwrap_or_else(|e| e.into_inner()));
    for (name, value) in &defaults {
        if let Ok(line) = std::ffi::CString::new(format!("{name} {value}\n")) {
            crate::engine::client_cmd(&line);
        }
    }
    let _ = std::fs::remove_file(settings_path());
    format!("{} setting(s) back to their defaults", defaults.len())
}

/// Writes each default layout that is missing (or all of them on `reset`),
/// and says what it wrote. Never touches an existing file otherwise:
/// build-mode edits are the user's.
fn ensure_res(reset: bool) -> Result<Option<String>, String> {
    let dir = res_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let mut written = Vec::new();
    for (_, file, default) in std::iter::once(WINDOW_RES).chain(PAGES.iter().map(|p| p.res)) {
        let path = dir.join(file);
        if path.exists() && !reset {
            continue;
        }
        std::fs::write(&path, default)
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        written.push(file);
    }
    Ok((!written.is_empty()).then(|| format!("wrote {} to {}", written.join(", "), dir.display())))
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;

    type Vpanel = u32;
    type CreateInterfaceFn = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    type OperatorNewFn = unsafe extern "C" fn(usize) -> *mut c_void;
    type FrameCtor3 = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char, u32);
    type FrameCtor4 =
        unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char, u32, u32);
    type SheetCtor = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char);
    type PageCtor = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char, u32);
    type AddPageFn = unsafe extern "thiscall" fn(*mut c_void, *mut c_void, *const c_char);
    type LoadSettingsFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_char);
    type OnCommandFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char);
    type ActivateFn = unsafe extern "thiscall" fn(*mut c_void);
    type KeyFn = unsafe extern "thiscall" fn(*mut c_void, i32);
    type ClientAreaFn =
        unsafe extern "thiscall" fn(*mut c_void, *mut i32, *mut i32, *mut i32, *mut i32);
    type GetVpanelFn = unsafe extern "thiscall" fn(*mut c_void) -> Vpanel;
    type PanelBoolFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> u32;
    type PanelSetBoolFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, u32);
    type PanelIntFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> i32;
    type ChildFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, i32) -> Vpanel;
    type ParentFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> Vpanel;
    type StrFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel) -> *const c_char;
    type GetPanelFn =
        unsafe extern "thiscall" fn(*mut c_void, Vpanel, *const c_char) -> *mut c_void;
    type CountFn = unsafe extern "thiscall" fn(*mut c_void) -> i32;
    type PopupFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> Vpanel;
    type XyFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, i32, i32);
    type PanelFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel);
    type SetParentFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, Vpanel);
    type SetActivePageFn = unsafe extern "thiscall" fn(*mut c_void, *mut c_void);
    type ListIntFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> u32;
    type IsSelectedFn = unsafe extern "thiscall" fn(*mut c_void) -> u32;
    type GetTextFn = unsafe extern "thiscall" fn(*mut c_void, *mut c_char, i32);
    type SetTextFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char);
    type SetWideTextFn = unsafe extern "thiscall" fn(*mut c_void, *const u16);
    type SetSelectedFn = unsafe extern "thiscall" fn(*mut c_void, u32);
    type ListItemFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> *mut c_void;
    type ListFirstFn = unsafe extern "thiscall" fn(*mut c_void) -> i32;
    type ListVoidFn = unsafe extern "thiscall" fn(*mut c_void);
    type ListItemIdFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> i32;
    type ListSetVisibleFn = unsafe extern "thiscall" fn(*mut c_void, i32, u32);
    type AddColumnFn =
        unsafe extern "thiscall" fn(*mut c_void, i32, *const c_char, *const c_char, i32, i32);
    type ListIntVoidFn = unsafe extern "thiscall" fn(*mut c_void, i32);
    type ListIntBoolFn = unsafe extern "thiscall" fn(*mut c_void, i32, u32);
    type SetStringFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_char);
    type GetStringFn =
        unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_char) -> *const c_char;
    type GetActivePageFn = unsafe extern "thiscall" fn(*mut c_void) -> *mut c_void;
    type GetXyFn = unsafe extern "thiscall" fn(*mut c_void, Vpanel, *mut i32, *mut i32);

    /// The window's size as its `.res` laid it out, read straight after
    /// loading it: what the tabs' designs are worked back to.
    static WINDOW_DESIGN: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

    /// Frames left to keep giving the borrowed console input line the
    /// keyboard after the console key opened the Console tab.
    static FOCUS_ENTRY: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    const FOCUS_ENTRY_FRAMES: u32 = 5;

    /// Each class's own `OnCommand`, for whatever our handler passes on.
    static FRAME_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    static PAGE_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    static PAGE_ON_KEY: AtomicUsize = AtomicUsize::new(0);
    static PAGE_ON_KEY_PRESSED: AtomicUsize = AtomicUsize::new(0);
    static DEMO_LIST_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    /// Our own Load Demo window, or 0.
    static DEMO_DIALOG: AtomicUsize = AtomicUsize::new(0);

    /// The selected row's `demoname` in our Load Demo window's list.
    unsafe fn selected_demo(dialog: *mut c_void) -> Option<String> {
        unsafe {
            let (_, build) = gameui().ok()?;
            let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
            if list.is_null() {
                return None;
            }
            let get_selected: ListIntFn = slot(list, LIST_SLOT_GET_SELECTED_ITEM);
            let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
            let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
            let id = get_selected(list, 0) as i32;
            if is_valid(list, id) & 0xff == 0 {
                return None;
            }
            let row = get_item(list, id);
            if row.is_null() {
                return None;
            }
            let get_string: GetStringFn = slot(row, KEYVALUES_SLOT_GET_STRING);
            let raw = get_string(row, ROW_KEY.as_ptr(), c"".as_ptr());
            (!raw.is_null())
                .then(|| text(raw))
                .filter(|t| !t.is_empty())
        }
    }

    /// Our Load Demo window's `OnCommand`. Load (and a double-click, which
    /// sends the same command) on a demo runs `viewdemo` here: the stock
    /// window would post it to its parent for the VCR bar to run, and our
    /// window is no VCR bar. A folder row (#409) and everything else go to
    /// the window's own handler.
    unsafe extern "thiscall" fn demo_list_on_command(this: *mut c_void, raw: *const c_char) {
        let command = text(raw);
        if command.eq_ignore_ascii_case("load") {
            match unsafe { selected_demo(this) } {
                Some(row) if !row.trim_end_matches('"').ends_with('/') => {
                    let line = viewdemo_line(&row);
                    let ran = std::ffi::CString::new(line.clone())
                        .is_ok_and(|l| crate::engine::client_cmd(&l));
                    unsafe {
                        crate::debug::report(&format!(
                            "studio_panel: Demos tab {} {}",
                            if ran { "ran" } else { "could not run" },
                            line.trim()
                        ))
                    };
                    return;
                }
                Some(_) => {}
                None => {
                    crate::commands::console_print(&format!(
                        "{NAME}: pick a demo in the list first\n"
                    ));
                    return;
                }
            }
        }
        let original = DEMO_LIST_ON_COMMAND.load(Ordering::Acquire);
        if original != 0 {
            // Safety: the window's own OnCommand, from its vftable.
            let original: OnCommandFn = unsafe { std::mem::transmute(original) };
            unsafe { original(this, raw) };
        }
    }

    /// What the search box held when the list was last filtered, and
    /// whether the list has been refilled since (every row shows again).
    static FILTERED_FOR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

    /// Shows only the demos matching the Demos tab's search box. Runs every
    /// frame; does work only when the text (or the list) changed.
    /// A text box's text on `page`, or "" when the layout has none.
    unsafe fn box_text(vgui: &Vgui, page: Vpanel, name: &str) -> String {
        unsafe {
            let Some(entry) = vgui
                .child_named(page, name)
                .map(|vp| vgui.object(vp))
                .filter(|o| !o.is_null())
            else {
                return String::new();
            };
            let get_text: GetTextFn = slot(entry, TEXT_ENTRY_SLOT_GET_TEXT);
            let mut buf = [0u8; 128];
            get_text(entry, buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
            CStr::from_bytes_until_nul(&buf)
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_default()
        }
    }

    /// Whether a check box on `page` is ticked; `true` when the layout has
    /// none, so a layout without it filters nothing out.
    unsafe fn box_ticked(vgui: &Vgui, page: Vpanel, name: &str) -> bool {
        unsafe {
            let Ok((base, build)) = gameui() else {
                return true;
            };
            match vgui.child_named(page, name).map(|vp| vgui.object(vp)) {
                Some(o)
                    if !o.is_null()
                        && *(o as *const usize) == base + build.check_button_vftable =>
                {
                    let is_selected: IsSelectedFn = slot(o, BUTTON_SLOT_IS_SELECTED);
                    is_selected(o) & 0xff != 0
                }
                _ => true,
            }
        }
    }

    /// Each demo's header facts, by its path, kept while the file is unchanged.
    static DEMO_INFO: std::sync::Mutex<Vec<(String, u64, Option<DemoInfo>)>> =
        std::sync::Mutex::new(Vec::new());

    /// How much of a demo `demo_info` reads.
    const DEMO_START: usize = 4096;

    /// The header facts of the demo a row names (a path from `dod/`).
    fn info_for(row: &str) -> Option<DemoInfo> {
        let name = row.trim().trim_matches('"');
        if name.ends_with('/') || name.is_empty() {
            return None;
        }
        let path = res_dir().parent()?.join(name);
        let meta = std::fs::metadata(&path).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();
        let mut cache = DEMO_INFO.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, m, info)) = cache.iter().find(|(p, _, _)| p == name)
            && *m == modified
        {
            return info.clone();
        }
        use std::io::Read;
        let mut header = Vec::with_capacity(DEMO_START);
        let info = std::fs::File::open(&path)
            .and_then(|f| f.take(DEMO_START as u64).read_to_end(&mut header))
            .ok()
            .and_then(|_| demo_info(&header, modified));
        cache.retain(|(p, _, _)| p != name);
        cache.push((name.to_string(), modified, info.clone()));
        info
    }

    unsafe fn filter_demo_list(vgui: &Vgui) {
        unsafe {
            let page = vpanel_of(PAGE_OBJECTS[DEMOS_PAGE].load(Ordering::Acquire) as *mut c_void);
            if page == 0 || !vgui.visible(page) {
                return;
            }
            let filters = DemoFilters {
                search: box_text(vgui, page, DEMO_FILTER),
                map: box_text(vgui, page, MAP_FILTER),
                hltv: box_ticked(vgui, page, SHOW_HLTV),
                pov: box_ticked(vgui, page, SHOW_POV),
                days: box_text(vgui, page, DAYS_FILTER).trim().parse::<u64>().ok(),
            };
            let filter = format!("{filters:?}");
            let dialog = DEMO_DIALOG.load(Ordering::Acquire) as *mut c_void;
            let Ok((_, build)) = gameui() else { return };
            if dialog.is_null() {
                return;
            }
            let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
            if list.is_null() {
                return;
            }
            let mut last = FILTERED_FOR.lock().unwrap_or_else(|e| e.into_inner());
            // A refilled list shows every row again: stamp it and filter afresh.
            if stamp_demo_rows(list) {
                *last = None;
            }
            if last.as_deref() == Some(filter.as_str()) {
                return;
            }
            let first: ListFirstFn = slot(list, LIST_SLOT_FIRST_ITEM);
            let next: ListItemIdFn = slot(list, LIST_SLOT_NEXT_ITEM);
            let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
            let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
            let set_visible: ListSetVisibleFn = slot(list, LIST_SLOT_SET_ITEM_VISIBLE);
            let get_string_of = |row: *mut c_void| -> String {
                let get_string: GetStringFn = slot(row, KEYVALUES_SLOT_GET_STRING);
                let raw = get_string(row, ROW_KEY.as_ptr(), c"".as_ptr());
                if raw.is_null() {
                    String::new()
                } else {
                    text(raw)
                }
            };
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            // Header reads only when a filter needs them.
            let need_info = !filters.map.trim().is_empty()
                || !filters.hltv
                || !filters.pov
                || filters.days.is_some()
                || !filters.search.trim().is_empty();
            let mut id = first(list);
            let mut guard = 0;
            while is_valid(list, id) & 0xff != 0 && guard < 100_000 {
                guard += 1;
                let row = get_item(list, id);
                if !row.is_null() {
                    let name = get_string_of(row);
                    let info = if need_info { info_for(&name) } else { None };
                    set_visible(list, id, passes(&name, info.as_ref(), &filters, now) as u32);
                }
                id = next(list, id);
            }
            *last = Some(filter);
        }
    }

    /// Gives our Load Demo window's list Map and Date columns after the
    /// demo's name, every column sortable by a click on its heading. The name
    /// column stays the window's own (as wide as its `.res` list, so it is
    /// narrowed through its heading, which is a panel named after the column):
    /// `RemoveColumn` marks the column's sort-history byte 0xff instead of
    /// removing it, and `DeleteAllItems` then indexes column 0xff and crashes.
    unsafe fn add_demo_columns(vgui: &Vgui, dialog: *mut c_void) {
        unsafe {
            let Ok((_, build)) = gameui() else { return };
            let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
            if list.is_null() {
                return;
            }
            let add: AddColumnFn = slot(list, LIST_SLOT_ADD_COLUMN_HEADER);
            let sortable: ListIntBoolFn = slot(list, LIST_SLOT_SET_COLUMN_SORTABLE);
            let delete_all: ListVoidFn = slot(list, LIST_SLOT_DELETE_ALL_ITEMS);
            // A row remembers its place in each column's sort order by column
            // position, so columns change only while the list is empty (the
            // window filled it as it was built); `refill_demo_list` refills it.
            delete_all(list);
            for (index, (key, heading, width)) in DEMO_COLUMNS.iter().enumerate() {
                add(
                    list,
                    index as i32 + 1,
                    key.as_ptr(),
                    heading.as_ptr(),
                    *width,
                    0,
                );
            }
            if let Some(heading) = vgui.child_named(vpanel_of(list), "demoname") {
                let (x, y, _, tall) = vgui.rect(heading);
                vgui.place(heading, (x, y, NAME_COLUMN_WIDE, tall));
            }
            for index in 0..=DEMO_COLUMNS.len() as i32 {
                sortable(list, index, 1);
            }
        }
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn SystemTimeToTzSpecificLocalTime(
            zone: *const c_void,
            utc: *const [u16; 8],
            local: *mut [u16; 8],
        ) -> i32;
    }

    /// A file time as the Date column shows it, in local time (with the
    /// daylight saving of that date, as Explorer shows it).
    fn local_date(secs: u64) -> String {
        let (year, month, day, hour, minute) = civil(secs);
        let utc = [
            year as u16,
            month as u16,
            0,
            day as u16,
            hour as u16,
            minute as u16,
            0,
            0,
        ];
        let mut local = [0u16; 8];
        if unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) } == 0 {
            return date_text((year, month, day, hour, minute));
        }
        let [year, month, _, day, hour, minute, ..] = local.map(u64::from);
        date_text((year, month, day, hour, minute))
    }

    /// Fills in every row's Map and Date, when the list was (re)filled since
    /// the last time. Returns whether it did.
    unsafe fn stamp_demo_rows(list: *mut c_void) -> bool {
        unsafe {
            let first: ListFirstFn = slot(list, LIST_SLOT_FIRST_ITEM);
            let next: ListItemIdFn = slot(list, LIST_SLOT_NEXT_ITEM);
            let is_valid: ListIntFn = slot(list, LIST_SLOT_IS_VALID_ITEM_ID);
            let get_item: ListItemFn = slot(list, LIST_SLOT_GET_ITEM);
            let apply_changes: ListIntVoidFn = slot(list, LIST_SLOT_APPLY_ITEM_CHANGES);
            let get = |row: *mut c_void, key: &CStr| -> String {
                let get_string: GetStringFn = slot(row, KEYVALUES_SLOT_GET_STRING);
                let raw = get_string(row, key.as_ptr(), c"".as_ptr());
                if raw.is_null() {
                    String::new()
                } else {
                    text(raw)
                }
            };
            let id = first(list);
            if is_valid(list, id) & 0xff == 0 {
                return false;
            }
            let row = get_item(list, id);
            if row.is_null() || !get(row, STAMP_KEY).is_empty() {
                return false;
            }
            let mut id = id;
            let mut guard = 0;
            while is_valid(list, id) & 0xff != 0 && guard < 100_000 {
                guard += 1;
                let row = get_item(list, id);
                if !row.is_null() {
                    let set_string: SetStringFn = slot(row, KEYVALUES_SLOT_SET_STRING);
                    if let Some(info) = info_for(&get(row, ROW_KEY))
                        && let Ok(map) = std::ffi::CString::new(info.map.clone())
                        && let Ok(date) = std::ffi::CString::new(local_date(info.modified))
                    {
                        set_string(row, DEMO_COLUMNS[0].0.as_ptr(), map.as_ptr());
                        set_string(row, DEMO_COLUMNS[1].0.as_ptr(), date.as_ptr());
                    }
                    set_string(row, STAMP_KEY.as_ptr(), c"1".as_ptr());
                    // Each column keeps its rows sorted as they were added;
                    // re-sort this one into the new columns.
                    apply_changes(list, id);
                }
                id = next(list, id);
            }
            true
        }
    }

    /// Lists the demos again in our Load Demo window (new recordings, or a
    /// folder change), with the window's own fill.
    unsafe fn refill_demo_list() {
        let dialog = DEMO_DIALOG.load(Ordering::Acquire) as *mut c_void;
        if dialog.is_null() {
            return;
        }
        if let Ok((base, build)) = gameui() {
            unsafe {
                let fill: ActivateFn = std::mem::transmute(base + build.file_dialog_fill);
                fill(dialog);
            }
            // Every row shows again: filter afresh.
            *FILTERED_FOR.lock().unwrap_or_else(|e| e.into_inner()) = None;
        }
    }

    unsafe fn slot<F: Copy>(object: *mut c_void, index: usize) -> F {
        unsafe {
            let vftable = *(object as *const *const usize);
            std::mem::transmute_copy(&*vftable.add(index))
        }
    }

    fn module(name: &CStr) -> Option<usize> {
        let handle = unsafe { GetModuleHandleA(name.as_ptr() as *const u8) };
        (!handle.is_null()).then_some(handle as usize)
    }

    fn interface(module: usize, name: &CStr) -> Option<*mut c_void> {
        let create =
            unsafe { GetProcAddress(module as _, c"CreateInterface".as_ptr() as *const u8) }?;
        // Safety: the interface factory's signature.
        let create: CreateInterfaceFn = unsafe { std::mem::transmute(create) };
        let object = unsafe { create(name.as_ptr(), std::ptr::null_mut()) };
        (!object.is_null()).then_some(object)
    }

    fn text(raw: *const c_char) -> String {
        if raw.is_null() {
            String::new()
        } else {
            // Safety: a NUL-terminated string the panel system owns.
            unsafe { CStr::from_ptr(raw) }
                .to_string_lossy()
                .into_owned()
        }
    }

    struct Vgui {
        panel: *mut c_void,
        surface: *mut c_void,
    }

    impl Vgui {
        fn get() -> Result<Self, String> {
            let vgui2 = module(c"vgui2.dll").ok_or("vgui2.dll is not loaded")?;
            let hw = module(c"hw.dll").ok_or("hw.dll is not loaded")?;
            Ok(Self {
                panel: interface(vgui2, c"VGUI_Panel007").ok_or("no VGUI_Panel007")?,
                surface: interface(hw, c"VGUI_Surface026").ok_or("no VGUI_Surface026")?,
            })
        }

        unsafe fn name(&self, vp: Vpanel) -> String {
            let name: StrFn = unsafe { slot(self.panel, IPANEL_GET_NAME) };
            text(unsafe { name(self.panel, vp) })
        }

        unsafe fn module_name(&self, vp: Vpanel) -> *const c_char {
            let module: StrFn = unsafe { slot(self.panel, IPANEL_GET_MODULE_NAME) };
            unsafe { module(self.panel, vp) }
        }

        unsafe fn object(&self, vp: Vpanel) -> *mut c_void {
            let get: GetPanelFn = unsafe { slot(self.panel, IPANEL_GET_PANEL) };
            unsafe { get(self.panel, vp, self.module_name(vp)) }
        }

        /// GameUI's popups (its dialogs), by panel name.
        unsafe fn gameui_popups(&self) -> Vec<(Vpanel, String)> {
            unsafe {
                let count: CountFn = slot(self.surface, SURFACE_GET_POPUP_COUNT);
                let popup: PopupFn = slot(self.surface, SURFACE_GET_POPUP);
                (0..count(self.surface).clamp(0, 256))
                    .map(|i| popup(self.surface, i))
                    .filter(|&vp| vp != 0 && text(self.module_name(vp)) == "GameUI")
                    .map(|vp| (vp, self.name(vp)))
                    .collect()
            }
        }

        unsafe fn child_list(&self, vp: Vpanel) -> Vec<Vpanel> {
            unsafe {
                let count: PanelIntFn = slot(self.panel, IPANEL_GET_CHILD_COUNT);
                let child: ChildFn = slot(self.panel, IPANEL_GET_CHILD);
                (0..count(self.panel, vp).clamp(0, 512))
                    .map(|i| child(self.panel, vp, i))
                    .filter(|&c| c != 0)
                    .collect()
            }
        }

        unsafe fn children(&self, vp: Vpanel) -> usize {
            unsafe { self.child_list(vp).len() }
        }

        unsafe fn child_named(&self, vp: Vpanel, name: &str) -> Option<Vpanel> {
            unsafe {
                self.child_list(vp)
                    .into_iter()
                    .find(|&c| self.name(c) == name)
            }
        }

        unsafe fn parent_of(&self, vp: Vpanel) -> Vpanel {
            let parent: ParentFn = unsafe { slot(self.panel, IPANEL_GET_PARENT) };
            unsafe { parent(self.panel, vp) }
        }

        unsafe fn visible(&self, vp: Vpanel) -> bool {
            let visible: PanelBoolFn = unsafe { slot(self.panel, IPANEL_IS_VISIBLE) };
            unsafe { visible(self.panel, vp) & 0xff != 0 }
        }

        unsafe fn set_visible(&self, vp: Vpanel, on: bool) {
            let set: PanelSetBoolFn = unsafe { slot(self.panel, IPANEL_SET_VISIBLE) };
            unsafe { set(self.panel, vp, on as u32) };
        }

        /// Moves and sizes `vp`, writing only what differs.
        unsafe fn place(&self, vp: Vpanel, want: (i32, i32, i32, i32)) {
            unsafe {
                let now = self.rect(vp);
                if (now.0, now.1) != (want.0, want.1) {
                    let set_pos: XyFn = slot(self.panel, IPANEL_SET_POS);
                    set_pos(self.panel, vp, want.0, want.1);
                }
                if (now.2, now.3) != (want.2, want.3) {
                    let set_size: XyFn = slot(self.panel, IPANEL_SET_SIZE);
                    set_size(self.panel, vp, want.2, want.3);
                }
            }
        }

        /// A panel's position on screen.
        unsafe fn abs_pos(&self, vp: Vpanel) -> (i32, i32) {
            unsafe {
                let get: GetXyFn = slot(self.panel, IPANEL_GET_ABS_POS);
                let (mut x, mut y) = (0, 0);
                get(self.panel, vp, &mut x, &mut y);
                (x, y)
            }
        }

        unsafe fn set_parent(&self, vp: Vpanel, parent: Vpanel) {
            let set: SetParentFn = unsafe { slot(self.panel, IPANEL_SET_PARENT) };
            unsafe { set(self.panel, vp, parent) };
        }

        unsafe fn popup(&self, wanted: &str) -> Option<Vpanel> {
            unsafe {
                self.gameui_popups()
                    .into_iter()
                    .find(|(_, name)| name == wanted)
                    .map(|(vp, _)| vp)
            }
        }

        unsafe fn bar(&self) -> Option<Vpanel> {
            unsafe { self.popup(VCR_BAR) }
        }
    }

    unsafe fn vpanel_of(object: *mut c_void) -> Vpanel {
        if object.is_null() {
            return 0;
        }
        unsafe {
            let get_vpanel: GetVpanelFn = slot(object, PANEL_SLOT_GET_VPANEL);
            get_vpanel(object)
        }
    }

    /// One of another window's controls, moved onto one of our tabs.
    struct Borrowed {
        control: Vpanel,
        /// The window it came from (by name and panel), and where it sat there.
        source_name: &'static str,
        source: Vpanel,
        home: (i32, i32, i32, i32),
    }

    struct Lent {
        borrowed: Vec<Borrowed>,
        /// The bar's own place while it is parked off screen.
        parked_from: Option<(Vpanel, i32, i32)>,
        /// The minimum size last set on the window.
        minimum: (i32, i32),
        /// Loans already reported missing, so each is logged once.
        reported: Vec<&'static str>,
        /// Where the console window was before we moved it (see `borrow`).
        console_home: Option<(Vpanel, i32, i32)>,
        /// The console's own offset for its type-ahead list, once measured,
        /// and where we last put the list.
        list_offset: Option<(i32, i32)>,
        list_set: Option<(i32, i32)>,
        /// The help line's text, as last set.
        help_shown: Option<String>,
        /// Each bound check box's state as last seen, so a click is told
        /// apart from the cvar changing under it.
        boxes: Vec<(Vpanel, bool)>,
        /// Each tab's size and its controls' places as its `.res` laid them
        /// out, read the first time the tab has a size.
        designs: Vec<PageDesign>,
    }

    struct PageDesign {
        page: Vpanel,
        /// The tab's size the `.res` was laid out for: unknown until the
        /// sheet has laid the tab out, then worked back to the window's own
        /// `.res` size (#410 may have resized the window already).
        size: Option<(i32, i32)>,
        /// Each control where the `.res` put it, read straight after loading.
        controls: Vec<(Vpanel, (i32, i32, i32, i32))>,
    }

    thread_local! {
        // Only the engine's main thread polls, so no lock is needed.
        static LENT: std::cell::RefCell<Lent> = const {
            std::cell::RefCell::new(Lent {
                borrowed: Vec::new(),
                parked_from: None,
                minimum: (0, 0),
                reported: Vec::new(),
                console_home: None,
                list_offset: None,
                list_set: None,
                boxes: Vec::new(),
                help_shown: None,
                designs: Vec::new(),
            })
        };
    }

    impl Vgui {
        unsafe fn rect(&self, vp: Vpanel) -> (i32, i32, i32, i32) {
            unsafe {
                let get_pos: GetXyFn = slot(self.panel, IPANEL_GET_POS);
                let get_size: GetXyFn = slot(self.panel, IPANEL_GET_SIZE);
                let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
                get_pos(self.panel, vp, &mut x, &mut y);
                get_size(self.panel, vp, &mut w, &mut h);
                (x, y, w, h)
            }
        }

        /// Where a panel is and what holds it, for the log.
        unsafe fn describe(&self, vp: Vpanel) -> String {
            unsafe {
                let visible: PanelBoolFn = slot(self.panel, IPANEL_IS_VISIBLE);
                let parent: ParentFn = slot(self.panel, IPANEL_GET_PARENT);
                let (x, y, w, h) = self.rect(vp);
                let p = parent(self.panel, vp);
                format!(
                    "at {x},{y} {w}x{h}, visible {}, {} child panel(s), parent {:?}",
                    visible(self.panel, vp) & 0xff,
                    self.children(vp),
                    self.name(p),
                )
            }
        }
    }

    fn gameui() -> Result<(usize, &'static Build), String> {
        let base = module(c"GameUI.dll").ok_or("GameUI.dll is not loaded yet")?;
        // Safety: a module handle the loader gave us.
        let identity = unsafe { crate::pe::image_identity(base as *mut u8) };
        BUILDS
            .iter()
            .find(|b| identity == Some((b.time_date_stamp, b.size_of_image)))
            .map(|b| (base, b))
            .ok_or_else(|| "GameUI.dll is a build this was not checked against".to_string())
    }

    /// The panel GameUI's own dialogs hang from: `TaskBar`, the parent of its
    /// main menu (`GameMenu`) and of the VCR bar on both builds (listed live,
    /// 2026-10-01). Taking just any GameUI popup's parent picked a dialog's
    /// inner panel once, and the window never showed.
    unsafe fn base_panel(vgui: &Vgui) -> Result<*mut c_void, String> {
        unsafe {
            let parent: ParentFn = slot(vgui.panel, IPANEL_GET_PARENT);
            for (vp, _) in vgui.gameui_popups() {
                let p = parent(vgui.panel, vp);
                if p == 0 || vgui.name(p) != TASKBAR || text(vgui.module_name(p)) != "GameUI" {
                    continue;
                }
                let object = vgui.object(p);
                if !object.is_null() {
                    return Ok(object);
                }
            }
        }
        Err("GameUI's menu isn't up yet -- open it (ESC) once, then try again".to_string())
    }

    /// Gives `object` a copy of its vftable with `OnCommand` pointing at
    /// `handler`, and returns the class's own `OnCommand`. The copy (RTTI
    /// locator at [-1] included) is leaked: it lives as long as the object.
    unsafe fn own_on_command(object: *mut c_void, handler: usize) -> usize {
        unsafe { own_slots(object, &[(FRAME_SLOT_ON_COMMAND, handler)])[0] }
    }

    /// Gives `object` a copy of its vftable with each `(slot, handler)`
    /// replaced, and returns the class's own functions for those slots, in
    /// order. The copy (RTTI locator at [-1] included) is leaked: it lives as
    /// long as the object.
    unsafe fn own_slots(object: *mut c_void, replace: &[(usize, usize)]) -> Vec<usize> {
        unsafe {
            let original = *(object as *const *const usize);
            let mut copy = vec![0usize; VFTABLE_SLOTS + 1];
            for (i, entry) in copy.iter_mut().enumerate() {
                *entry = *original.offset(i as isize - 1);
            }
            let own = replace
                .iter()
                .map(|&(index, handler)| {
                    let own = copy[index + 1];
                    copy[index + 1] = handler;
                    own
                })
                .collect();
            let copy: &'static mut [usize] = Box::leak(copy.into_boxed_slice());
            *(object as *mut *const usize) = copy.as_ptr().add(1);
            own
        }
    }

    /// GameUI's `operator new`, zeroed.
    unsafe fn allocate(base: usize, build: &Build, size: usize) -> Result<*mut c_void, String> {
        unsafe {
            let new: OperatorNewFn = std::mem::transmute(base + build.operator_new);
            let object = new(size);
            if object.is_null() {
                return Err("GameUI's operator new returned null".to_string());
            }
            std::ptr::write_bytes(object as *mut u8, 0, size);
            Ok(object)
        }
    }

    /// Builds the window, its tab strip and its pages.
    unsafe fn build() -> Result<(*mut c_void, *mut c_void), String> {
        let (base, build) = gameui()?;
        let vgui = Vgui::get()?;
        let parent = unsafe { base_panel(&vgui) }?;
        unsafe {
            let load: LoadSettingsFn = std::mem::transmute(base + build.load_control_settings);

            let frame = allocate(base, build, build.frame_size)?;
            if build.frame_ctor_fourth_arg {
                let ctor: FrameCtor4 = std::mem::transmute(base + build.frame_ctor);
                ctor(frame, parent, PANEL_NAME.as_ptr(), 1, 0);
            } else {
                let ctor: FrameCtor3 = std::mem::transmute(base + build.frame_ctor);
                ctor(frame, parent, PANEL_NAME.as_ptr(), 1);
            }
            FRAME_ON_COMMAND.store(
                own_on_command(frame, frame_on_command as *const () as usize),
                Ordering::Release,
            );
            load(frame, WINDOW_RES.0.as_ptr(), std::ptr::null());
            let vgui = Vgui::get()?;
            let (_, _, fw, fh) = vgui.rect(vpanel_of(frame));
            *WINDOW_DESIGN.lock().unwrap_or_else(|e| e.into_inner()) = Some((fw, fh));
            let mut designs = Vec::new();

            let sheet = allocate(base, build, build.sheet_size)?;
            let sheet_ctor: SheetCtor = std::mem::transmute(base + build.sheet_ctor);
            sheet_ctor(sheet, frame, SHEET_NAME.as_ptr());

            let page_ctor: PageCtor = std::mem::transmute(base + build.page_ctor);
            let add_page: AddPageFn = slot(sheet, SHEET_SLOT_ADD_PAGE);
            for (page, stored) in PAGES.iter().zip(&PAGE_OBJECTS) {
                let object = allocate(base, build, PAGE_ALLOC)?;
                page_ctor(object, frame, page.name.as_ptr(), 1);
                let own = own_slots(
                    object,
                    &[
                        (FRAME_SLOT_ON_COMMAND, page_on_command as *const () as usize),
                        (
                            PANEL_SLOT_ON_KEY_CODE_TYPED,
                            page_on_key as *const () as usize,
                        ),
                        (
                            PANEL_SLOT_ON_KEY_CODE_PRESSED,
                            page_on_key_pressed as *const () as usize,
                        ),
                    ],
                );
                PAGE_ON_COMMAND.store(own[0], Ordering::Release);
                PAGE_ON_KEY.store(own[1], Ordering::Release);
                PAGE_ON_KEY_PRESSED.store(own[2], Ordering::Release);
                load(object, page.res.0.as_ptr(), std::ptr::null());
                // Where the .res put each control, before anything resizes it.
                let page_vp = vpanel_of(object);
                designs.push(PageDesign {
                    page: page_vp,
                    size: None,
                    controls: vgui
                        .child_list(page_vp)
                        .into_iter()
                        .map(|c| (c, vgui.rect(c)))
                        .collect(),
                });
                add_page(sheet, object, page.title.as_ptr());
                stored.store(object as usize, Ordering::Release);
            }
            LENT.with(|cell| {
                if let Ok(mut lent) = cell.try_borrow_mut() {
                    lent.designs = designs;
                }
            });

            // The Commands tab's text, longer than a .res value may be.
            let commands_page =
                vpanel_of(PAGE_OBJECTS[COMMANDS_PAGE].load(Ordering::Acquire) as *mut c_void);
            if let Some(list) = vgui.child_named(commands_page, COMMAND_LIST) {
                let object = vgui.object(list);
                if !object.is_null() {
                    let wide: Vec<u16> = COMMANDS_TEXT.encode_utf16().chain([0]).collect();
                    let set_text: SetWideTextFn =
                        std::mem::transmute(base + build.rich_text_set_text_wide);
                    set_text(object, wide.as_ptr());
                }
            }

            // The Demos tab's HLTV / POV boxes start ticked: show everything.
            let demos_page =
                vpanel_of(PAGE_OBJECTS[DEMOS_PAGE].load(Ordering::Acquire) as *mut c_void);
            for name in [SHOW_HLTV, SHOW_POV] {
                if let Some(o) = vgui
                    .child_named(demos_page, name)
                    .map(|vp| vgui.object(vp))
                    .filter(|o| {
                        !o.is_null() && *(*o as *const usize) == base + build.check_button_vftable
                    })
                {
                    let set_selected: SetSelectedFn = slot(o, BUTTON_SLOT_SET_SELECTED);
                    set_selected(o, 1);
                }
            }

            // Our own Load Demo window, never shown: the Demos tab borrows its
            // list and Load button. It fills its list as it is built.
            let dialog = allocate(base, build, build.frame_size + 4)?;
            let dialog_ctor: SheetCtor = std::mem::transmute(base + build.file_dialog_ctor);
            dialog_ctor(dialog, frame, c"DodStudioDemoList".as_ptr());
            vgui.set_visible(vpanel_of(dialog), false);
            DEMO_LIST_ON_COMMAND.store(
                own_on_command(dialog, demo_list_on_command as *const () as usize),
                Ordering::Release,
            );
            DEMO_DIALOG.store(dialog as usize, Ordering::Release);
            add_demo_columns(&vgui, dialog);
            refill_demo_list();
            Ok((frame, sheet))
        }
    }

    /// The window's panel, if it still exists as the object we built.
    unsafe fn window(vgui: &Vgui) -> Option<(*mut c_void, Vpanel)> {
        let object = OBJECT.load(Ordering::Acquire) as *mut c_void;
        if object.is_null() {
            return None;
        }
        unsafe {
            let get_vpanel: GetVpanelFn = slot(object, PANEL_SLOT_GET_VPANEL);
            let vp = get_vpanel(object);
            (vp != 0 && vgui.object(vp) == object).then_some((object, vp))
        }
    }

    /// Sizes the tab strip to the window's client area, as
    /// `PropertyDialog::PerformLayout` does for its own. Writes only when it
    /// differs.
    unsafe fn fit_sheet(vgui: &Vgui, frame: *mut c_void) {
        let sheet = SHEET.load(Ordering::Acquire) as *mut c_void;
        if sheet.is_null() {
            return;
        }
        unsafe {
            let client_area: ClientAreaFn = slot(frame, FRAME_SLOT_GET_CLIENT_AREA);
            let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
            client_area(frame, &mut x, &mut y, &mut w, &mut h);
            let get_vpanel: GetVpanelFn = slot(sheet, PANEL_SLOT_GET_VPANEL);
            let vp = get_vpanel(sheet);
            if vp == 0 {
                return;
            }
            let want = sheet_bounds((x, y, w, h));
            let now = vgui.rect(vp);
            if (now.0, now.1) != (want.0, want.1) {
                let set_pos: XyFn = slot(vgui.panel, IPANEL_SET_POS);
                set_pos(vgui.panel, vp, want.0, want.1);
            }
            if (now.2, now.3) != (want.2, want.3) {
                let set_size: XyFn = slot(vgui.panel, IPANEL_SET_SIZE);
                set_size(vgui.panel, vp, want.2, want.3);
            }
            let frame_vp = vpanel_of(frame);
            if let Some(line) = vgui.child_named(frame_vp, HELP_LINE) {
                vgui.place(line, help_bounds(want));
            }
        }
    }

    /// Keeps the window no narrower than its tabs: the tab strip neither
    /// squeezes nor wraps them, it cuts off whatever passes its right edge.
    unsafe fn hold_minimum(vgui: &Vgui, frame: *mut c_void, vp: Vpanel, lent: &mut Lent) {
        unsafe {
            let sheet_vp = vpanel_of(SHEET.load(Ordering::Acquire) as *mut c_void);
            if sheet_vp == 0 {
                return;
            }
            let pages: Vec<Vpanel> = PAGE_OBJECTS
                .iter()
                .map(|p| vpanel_of(p.load(Ordering::Acquire) as *mut c_void))
                .collect();
            // The sheet's children other than the pages are its tabs.
            let tabs_right = vgui
                .child_list(sheet_vp)
                .into_iter()
                .filter(|c| !pages.contains(c))
                .map(|c| {
                    let (x, _, w, _) = vgui.rect(c);
                    x + w
                })
                .max()
                .unwrap_or(0);
            if tabs_right <= 0 {
                return;
            }
            let client_area: ClientAreaFn = slot(frame, FRAME_SLOT_GET_CLIENT_AREA);
            let (mut x, mut y, mut w, mut h) = (0, 0, 0, 0);
            client_area(frame, &mut x, &mut y, &mut w, &mut h);
            let _ = (x, y, h);
            let want = minimum_size(tabs_right, vgui.rect(vp).2, w);
            if want != lent.minimum {
                let set_min: XyFn = slot(vgui.panel, IPANEL_SET_MINIMUM_SIZE);
                set_min(vgui.panel, vp, want.0, want.1);
                lent.minimum = want;
            }
            // Already narrower than that (it was resized before this ran):
            // widen it once.
            let now = vgui.rect(vp);
            if now.2 < want.0 {
                vgui.place(vp, (now.0, now.1, want.0, now.3.max(want.1)));
            }
        }
    }

    /// Moves `source_name`'s controls in [`LOANS`] onto their tabs, into
    /// their slots. Re-asserted every frame: the source may lay them out again.
    unsafe fn borrow(vgui: &Vgui, source_name: &'static str, source: Vpanel, lent: &mut Lent) {
        unsafe {
            for loan in LOANS.iter().filter(|l| l.source == source_name) {
                let page =
                    vpanel_of(PAGE_OBJECTS[loan.page].load(Ordering::Acquire) as *mut c_void);
                if page == 0 {
                    continue;
                }
                // No slot: the control only needs our window as its parent.
                let slot_vp = if loan.slot.is_empty() {
                    None
                } else {
                    match vgui.child_named(page, loan.slot) {
                        Some(vp) => Some(vp),
                        None => continue, // a layout without this slot: nothing borrowed
                    }
                };
                if let Some(slot_vp) = slot_vp
                    && vgui.visible(slot_vp)
                {
                    vgui.set_visible(slot_vp, false);
                }
                let at = slot_vp.map(|v| vgui.rect(v));
                let parent = match slot_vp {
                    Some(_) => page,
                    None => match window(vgui) {
                        Some((_, frame_vp)) => frame_vp,
                        None => continue,
                    },
                };
                let known = lent
                    .borrowed
                    .iter()
                    .find(|b| b.source == source && vgui.name(b.control) == loan.control)
                    .map(|b| b.control);
                let vp = match known.or_else(|| vgui.child_named(source, loan.control)) {
                    Some(vp) => vp,
                    None => {
                        if !lent.reported.contains(&loan.control) {
                            lent.reported.push(loan.control);
                            let names: Vec<String> = vgui
                                .child_list(source)
                                .into_iter()
                                .map(|c| vgui.name(c))
                                .collect();
                            crate::debug::report(&format!(
                                "studio_panel: {source_name} has no control {:?} to lend; it has {names:?}",
                                loan.control
                            ));
                        }
                        continue;
                    }
                };
                if known.is_none() {
                    lent.borrowed.push(Borrowed {
                        control: vp,
                        source_name,
                        source,
                        home: vgui.rect(vp),
                    });
                }
                if vgui.parent_of(vp) != parent {
                    vgui.set_parent(vp, parent);
                }
                if let Some(at) = at {
                    vgui.place(vp, at);
                } else if loan.control == TYPE_AHEAD {
                    // The console places its list under its own window's
                    // input line; put it under ours instead (screen
                    // coordinates: it is a popup).
                    let entry = lent
                        .borrowed
                        .iter()
                        .find(|b| b.source == source && vgui.name(b.control) == CONSOLE_ENTRY)
                        .map(|b| b.control);
                    if let Some(entry) = entry {
                        let (ex, ey) = vgui.abs_pos(entry);
                        let (lx, ly, _, eh) = vgui.rect(entry);
                        let (px, py, w, h) = vgui.rect(vp);
                        let want = (ex, ey + eh);
                        // The console puts its list at its own window's place
                        // plus the input line's place in its parent, plus an
                        // offset of its own (0, 0x20 on pre-Anniversary).
                        // Whenever the list is somewhere we didn't put it,
                        // that was the console: measure the offset then.
                        let (sx, sy) = vgui.abs_pos(source);
                        if lent.list_set != Some((px, py)) && (px, py) != want {
                            lent.list_offset = Some((px - sx - lx, py - sy - ly));
                        }
                        // Then move the (hidden) console window so its own
                        // placement lands under our input line: the list stops
                        // jumping to the old place for a frame on every key.
                        if let Some((ox, oy)) = lent.list_offset {
                            if lent.console_home.is_none() {
                                let (hx, hy, _, _) = vgui.rect(source);
                                lent.console_home = Some((source, hx, hy));
                            }
                            let (_, _, cw, ch) = vgui.rect(source);
                            vgui.place(source, (want.0 - lx - ox, want.1 - ly - oy, cw, ch));
                        }
                        vgui.place(vp, (want.0, want.1, w, h));
                        lent.list_set = Some(want);
                        // Showing, the list takes the keyboard (a popup of
                        // ours now, not of the console's window), so typing
                        // stopped after one letter: hand it back to the
                        // input line while the list is up.
                        // A popup, so it stays up when its tab is switched
                        // away from: hide it whenever the Console tab isn't
                        // the one showing (the sheet hides the other pages).
                        if vgui.visible(vp) && !vgui.visible(page) {
                            vgui.set_visible(vp, false);
                        }
                        if vgui.visible(vp) {
                            // Above our window, which a click brings forward.
                            let front: PanelFn = slot(vgui.panel, IPANEL_MOVE_TO_FRONT);
                            front(vgui.panel, vp);
                            let keyboard: PanelSetBoolFn =
                                slot(vgui.panel, IPANEL_SET_KEYBOARD_INPUT_ENABLED);
                            keyboard(vgui.panel, vp, 0);
                            let focus: SetParentFn = slot(vgui.panel, IPANEL_REQUEST_FOCUS);
                            focus(vgui.panel, entry, 0);
                        }
                    }
                }
            }
        }
    }

    /// Keeps each tab's controls fitted to the tab's size, by #410's rule: a
    /// control spanning at least half the tab stretches with it, one near the
    /// right or bottom edge keeps its distance from it, the rest stay. The
    /// empty slots follow too, so what is borrowed into them does -- the
    /// console's history grows with the window.
    unsafe fn fit_pages(vgui: &Vgui, lent: &mut Lent) {
        unsafe {
            for stored in &PAGE_OBJECTS {
                let page = vpanel_of(stored.load(Ordering::Acquire) as *mut c_void);
                if page == 0 {
                    continue;
                }
                let (_, _, w, h) = vgui.rect(page);
                if w <= 0 || h <= 0 {
                    continue;
                }
                let sheet = vpanel_of(SHEET.load(Ordering::Acquire) as *mut c_void);
                let window_now = window(vgui).map(|(_, vp)| vgui.rect(vp));
                let window_design = *WINDOW_DESIGN.lock().unwrap_or_else(|e| e.into_inner());
                let Some(design) = lent.designs.iter_mut().find(|d| d.page == page) else {
                    continue;
                };
                if design.size.is_none() {
                    // Laid out by the sheet: nearly as wide as it, and as tall
                    // as it less the tab row.
                    let (_, _, sw, sh) = vgui.rect(sheet);
                    let (Some((_, _, fw, fh)), Some((dw, dh))) = (window_now, window_design) else {
                        continue;
                    };
                    if w < sw - 24 || h < sh - 64 {
                        continue;
                    }
                    design.size = Some((w - (fw - dw), h - (fh - dh)));
                }
                let Some(size) = design.size else { continue };
                for &(control, at) in &design.controls {
                    let want = crate::window_layout::fit_rect(at, size, (w, h));
                    vgui.place(control, want);
                }
            }
        }
    }

    /// Shows each tab's "use this tab" button while its setting is off, and
    /// hides it while on.
    unsafe fn show_enable_buttons(vgui: &Vgui) {
        unsafe {
            for (button, page, source) in ENABLE_BUTTONS {
                let page = vpanel_of(PAGE_OBJECTS[*page].load(Ordering::Acquire) as *mut c_void);
                if page == 0 {
                    continue;
                }
                if let Some(vp) = vgui.child_named(page, button) {
                    let want = !lends(source);
                    if vgui.visible(vp) != want {
                        vgui.set_visible(vp, want);
                    }
                }
            }
        }
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetCursorPos(point: *mut [i32; 2]) -> i32;
        fn ScreenToClient(hwnd: *mut c_void, point: *mut [i32; 2]) -> i32;
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(hwnd: *mut c_void, pid: *mut u32) -> u32;
    }

    /// The mouse, in the game window's own pixels (vgui's screen), while the
    /// game window is the one in front.
    fn cursor() -> Option<(i32, i32)> {
        unsafe {
            let hwnd = GetForegroundWindow();
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if hwnd.is_null() || pid != std::process::id() {
                return None;
            }
            let mut point = [0i32; 2];
            (GetCursorPos(&mut point) != 0 && ScreenToClient(hwnd, &mut point) != 0)
                .then_some((point[0], point[1]))
        }
    }

    /// Shows, along the bottom of the window, the help text of the control
    /// under the mouse.
    unsafe fn update_help(vgui: &Vgui, frame_vp: Vpanel, lent: &mut Lent) {
        unsafe {
            let Some(line) = vgui.child_named(frame_vp, HELP_LINE) else {
                return;
            };
            let mut text = String::new();
            if let Some((mx, my)) = cursor() {
                let shown = PAGES.iter().enumerate().find_map(|(i, page)| {
                    let vp = vpanel_of(PAGE_OBJECTS[i].load(Ordering::Acquire) as *mut c_void);
                    (vp != 0 && vgui.visible(vp)).then_some((i, page, vp))
                });
                if let Some((index, page, page_vp)) = shown {
                    let over = vgui
                        .child_list(page_vp)
                        .into_iter()
                        .filter(|&c| vgui.visible(c))
                        .filter(|&c| {
                            let (ax, ay) = vgui.abs_pos(c);
                            let (_, _, w, h) = vgui.rect(c);
                            mx >= ax && mx < ax + w && my >= ay && my < ay + h
                        })
                        .min_by_key(|&c| {
                            let (_, _, w, h) = vgui.rect(c);
                            w * h
                        });
                    if let Some(over) = over {
                        let name = vgui.name(over);
                        // A lent control's help is its slot's.
                        let key = LOANS
                            .iter()
                            .find(|l| l.page == index && l.control == name && !l.slot.is_empty())
                            .map_or(name.clone(), |l| l.slot.to_string());
                        let path = res_dir().join(page.res.1);
                        let res = std::fs::read_to_string(path).unwrap_or_default();
                        text = tooltips(&res)
                            .into_iter()
                            .find(|(n, _)| n.eq_ignore_ascii_case(&key))
                            .map(|(_, t)| t)
                            .unwrap_or_default();
                    }
                }
            }
            if lent.help_shown.as_deref() != Some(text.as_str()) {
                let object = vgui.object(line);
                if !object.is_null() {
                    let set_text: SetTextFn = slot(object, LABEL_SLOT_SET_TEXT);
                    if let Ok(c_text) = std::ffi::CString::new(text.clone()) {
                        set_text(object, c_text.as_ptr());
                    }
                }
                lent.help_shown = Some(text);
            }
        }
    }

    /// Keeps every `cvar_<name>` check box on the Settings tab and its cvar
    /// in step: a click sets the cvar, and a cvar set elsewhere (the console,
    /// a config) moves the box.
    unsafe fn sync_settings(vgui: &Vgui, lent: &mut Lent) {
        let Ok((base, build)) = gameui() else { return };
        let Some(engfuncs) = crate::engine::engfuncs() else {
            return;
        };
        unsafe {
            let page =
                vpanel_of(PAGE_OBJECTS[SETTINGS_PAGE].load(Ordering::Acquire) as *mut c_void);
            if page == 0 {
                return;
            }
            for control in vgui.child_list(page) {
                let name = vgui.name(control);
                let Some(cvar) = bound_cvar(&name) else {
                    continue;
                };
                let object = vgui.object(control);
                if object.is_null()
                    || *(object as *const usize) != base + build.check_button_vftable
                {
                    continue;
                }
                let is_selected: IsSelectedFn = slot(object, BUTTON_SLOT_IS_SELECTED);
                let set_selected: SetSelectedFn = slot(object, BUTTON_SLOT_SET_SELECTED);
                let checked = is_selected(object) & 0xff != 0;
                let Ok(c_name) = std::ffi::CString::new(cvar) else {
                    continue;
                };
                let cvar_on = (engfuncs.pfn_get_cvar_float)(c_name.as_ptr()) != 0.0;
                let last = lent
                    .boxes
                    .iter()
                    .find(|(c, _)| *c == control)
                    .map(|(_, v)| *v);
                let (set, redraw) = settle(checked, last, cvar_on);
                let now = match set {
                    Some(value) => {
                        if let Ok(line) =
                            std::ffi::CString::new(format!("{cvar} {}\n", value as u8))
                        {
                            crate::engine::client_cmd(&line);
                        }
                        let before = (engfuncs.pfn_get_cvar_float)(c_name.as_ptr());
                        save_setting(cvar, if value { "1" } else { "0" }, before);
                        value
                    }
                    None if redraw => {
                        set_selected(object, cvar_on as u32);
                        cvar_on
                    }
                    None => checked,
                };
                match lent.boxes.iter_mut().find(|(c, _)| *c == control) {
                    Some(entry) => entry.1 = now,
                    None => lent.boxes.push((control, now)),
                }
            }
        }
    }

    /// Hands borrowed controls back to the windows they came from, where they
    /// were: all of them, or only those from `only`.
    unsafe fn give_back(vgui: &Vgui, lent: &mut Lent, only: Option<&str>) {
        let (back, keep): (Vec<Borrowed>, Vec<Borrowed>) = lent
            .borrowed
            .drain(..)
            .partition(|b| only.is_none_or(|name| b.source_name == name));
        lent.borrowed = keep;
        if only.is_none_or(|name| name == CONSOLE)
            && let Some((console, x, y)) = lent.console_home.take()
        {
            unsafe {
                if !vgui.object(console).is_null() {
                    let (_, _, w, h) = vgui.rect(console);
                    vgui.place(console, (x, y, w, h));
                }
            }
        }
        for b in back {
            unsafe {
                if vgui.object(b.source).is_null() {
                    continue; // that window is gone
                }
                vgui.set_parent(b.control, b.source);
                vgui.place(b.control, b.home);
            }
        }
    }

    unsafe fn park(vgui: &Vgui, bar: Vpanel, lent: &mut Lent) {
        unsafe {
            let (x, y, w, h) = vgui.rect(bar);
            if lent.parked_from.is_none() && x != PARKED_AT {
                lent.parked_from = Some((bar, x, y));
            }
            vgui.place(bar, (PARKED_AT, PARKED_AT, w, h));
        }
        PARKED.store(bar, Ordering::Release);
    }

    unsafe fn unpark(vgui: &Vgui, lent: &mut Lent) {
        PARKED.store(0, Ordering::Release);
        if let Some((bar, x, y)) = lent.parked_from.take() {
            unsafe {
                if vgui.object(bar).is_null() {
                    return;
                }
                let (_, _, w, h) = vgui.rect(bar);
                vgui.place(bar, (x, y, w, h));
            }
        }
    }

    /// Whether our window is showing its Console tab; if so, closes it.
    pub(super) fn close_if_on_console() -> bool {
        let Ok(vgui) = Vgui::get() else { return false };
        unsafe {
            let Some((_, vp)) = window(&vgui) else {
                return false;
            };
            let sheet = SHEET.load(Ordering::Acquire) as *mut c_void;
            if !vgui.visible(vp) || sheet.is_null() {
                return false;
            }
            let active: GetActivePageFn = slot(sheet, SHEET_SLOT_GET_ACTIVE_PAGE);
            let console = PAGE_OBJECTS[CONSOLE_PAGE].load(Ordering::Acquire) as *mut c_void;
            if active(sheet) != console {
                return false;
            }
            vgui.set_visible(vp, false);
            true
        }
    }

    /// After the console key: once the console window is up, hides it and
    /// opens our window on the Console tab, its input line focused.
    unsafe fn console_to_tab(vgui: &Vgui) {
        let pending = CONSOLE_PENDING.load(Ordering::Relaxed);
        if pending == 0 {
            return;
        }
        CONSOLE_PENDING.store(pending - 1, Ordering::Relaxed);
        unsafe {
            let Some(console) = vgui.popup(CONSOLE).filter(|&c| vgui.visible(c)) else {
                return;
            };
            CONSOLE_PENDING.store(0, Ordering::Relaxed);
            vgui.set_visible(console, false);
            let line = match ensure_window(vgui, false)
                .and_then(|(object, vp, _)| show(vgui, object, vp, Some(CONSOLE_PAGE)))
            {
                Ok(state) => format!("the console key opened the Console tab -- {state}"),
                Err(why) => format!("the console key could not open the Console tab -- {why}"),
            };
            // The input line gets the keyboard once it is on the tab and the
            // window's own Activate has run: a few frames from now.
            FOCUS_ENTRY.store(FOCUS_ENTRY_FRAMES, Ordering::Relaxed);
            crate::debug::report(&format!("studio_panel: {line}"));
        }
    }

    pub(super) fn poll() {
        let pending = VIEWDEMO_PENDING.load(Ordering::Relaxed);
        if OBJECT.load(Ordering::Relaxed) == 0
            && pending == 0
            && CONSOLE_PENDING.load(Ordering::Relaxed) == 0
        {
            return;
        }
        let Ok(vgui) = Vgui::get() else { return };
        unsafe {
            console_to_tab(&vgui);
            // After a viewdemo, open on Playback once the bar has appeared.
            if pending > 0 {
                VIEWDEMO_PENDING.store(pending - 1, Ordering::Relaxed);
                if vgui.bar().is_some_and(|bar| vgui.visible(bar)) {
                    VIEWDEMO_PENDING.store(0, Ordering::Relaxed);
                    let line = match ensure_window(&vgui, false)
                        .and_then(|(object, vp, _)| show(&vgui, object, vp, Some(PLAYBACK_PAGE)))
                    {
                        Ok(state) => format!("{NAME}: viewdemo opened the window -- {state}"),
                        Err(why) => format!("{NAME}: viewdemo could not open the window -- {why}"),
                    };
                    crate::debug::report(&format!("studio_panel: {line}"));
                }
            }
            let Some((frame, vp)) = window(&vgui) else {
                return;
            };
            fit_sheet(&vgui, frame);
            LENT.with(|cell| {
                let Ok(mut lent) = cell.try_borrow_mut() else {
                    return;
                };
                hold_minimum(&vgui, frame, vp, &mut lent);
                if vgui.visible(vp) {
                    fit_pages(&vgui, &mut lent);
                    sync_settings(&vgui, &mut lent);
                    update_help(&vgui, vp, &mut lent);
                    filter_demo_list(&vgui);
                }
                if !vgui.visible(vp) {
                    give_back(&vgui, &mut lent, None);
                    unpark(&vgui, &mut lent);
                    return;
                }
                show_enable_buttons(&vgui);
                let mut sources: Vec<&'static str> = LOANS.iter().map(|l| l.source).collect();
                sources.dedup();
                for name in sources {
                    // Only while its setting is on: otherwise the stock window
                    // is the one in use, and it keeps its own pieces.
                    if !lends(name) {
                        give_back(&vgui, &mut lent, Some(name));
                        continue;
                    }
                    match vgui.popup(name) {
                        // The console window came up (the engine shows it
                        // itself at times): its pieces are ours, so it would
                        // be blank. It goes away again.
                        Some(source) if name == DEMO_LIST && vgui.visible(source) => {
                            vgui.set_visible(source, false);
                            borrow(&vgui, name, source, &mut lent);
                        }
                        Some(source) if name == CONSOLE && vgui.visible(source) => {
                            vgui.set_visible(source, false);
                            borrow(&vgui, name, source, &mut lent);
                        }
                        Some(source) => {
                            if lent
                                .borrowed
                                .iter()
                                .any(|b| b.source_name == name && b.source != source)
                            {
                                give_back(&vgui, &mut lent, Some(name)); // a new window
                            }
                            borrow(&vgui, name, source, &mut lent);
                        }
                        None => give_back(&vgui, &mut lent, Some(name)),
                    }
                }
                match vgui.bar() {
                    Some(bar) if viewdemo_in_panel() => park(&vgui, bar, &mut lent),
                    _ => unpark(&vgui, &mut lent),
                }
                let focus_left = FOCUS_ENTRY.load(Ordering::Relaxed);
                if focus_left > 0 {
                    FOCUS_ENTRY.store(focus_left - 1, Ordering::Relaxed);
                    let entry = lent
                        .borrowed
                        .iter()
                        .find(|b| {
                            b.source_name == CONSOLE && vgui.name(b.control) == "ConsoleEntry"
                        })
                        .map(|b| b.control);
                    if let Some(entry) = entry {
                        let focus: SetParentFn = slot(vgui.panel, IPANEL_REQUEST_FOCUS);
                        focus(vgui.panel, entry, 0);
                    }
                }
            });
        }
    }

    /// Hands a VCR command to the open VCR bar.
    unsafe fn to_vcr_bar(vgui: &Vgui, command: &str) -> Result<(), String> {
        unsafe {
            let (bar, _) = vgui
                .gameui_popups()
                .into_iter()
                .find(|(_, name)| name == "DemoPlayerDialog")
                .ok_or("there's no demo in the demo player -- start one with viewdemo")?;
            let object = vgui.object(bar);
            if object.is_null() {
                return Err("the VCR bar has no object".to_string());
            }
            let on_command: OnCommandFn = slot(object, FRAME_SLOT_ON_COMMAND);
            let line = std::ffi::CString::new(command).map_err(|e| e.to_string())?;
            on_command(object, line.as_ptr());
        }
        Ok(())
    }

    unsafe fn handle(this: *mut c_void, raw: *const c_char, own: &AtomicUsize) {
        let command = text(raw);
        let result = match action(&command) {
            // Load demo... goes to our own list on the Demos tab, which works
            // with no demo playing (the VCR bar's needs a demo loaded).
            Action::Vcr("load") => {
                let sheet = SHEET.load(Ordering::Acquire) as *mut c_void;
                let demos = PAGE_OBJECTS[DEMOS_PAGE].load(Ordering::Acquire) as *mut c_void;
                if !sheet.is_null() && !demos.is_null() {
                    unsafe {
                        refill_demo_list();
                        let set_active: SetActivePageFn = slot(sheet, SHEET_SLOT_SET_ACTIVE_PAGE);
                        set_active(sheet, demos);
                    }
                }
                Ok(())
            }
            Action::Vcr(c) => Vgui::get().and_then(|vgui| unsafe { to_vcr_bar(&vgui, c) }),
            Action::Goto => unsafe { goto_typed_time() },
            Action::ResetSettings => {
                crate::commands::console_print(&format!("{NAME}: {}\n", reset_settings()));
                Ok(())
            }
            Action::Engine(line) => {
                let ran = std::ffi::CString::new(format!("{line}\n"))
                    .is_ok_and(|l| crate::engine::client_cmd(&l));
                if ran {
                    Ok(())
                } else {
                    Err(format!("could not run \"{line}\""))
                }
            }
            Action::Own => {
                let original = own.load(Ordering::Acquire);
                if original != 0 {
                    // Safety: the class's own OnCommand, from its vftable.
                    let original: OnCommandFn = unsafe { std::mem::transmute(original) };
                    unsafe { original(this, raw) };
                }
                Ok(())
            }
        };
        let outcome = match &result {
            Ok(()) => "done".to_string(),
            Err(why) => {
                crate::commands::console_print(&format!("{NAME}: {why}\n"));
                why.clone()
            }
        };
        unsafe {
            crate::debug::report(&format!(
                "studio_panel: button \"{}\" -- {outcome}",
                command.escape_debug()
            ))
        };
    }

    /// The window's `OnCommand`.
    unsafe extern "thiscall" fn frame_on_command(this: *mut c_void, raw: *const c_char) {
        unsafe { handle(this, raw, &FRAME_ON_COMMAND) }
    }

    /// Each page's `OnCommand`: the buttons on a tab send their commands here.
    unsafe extern "thiscall" fn page_on_command(this: *mut c_void, raw: *const c_char) {
        unsafe { handle(this, raw, &PAGE_ON_COMMAND) }
    }

    /// Each page's `OnKeyCodePressed`. On the Console tab, a key the borrowed
    /// input line passes up (Tab, the arrows) goes to the console dialog, its
    /// usual parent, which does the type-ahead and the command history.
    unsafe extern "thiscall" fn page_on_key_pressed(this: *mut c_void, code: i32) {
        let console_page = PAGE_OBJECTS[CONSOLE_PAGE].load(Ordering::Acquire) as *mut c_void;
        if this == console_page
            && let Ok(vgui) = Vgui::get()
        {
            unsafe {
                let object = vgui
                    .popup(CONSOLE)
                    .map(|console| vgui.object(console))
                    .filter(|o| !o.is_null());
                if let Some(object) = object {
                    let pressed: KeyFn = slot(object, PANEL_SLOT_ON_KEY_CODE_PRESSED);
                    pressed(object, code);
                    return;
                }
            }
        }
        let original = PAGE_ON_KEY_PRESSED.load(Ordering::Acquire);
        if original != 0 {
            // Safety: the class's own OnKeyCodePressed, from its vftable.
            let original: KeyFn = unsafe { std::mem::transmute(original) };
            unsafe { original(this, code) };
        }
    }

    /// Reads the Playback tab's time box and jumps there.
    unsafe fn goto_typed_time() -> Result<(), String> {
        let vgui = Vgui::get()?;
        unsafe {
            let page =
                vpanel_of(PAGE_OBJECTS[PLAYBACK_PAGE].load(Ordering::Acquire) as *mut c_void);
            let entry = vgui
                .child_named(page, GOTO_BOX)
                .map(|vp| vgui.object(vp))
                .filter(|o| !o.is_null())
                .ok_or("this Playback layout has no GotoTime box")?;
            let get_text: GetTextFn = slot(entry, TEXT_ENTRY_SLOT_GET_TEXT);
            let mut buf = [0u8; 64];
            get_text(entry, buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
            let typed = CStr::from_bytes_until_nul(&buf)
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_default();
            let seconds = parse_time(&typed)?;
            let line = std::ffi::CString::new(format!(
                "{} {seconds:.2}\n",
                crate::demo_seek::SEEK_TO_NAME
            ))
            .map_err(|e| e.to_string())?;
            if crate::engine::client_cmd(&line) {
                Ok(())
            } else {
                Err("could not run the seek".to_string())
            }
        }
    }

    /// Each page's `OnKeyCodeTyped`: a key a control on the tab didn't use
    /// comes here. On the Console tab, Enter submits the borrowed input line,
    /// as it does in the console window -- where the dialog's own Submit
    /// button is the window's default, which our window knows nothing of.
    unsafe extern "thiscall" fn page_on_key(this: *mut c_void, code: i32) {
        // Enter in the Playback tab's time box jumps, as Go does.
        let playback_page = PAGE_OBJECTS[PLAYBACK_PAGE].load(Ordering::Acquire) as *mut c_void;
        if this == playback_page && (code == KEY_ENTER || code == KEY_PAD_ENTER) {
            if let Err(why) = unsafe { goto_typed_time() } {
                crate::commands::console_print(&format!("{NAME}: {why}\n"));
            }
            return;
        }
        let console_page = PAGE_OBJECTS[CONSOLE_PAGE].load(Ordering::Acquire) as *mut c_void;
        if this == console_page
            && (code == KEY_ENTER || code == KEY_PAD_ENTER)
            && let Ok(vgui) = Vgui::get()
        {
            unsafe {
                let object = vgui
                    .popup(CONSOLE)
                    .map(|console| vgui.object(console))
                    .filter(|o| !o.is_null());
                if let Some(object) = object {
                    let on_command: OnCommandFn = slot(object, FRAME_SLOT_ON_COMMAND);
                    on_command(object, c"Submit".as_ptr());
                    return;
                }
            }
        }
        let original = PAGE_ON_KEY.load(Ordering::Acquire);
        if original != 0 {
            // Safety: the class's own OnKeyCodeTyped, from its vftable.
            let original: KeyFn = unsafe { std::mem::transmute(original) };
            unsafe { original(this, code) };
        }
    }

    /// The window, built the first time (or rebuilt on `reset`), with what
    /// was done to get it.
    unsafe fn ensure_window(
        vgui: &Vgui,
        reset: bool,
    ) -> Result<(*mut c_void, Vpanel, Vec<String>), String> {
        let mut notes = Vec::new();
        if let Some(note) = ensure_res(reset)? {
            notes.push(note);
        }
        unsafe {
            if let Some((object, vp)) = window(vgui) {
                if !reset {
                    return Ok((object, vp, notes));
                }
                // Rebuilt on reset: what it borrowed goes back first, and the
                // old window is hidden and left for GameUI to delete with its
                // parent.
                LENT.with(|cell| {
                    if let Ok(mut lent) = cell.try_borrow_mut() {
                        give_back(vgui, &mut lent, None);
                        unpark(vgui, &mut lent);
                        lent.minimum = (0, 0);
                        lent.designs.clear();
                    }
                });
                vgui.set_visible(vp, false);
            }
            let (frame, sheet) = build()?;
            OBJECT.store(frame as usize, Ordering::Release);
            SHEET.store(sheet as usize, Ordering::Release);
            let (_, b) = gameui()?;
            notes.push(format!(
                "built the window with {} tab(s) ({} GameUI)",
                PAGES.len(),
                b.name
            ));
            let (object, vp) = window(vgui).ok_or("the new window has no panel")?;
            Ok((object, vp, notes))
        }
    }

    /// Shows the window, on tab `page` (an index into [`PAGES`]) when given.
    unsafe fn show(
        vgui: &Vgui,
        object: *mut c_void,
        vp: Vpanel,
        page: Option<usize>,
    ) -> Result<String, String> {
        unsafe {
            fit_sheet(vgui, object);
            if let Some(index) = page {
                let sheet = SHEET.load(Ordering::Acquire) as *mut c_void;
                let target = PAGE_OBJECTS[index].load(Ordering::Acquire) as *mut c_void;
                if !sheet.is_null() && !target.is_null() {
                    let set_active: SetActivePageFn = slot(sheet, SHEET_SLOT_SET_ACTIVE_PAGE);
                    set_active(sheet, target);
                }
            }
            // Frame::Activate, as GameUI opens its own dialogs: shows it,
            // brings it to the front and gives it focus.
            let activate: ActivateFn = slot(object, FRAME_SLOT_ACTIVATE);
            activate(object);
            Ok(format!(
                "open (press ESC for the menu if you can't see it); {}",
                vgui.describe(vp)
            ))
        }
    }

    /// Opens the window on tab `page`, building it if needed.
    pub(super) fn open_on(page: usize) -> Result<String, String> {
        let vgui = Vgui::get()?;
        unsafe {
            let (object, vp, _) = ensure_window(&vgui, false)?;
            if page == DEMOS_PAGE {
                refill_demo_list();
            }
            show(&vgui, object, vp, Some(page))
        }
    }

    /// Opens the window (building it the first time), or closes it when open.
    pub(super) fn toggle(request: Request) -> Result<String, String> {
        let vgui = Vgui::get()?;
        unsafe {
            let reset = request == Request::Reset;
            let (object, vp, mut notes) = ensure_window(&vgui, reset)?;
            let close = match request {
                Request::Toggle => vgui.visible(vp),
                Request::Close => true,
                Request::Open | Request::Reset => false,
            };
            if close {
                vgui.set_visible(vp, false);
                notes.push("closed".to_string());
            } else {
                notes.push(show(&vgui, object, vp, None)?);
            }
            Ok(notes.join("; "))
        }
    }
}

/// Keeps the tab strip sized to the window, swaps in our window after
/// `viewdemo` or the console key, and wraps `toggleconsole` once the engine
/// has it. Called every frame from `commands::poll`; a few atomic loads until
/// the window has been opened.
pub fn poll() {
    wrap_toggleconsole();
    apply_saved_settings();
    #[cfg(target_arch = "x86")]
    hook::poll();
}

fn argument() -> Option<String> {
    let engfuncs = crate::engine::engfuncs()?;
    // Safety: the engine's own argument accessors, during a command.
    unsafe {
        if (engfuncs.cmd_argc)() < 2 {
            return None;
        }
        let raw = (engfuncs.cmd_argv)(1);
        (!raw.is_null()).then(|| CStr::from_ptr(raw).to_string_lossy().to_ascii_lowercase())
    }
}

/// What `dodstudio_panel` was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    /// Bare: open it when closed, close it when open.
    Toggle,
    Open,
    Close,
    /// Write the default layouts back and rebuild it, open.
    Reset,
}

fn request(argument: Option<&str>) -> Result<Request, String> {
    match argument {
        None => Ok(Request::Toggle),
        Some("1") | Some("open") => Ok(Request::Open),
        Some("0") | Some("close") => Ok(Request::Close),
        Some("reset") => Ok(Request::Reset),
        Some(other) => Err(format!(
            "unknown argument {other:?} -- dodstudio_panel [1|0|reset]: bare opens or closes it, 1 opens, 0 closes, reset restores the default layouts"
        )),
    }
}

/// `dodstudio_panel [1|0|reset]`: bare opens the window or closes it, `1`
/// opens it, `0` closes it, `reset` writes the default layouts back and
/// rebuilds it.
pub unsafe extern "C" fn command() {
    let result: Result<String, String> = request(argument().as_deref()).and_then(|request| {
        #[cfg(target_arch = "x86")]
        return hook::toggle(request);
        #[cfg(not(target_arch = "x86"))]
        {
            let _ = request;
            Err("only the 32-bit build has a window".to_string())
        }
    });
    let line = match result {
        Ok(what) => format!("{NAME}: {what}"),
        Err(why) => format!("{NAME}: {why}"),
    };
    crate::commands::console_print(&format!("{line}\n"));
    unsafe { crate::debug::report(&format!("studio_panel: {line}")) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_enable_button_is_in_its_tab_and_runs_its_setting() {
        for (button, page, source) in ENABLE_BUTTONS {
            let res = PAGES[*page].res.2;
            assert!(res.contains(&format!("\"{button}\"")), "{button}");
            let setting = if *source == VCR_BAR {
                VIEWDEMO_NAME
            } else {
                CONSOLE_NAME
            };
            assert!(res.contains(&format!("engine {setting} 1")), "{button}");
        }
    }

    #[test]
    fn the_main_menu_is_written_only_over_our_own() {
        assert_eq!(game_menu_action(None), Some("wrote"));
        assert_eq!(game_menu_action(Some(GAME_MENU)), None);
        assert_eq!(
            game_menu_action(Some("// DoD Studio old menu")),
            Some("updated")
        );
        assert_eq!(game_menu_action(Some("\"GameMenu\" { }")), None);
        assert!(GAME_MENU.contains(GAME_MENU_MARK));
        assert!(GAME_MENU.contains("engine dodstudio_panel 1"));
    }

    #[test]
    fn a_check_box_is_bound_by_its_name() {
        assert_eq!(bound_cvar("cvar_hud_draw"), Some("hud_draw"));
        assert_eq!(bound_cvar("cvar_"), None);
        assert_eq!(bound_cvar("cvar_x;quit"), None);
        assert_eq!(bound_cvar("Hint"), None);
    }

    #[test]
    fn a_click_sets_the_cvar_and_the_cvar_moves_the_box() {
        // First sight: the box follows the cvar.
        assert_eq!(settle(false, None, true), (None, true));
        // The user clicked it: the cvar follows the box.
        assert_eq!(settle(true, Some(false), false), (Some(true), false));
        // The cvar changed elsewhere: the box follows.
        assert_eq!(settle(false, Some(false), true), (None, true));
        // Nothing changed.
        assert_eq!(settle(true, Some(true), true), (None, false));
    }

    #[test]
    fn every_settings_box_names_a_known_cvar() {
        let res = PAGES[SETTINGS_PAGE].res.2;
        let boxes: Vec<&str> = res.split('"').filter_map(bound_cvar).collect();
        assert!(boxes.len() >= 10, "{boxes:?}");
        assert!(boxes.contains(&"dodstudio_viewdemo_in_panel"));
        assert!(boxes.contains(&"dodstudio_console_in_panel"));
    }

    #[test]
    fn a_typed_time_reads_as_the_vcr_bar_writes_it() {
        assert_eq!(parse_time("75"), Ok(75.0));
        assert_eq!(parse_time("20:33"), Ok(20.0 * 60.0 + 33.0));
        assert_eq!(parse_time(" 20:33:50 "), Ok(20.0 * 60.0 + 33.5));
        assert_eq!(parse_time("1:2.5"), Ok(62.5));
        assert!(parse_time("").is_err());
        assert!(parse_time("1:2:3:4").is_err());
        assert!(parse_time("-5").is_err());
        assert!(parse_time("abc").is_err());
    }

    #[test]
    fn the_settings_file_only_holds_cvar_number_lines() {
        assert_eq!(settings_line("hud_draw 0"), Some(("hud_draw", "0")));
        assert_eq!(settings_line("hud_draw 0;quit"), None);
        assert_eq!(settings_line("quit"), None);
        let text = settings_with("hud_draw 0\njunk\nr_drawviewmodel 1\n", "hud_draw", "1");
        assert_eq!(text, "r_drawviewmodel 1\nhud_draw 1\n");
    }

    #[test]
    fn help_text_is_read_from_a_res_file() {
        let res = "\"x.res\"
{
	\"A\"
	{
		\"labelText\"	\"{not a brace}\"
		\"tooltiptext\"	\"help A\"
	}
// \"B\" { \"tooltiptext\" \"commented\" }
	\"B\"
	{
		\"wide\"	\"5\"
	}
}
";
        assert_eq!(tooltips(res), vec![("A".to_string(), "help A".to_string())]);
    }

    #[test]
    fn every_default_control_has_help() {
        for page in &PAGES {
            let tips = tooltips(page.res.2);
            assert!(!tips.is_empty(), "{} has no help text", page.res.1);
        }
    }

    #[test]
    fn the_commands_tab_lists_every_console_name() {
        // Every console_name!("...") in the crate, but the doc example and
        // the kill feed's second name.
        let mut missing = Vec::new();
        let commands = COMMANDS_TEXT;
        assert_eq!(PAGES[COMMANDS_PAGE].res.1, "Commands.res");
        assert!(PAGES[COMMANDS_PAGE].res.2.contains(COMMAND_LIST));
        for entry in std::fs::read_dir("src")
            .unwrap()
            .chain(std::fs::read_dir("src/anim_fix").unwrap())
        {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            for part in text.split("console_name!(\"").skip(1) {
                let name = part.split('"').next().unwrap();
                if name == "status"
                    || name == "killfeed"
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
                {
                    continue;
                }
                let full = format!("dodstudio_{name}");
                if !commands.contains(&full) {
                    missing.push(full);
                }
            }
        }
        missing.sort();
        missing.dedup();
        assert!(missing.is_empty(), "missing from Commands.res: {missing:?}");
    }

    #[test]
    fn the_demo_search_matches_every_word_anywhere() {
        let row = "monday-wsod25_r07_m1_h1_hltv.dem";
        assert!(matches_filter(row, ""));
        assert!(matches_filter(row, "MONDAY"));
        assert!(matches_filter(row, "hltv monday"));
        assert!(matches_filter(row, "  r07  "));
        assert!(!matches_filter(row, "monday anzio"));
    }

    #[test]
    fn a_demo_header_gives_its_map_and_whether_hltv_recorded_it() {
        let mut header = vec![0u8; 544];
        header[..8].copy_from_slice(b"HLDEMO\0\0");
        header[16..16 + 13].copy_from_slice(b"dod_anzio\0xyz");
        let pov = demo_info(&header, 7).unwrap();
        assert_eq!(
            pov,
            DemoInfo {
                map: "dod_anzio".to_string(),
                hltv: false,
                modified: 7
            }
        );
        header.extend_from_slice(b"Spawn count 19 (HLTV)\n");
        assert!(demo_info(&header, 7).unwrap().hltv);
        assert_eq!(demo_info(b"not a demo", 0), None);
    }

    #[test]
    fn a_file_time_reads_as_a_date_that_sorts_as_text() {
        assert_eq!(date_text(civil(0)), "1970-01-01 00:00");
        // 2026-09-29 21:52:30 UTC; 2024-02-29, a leap day.
        assert_eq!(date_text(civil(1_790_718_750)), "2026-09-29 21:52");
        assert_eq!(date_text(civil(1_709_164_800)), "2024-02-29 00:00");
    }

    #[test]
    fn every_filter_has_to_pass() {
        let anzio = DemoInfo {
            map: "dod_anzio".to_string(),
            hltv: true,
            modified: 1_000_000,
        };
        let all = DemoFilters {
            search: String::new(),
            map: String::new(),
            hltv: true,
            pov: true,
            days: None,
        };
        let now = 1_000_000 + 3 * 86_400;
        assert!(passes("x.dem", Some(&anzio), &all, now));
        // The search also sees the map.
        assert!(passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                search: "anzio".into(),
                ..all.clone()
            },
            now
        ));
        assert!(!passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                map: "flash".into(),
                ..all.clone()
            },
            now
        ));
        assert!(!passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                hltv: false,
                ..all.clone()
            },
            now
        ));
        assert!(passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                pov: false,
                ..all.clone()
            },
            now
        ));
        assert!(!passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                days: Some(2),
                ..all.clone()
            },
            now
        ));
        assert!(passes(
            "x.dem",
            Some(&anzio),
            &DemoFilters {
                days: Some(3),
                ..all.clone()
            },
            now
        ));
        // A folder (no header) only answers to the search.
        assert!(passes(
            "maps/",
            None,
            &DemoFilters {
                map: "flash".into(),
                ..all.clone()
            },
            now
        ));
    }

    #[test]
    fn the_command_takes_1_0_or_reset() {
        assert_eq!(request(None), Ok(Request::Toggle));
        assert_eq!(request(Some("1")), Ok(Request::Open));
        assert_eq!(request(Some("0")), Ok(Request::Close));
        assert_eq!(request(Some("reset")), Ok(Request::Reset));
        assert!(request(Some("2")).is_err());
    }

    #[test]
    fn button_commands_are_sorted_into_what_they_do() {
        assert_eq!(action("play"), Action::Vcr("play"));
        assert_eq!(action("Faster"), Action::Vcr("faster"));
        assert_eq!(
            action("engine dodstudio_debug_status"),
            Action::Engine("dodstudio_debug_status")
        );
        assert_eq!(action("Close"), Action::Own);
        assert_eq!(action("goto"), Action::Goto);
        assert_eq!(action("reset_settings"), Action::ResetSettings);
    }

    #[test]
    fn the_tab_strip_fills_the_client_area_inside_a_margin() {
        assert_eq!(
            sheet_bounds((2, 28, 516, 210)),
            (6, 32, 508, 202 - HELP_TALL)
        );
        // Never inverted, however small the window gets.
        let tiny = sheet_bounds((0, 0, 3, 3));
        assert!(tiny.2 >= 1 && tiny.3 >= 1);
    }

    #[test]
    fn every_default_layout_is_built_in_and_names_its_own_file() {
        assert!(WINDOW_RES.2.contains("\"DodStudio\""));
        assert!(WINDOW_RES.2.contains("dodstudio_ui/DodStudio.res"));
        for page in &PAGES {
            let path = page.res.0.to_str().unwrap();
            assert_eq!(path, format!("{RES_DIR}/{}", page.res.1));
            assert!(page.res.2.starts_with(&format!("\"{path}\"")), "{path}");
        }
        // The Playback tab carries every VCR button.
        for command in [
            "start", "slower", "stepb", "pause", "play", "stepf", "faster", "end", "stop",
        ] {
            assert!(
                PAGES[0]
                    .res
                    .2
                    .contains(&format!("\"Command\"\t\t\"{command}\"")),
                "{command}"
            );
        }
    }

    #[test]
    fn the_window_is_never_narrower_than_its_tabs() {
        // Tabs ending at 200 inside a sheet, in a frame 8 px wider than its
        // client area: the tabs, both margins, the border and a little slack.
        assert_eq!(minimum_size(200, 528, 520), (200 + 8 + 8 + 8, MIN_TALL));
    }

    #[test]
    fn every_loan_has_its_slot_on_its_tab() {
        for loan in LOANS.iter().filter(|l| !l.slot.is_empty()) {
            let res = PAGES[loan.page].res.2;
            assert!(res.contains(&format!("\"{}\"", loan.slot)), "{}", loan.slot);
        }
        assert_eq!(PAGES[PLAYBACK_PAGE].name, c"Playback");
        assert_eq!(PAGES[CONSOLE_PAGE].name, c"Console");
    }

    #[test]
    fn no_name_is_the_start_of_another() {
        let names = [NAME, VIEWDEMO_NAME, CONSOLE_NAME];
        for a in names {
            for b in names {
                assert!(a == b || !a.starts_with(b), "{b} starts {a}");
            }
        }
    }

    #[test]
    fn the_builds_match_the_window_layout_table() {
        for (ours, theirs) in BUILDS
            .iter()
            .zip(crate::window_layout::GAMEUI_BUILDS.iter())
        {
            assert_eq!(ours.name, theirs.name);
            assert_eq!(ours.time_date_stamp, theirs.time_date_stamp);
            assert_eq!(ours.size_of_image, theirs.size_of_image);
        }
    }
}

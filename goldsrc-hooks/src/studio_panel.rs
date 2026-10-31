//! `ds_ui_panel`: DoD Studio's own window inside the game (issue #408,
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
//! `<game>\dod_addon\dodstudio_ui\` (or `dod\dodstudio_ui\` without
//! `-addons`, or while only that one exists: [`res_dir`]): `DodStudio.res` for the window, then one per
//! tab in [`PAGES`] (`Playback.res`, `Demos.res`, `Highlights.res`,
//! `Console.res`, `Settings.res`, `Commands.res`, `Studio.res`), and
//! `ReviewForm.res` for the review's kill range and note form (#1139, its own
//! small window over the demo, `hook::review_form`). Our own folder beside
//! `dodstudio_hd` -- never `dod\resource`, which is the user's. Each default
//! (`goldsrc-hooks/ui/`, built into the DLL) is written the first time; a
//! later DLL with a changed default replaces a file only while it is still
//! exactly what was written (`defaults.txt` keeps each one's hash), so
//! build-mode edits stay. `ds_ui_panel reset` puts every default back and
//! rebuilds the window.
//!
//! ## Per build
//!
//! Its `GameUI.dll` addresses, sizes and offsets differ between the pre-Anniversary
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

/// The Demos tab's filters, columns and rows, worked out without the game.
mod demos_tab;
/// `viewdemo`, the console key and the main menu opening the window.
mod panel_toggles;
/// The Settings tab's changes and the layout files, kept between launches.
mod saved_settings;

use demos_tab::*;
pub(crate) use panel_toggles::viewdemo_in_panel;
use panel_toggles::*;
pub use panel_toggles::{
    CONSOLE_IN_PANEL, CONSOLE_NAME, VIEWDEMO_IN_PANEL, VIEWDEMO_NAME, after_viewdemo,
    bare_viewdemo, engine_toggleconsole, write_game_menu,
};
use saved_settings::*;

pub const NAME: &str = console_name!("ui_panel");

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
/// The review's kill range and note form (#1139), a window of its own.
const FORM_RES: (&CStr, &str, &str) = (
    c"dodstudio_ui/ReviewForm.res",
    "ReviewForm.res",
    include_str!("../ui/ReviewForm.res"),
);

/// One tab: its panel name, its title on the tab strip, and its layout file
/// (path for the engine, file name, built-in default).
pub struct Page {
    pub name: &'static CStr,
    pub title: &'static CStr,
    pub res: (&'static CStr, &'static str, &'static str),
}

/// The tabs, in strip order.
pub const PAGES: [Page; 8] = [
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
        name: c"Highlights",
        title: c"Highlights",
        res: (
            c"dodstudio_ui/Highlights.res",
            "Highlights.res",
            include_str!("../ui/Highlights.res"),
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
    Page {
        name: c"Review",
        title: c"Review",
        res: (
            c"dodstudio_ui/Review.res",
            "Review.res",
            include_str!("../ui/Review.res"),
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
    /// `KeyValues::operator new(size_t)` (cdecl) and `KeyValues::KeyValues(
    /// const char *name, const char *firstKey, const char *firstValue)`: how
    /// the fill makes each row, and how the Highlights tab makes its own.
    pub keyvalues_new: usize,
    pub keyvalues_ctor: usize,
    /// `ProgressBar`'s vftable: only a control with exactly this one gets the
    /// Highlights tab's, the Playback tab's loading or the Demos tab's
    /// counting progress.
    pub progress_bar_vftable: usize,
    /// `ComboBox`'s vftable: only a control with exactly this one is filled
    /// as the Demos tab's Type or Player match dropdown, or driven as its
    /// Player box.
    pub combo_box_vftable: usize,
    /// Where a `ComboBox` keeps its drop-down `Menu *` (what its item slots
    /// hand on to, `mov ecx, [ecx + combo_menu]`).
    pub combo_menu: usize,
    /// `MenuItem`'s vftable: only a child of the Player list's menu with
    /// exactly this one has its text read, for the hovered entry's names on
    /// the help line (#781).
    pub menu_item_vftable: usize,
    /// Where a `ListPanel` keeps its sort column (`int`, -1 for none) and its
    /// ascending flag (a byte), as its `OnSetSortColumn` reads them; the
    /// secondary column and its flag follow each (#611).
    pub list_sort_column: usize,
    pub list_sort_ascending: usize,
    /// Where a `ListPanel` keeps its column records (a pointer to
    /// [`COLUMN_RECORD_SIZE`]-byte records, their allocated count after it)
    /// and the order the columns show in (a pointer to one record index per
    /// column, the count at +0xc), as `AddColumnHeader` and
    /// `OnColumnResized` read them (#801).
    pub list_columns: usize,
    pub list_column_order: usize,
    /// Where a column's drag handle (vgui2's `Dragger`) keeps the column it
    /// resizes, the position it was added at, which its "ColumnResized"
    /// message names (#801).
    pub dragger_column: usize,
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
        keyvalues_new: 0x5_3bc0,
        keyvalues_ctor: 0x5_2510,
        progress_bar_vftable: 0xa_1dcc,
        combo_box_vftable: 0x9_fbbc,
        combo_menu: 0x13c,
        menu_item_vftable: 0xa_2604,
        list_sort_column: 0xfc,
        list_sort_ascending: 0x104,
        list_columns: 0x78,
        list_column_order: 0xa4,
        dragger_column: 0x70,
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
        keyvalues_new: 0x4_4330,
        keyvalues_ctor: 0x4_4010,
        progress_bar_vftable: 0xa_bbd4,
        combo_box_vftable: 0xa_8f10,
        combo_menu: 0x140,
        menu_item_vftable: 0xa_ca20,
        list_sort_column: 0x100,
        list_sort_ascending: 0x108,
        list_columns: 0x7c,
        list_column_order: 0xa8,
        dragger_column: 0x74,
    },
];

/// Our own Load Demo window's panel name: not the stock one's, so a VCR bar's
/// own Load Demo window is never mistaken for it.
const DEMO_LIST: &str = "DodStudioDemoList";
/// The Highlights tab's own hidden Load Demo window, lending its list and
/// Load button the same way (`studio_panel/hook/streaks_tab.rs`).
const STREAK_LIST: &str = "DodStudioStreakList";
/// `ListPanel::GetSelectedItem(int)`, `IsValidItemID(int)`, `GetItem(int)`
/// (the same slots #409 reads the selected row through).
const LIST_SLOT_GET_SELECTED_ITEM: usize = 176;
const LIST_SLOT_IS_VALID_ITEM_ID: usize = 170;
const LIST_SLOT_GET_ITEM: usize = 153;
/// `KeyValues::GetString(const char *key, const char *default)`.
const KEYVALUES_SLOT_GET_STRING: usize = 12;
/// The key each row's demo name is stored under.
const ROW_KEY: &CStr = c"demoname";
/// Where the Demos tab keeps a row's real path from `dod/` once
/// [`ROW_KEY`], the list's own first column, shows just the name (#409).
const PATH_KEY: &CStr = c"dodstudio_path";

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
/// vgui2's `KEY_UP` and `KEY_DOWN`: the console's history keys.
const KEY_UP: i32 = 88;
const KEY_DOWN: i32 = 90;
/// `Frame::Activate()`: what GameUI calls on a dialog it has just built
/// (`jmp [vftable+0x280]`, both builds).
const FRAME_SLOT_ACTIVATE: usize = 160;
/// `IBaseUI::ActivateGameUI()` on the engine's `BaseUI001`: shows the menu.
/// GameUI calls slot 7 (`[vftable+0x1c]`, `HideGameUI`) after `ResumeGame` and
/// after every `engine ...` menu command, and slot 8 (`+0x20`) to bring the
/// menu up; the same in both builds' `GameUI.dll`.
const BASEUI_SLOT_ACTIVATE_GAME_UI: usize = 8;
/// `IBaseUI::HideGameUI()`: closes the menu (slot 7, above).
const BASEUI_SLOT_HIDE_GAME_UI: usize = 7;
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
    Loan {
        source: STREAK_LIST,
        control: "DemoList",
        slot: "StreakListSlot",
        page: STREAKS_PAGE,
    },
    Loan {
        source: STREAK_LIST,
        control: "LoadButton",
        slot: "StreakGoSlot",
        page: STREAKS_PAGE,
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

/// The slots whose borrowed lists take all the height their tab gains,
/// whatever their share of the tab's design height (#612).
const FILL_HEIGHT: [&str; 2] = ["DemoListSlot", "StreakListSlot"];
const STREAKS_PAGE: usize = 2;
const CONSOLE_PAGE: usize = 3;
const SETTINGS_PAGE: usize = 4;
/// The review mode's tab (#623).
const REVIEW_PAGE: usize = 7;
/// A check box named `cvar_<name>` on the Settings tab is bound to cvar
/// `<name>`: it shows the cvar's value and sets it when clicked. Any tab
/// layout can add more in build mode.
const CVAR_BOX_PREFIX: &str = "cvar_";
/// The HD styles the Settings tab's style dropdown was filled with, in its
/// row order (#899).
static SETTINGS_STYLES: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());
/// Scrolling asked for on the Settings tab since the last frame: lines (the
/// mouse wheel, 3 a notch, as a list scrolls) and pages (its buttons).
static SETTINGS_SCROLL_LINES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static SETTINGS_SCROLL_PAGES: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
/// `Panel::OnMouseWheeled(int delta)`. A control's own passes the wheel up
/// to its parent (`CallParentFunction` with "MouseWheeled", which the
/// parent's `InternalMouseWheeled`, slot 125, hands to this slot), so the
/// Settings tab hears the wheel over any of its boxes.
const PANEL_SLOT_ON_MOUSE_WHEELED: usize = 99;
/// `Button::SetSelected(bool)` (CheckButton's override posts
/// `CheckButtonChecked`) and `Button::IsSelected()`.
const BUTTON_SLOT_SET_SELECTED: usize = 173;
/// `TextEntry::GetText(char *buf, int bufLen)`, as the console reads its own
/// input line (`call [vftable+0x224]`).
const TEXT_ENTRY_SLOT_GET_TEXT: usize = 137;
/// `Label::SetText(const char *)`, as GameUI calls it with its `#GameUI_`
/// strings (`call [vftable+0x21c]`).
const LABEL_SLOT_SET_TEXT: usize = 135;
/// `Label::GetText(char *buf, int bufLen)`: hands on to its `TextImage`,
/// which converts the text through the localizer. `MenuItem` inherits it;
/// the Player list reads its hovered row through it (#781).
const LABEL_SLOT_GET_TEXT: usize = 137;
const BUTTON_SLOT_IS_SELECTED: usize = 174;

/// The Commands tab's text: every console setting and command, grouped.
const COMMANDS_TEXT: &str = include_str!("../ui/Commands.txt");
/// The Commands tab's text box, in `Commands.res`.
const COMMAND_LIST: &str = "CommandList";
const COMMANDS_PAGE: usize = 5;

/// `ListPanel::FirstItem()`, `NextItem(int)`, `SetItemVisible(int, bool)`
/// (the Source `ListPanel` order, which GoldSrc's matches from `GetItem(int)`
/// at 153 to `GetSelectedItem` at 176).
const LIST_SLOT_FIRST_ITEM: usize = 167;
const LIST_SLOT_NEXT_ITEM: usize = 168;
const LIST_SLOT_SET_ITEM_VISIBLE: usize = 171;
/// `AddColumnHeader(int index, const char *name, const char *text, int width,
/// int minWidth, int maxWidth, int flags)`: what our columns are built with
/// ([`column_width_range`]), and what the 5-argument form in the slot before
/// it forwards to on both builds. `SetColumnSortable(int, bool)` follows.
const LIST_SLOT_ADD_COLUMN_HEADER_RANGE: usize = 135;
const LIST_SLOT_SET_COLUMN_SORTABLE: usize = 148;
/// `SetColumnVisible(int, bool)`: the Highlights tab hides its Player
/// column for a POV demo (one player).
const LIST_SLOT_SET_COLUMN_VISIBLE: usize = 149;
/// `ApplyItemChanges(int itemID)`: re-indexes a row after its values
/// changed. The rows shown keep their order until the list sorts again.
const LIST_SLOT_APPLY_ITEM_CHANGES: usize = 161;
/// `DeleteAllItems()`: what the Load Demo window's own fill starts with.
const LIST_SLOT_DELETE_ALL_ITEMS: usize = 165;
/// `AddItem(const KeyValues *data, int userData, bool scrollToItem, bool
/// sortOnAdd)`, which keeps a copy of `data` (`MakeCopy`, KeyValues slot
/// 21), never `data` itself.
const LIST_SLOT_ADD_ITEM: usize = 151;
/// `sizeof(KeyValues)`, what the fill allocates for each row.
const KEYVALUES_SIZE: usize = 0x18;
/// `ProgressBar::SetProgress(float)`, 0 to 1.
const PROGRESS_BAR_SLOT_SET_PROGRESS: usize = 134;
/// `KeyValues::SetString(const char *key, const char *value)`: the 3-argument
/// `KeyValues` constructor the demo list's rows are made with calls it.
const KEYVALUES_SLOT_SET_STRING: usize = 17;
/// `KeyValues::deleteThis()`: destroys the row and hands its memory back to
/// the `KeyValuesSystem` that `Build::keyvalues_new` took it from. A vftable
/// slot, the same on both builds; `ListPanel::DeleteAllItems` frees its own
/// rows through it (#812).
const KEYVALUES_SLOT_DELETE_THIS: usize = 24;
/// `sizeof` a `ListPanel` column record (`column_t` plus its linked-list
/// links) and where in it the column's `Dragger *` sits, on both builds
/// (#801, `verify_studio_panel.py`).
const COLUMN_RECORD_SIZE: usize = 0x44;
const COLUMN_DRAGGER: usize = 0x14;

/// Each column's drag handle and the position it must name: (record index,
/// position) for every column in `order` whose record is among the
/// `allocated` ones. A `Dragger` keeps the position its column was added at,
/// and `AddColumnHeader` never renumbers them, so a column inserted before
/// others (the Demos tab's icons, #779) left Demo File's handle naming
/// position 0, the fixed icon column, and dragging it did nothing (#801).
/// The narrowest a resizable column may be dragged, in the pixels our widths
/// are given in: the minimum the 5-argument `AddColumnHeader` gives on the
/// pre-Anniversary build. And the widest any column may be.
const COLUMN_MIN_WIDE: i32 = 20;
const COLUMN_MAX_WIDE: i32 = 10_000;
/// `ListPanel::AddColumnHeader` flags: fixed width is `COLUMN_FIXEDSIZE`
/// without `COLUMN_RESIZEWITHWINDOW` (2).
const COLUMN_FIXED_MASK: i32 = 3;
const COLUMN_FIXED: i32 = 1;

/// The minimum and maximum width a column of `width` and `flags` is added
/// with: what the 5-argument `AddColumnHeader` gives, except that the
/// minimum is never above the width. The 25th Anniversary's 5-argument form
/// scales its minimum of 20 to the screen but leaves the width as given, so
/// on a screen 2160 lines tall its minimum passed our narrowest columns (40)
/// and `ListPanel.cpp`'s `minWidth <= width` assert stopped the game the
/// moment our window was built (#1092).
fn column_width_range(width: i32, flags: i32) -> (i32, i32) {
    if flags & COLUMN_FIXED_MASK == COLUMN_FIXED {
        (width, width)
    } else {
        (COLUMN_MIN_WIDE.min(width), COLUMN_MAX_WIDE.max(width))
    }
}

fn dragger_positions(order: &[u8], allocated: usize) -> impl Iterator<Item = (usize, i32)> + '_ {
    order
        .iter()
        .enumerate()
        .filter(move |&(_, &record)| usize::from(record) < allocated)
        .map(|(position, &record)| (usize::from(record), position as i32))
}

/// `TextEntry::SetText(const char *)`: the Player box is emptied after a
/// name is picked from its list.
const TEXT_ENTRY_SLOT_SET_TEXT: usize = 134;
/// `ComboBox::AddItem(const char *text, const KeyValues *userData)` and
/// `ActivateItemByRow(int row)`.
const COMBO_SLOT_ADD_ITEM: usize = 196;
const COMBO_SLOT_ACTIVATE_ITEM_BY_ROW: usize = 208;
/// `ComboBox::DeleteAllItems()`, handing on to its drop-down menu's (which
/// marks every item for deletion): the Player dropdown's list is rebuilt
/// for what is typed.
const COMBO_SLOT_DELETE_ALL_ITEMS: usize = 203;
/// `Menu::SetFixedWidth(int)`, on a `ComboBox`'s drop-down menu: the box sets
/// it to its own width each time the list opens, which cuts the Go to
/// list's folder names short, so the tab widens it again while it is open.
const MENU_SLOT_SET_FIXED_WIDTH: usize = 166;
/// `Panel::SetEnabled(bool)`, what a `.res` file's `enabled` key calls: the
/// Demos tab's Back, Forward and Up are greyed out where they have nowhere
/// to go.
const PANEL_SLOT_SET_ENABLED: usize = 47;
/// `Panel::IsEnabled()`, the slot after: what `Button` and `Slider` check
/// before taking a click, and what the review's lock (#826) reads to put
/// each control back as it was.
const PANEL_SLOT_IS_ENABLED: usize = 48;
/// What a `ComboBox`'s own arrow button sends it: `OnCommand` opens the list,
/// or closes it if open.
const BUTTON_CLICKED: &CStr = c"ButtonClicked";
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

/// How far down the Settings tab is scrolled after `lines` (the wheel) and
/// `pages` (its buttons): a page is the view less one line, so a line stays
/// in sight across it, and the scroll stays between the top and where the
/// last row is just in view.
fn settings_scroll(scroll: i32, lines: i32, pages: i32, line: i32, view: i32, content: i32) -> i32 {
    let page = (view - line).max(line);
    let most = (content - view).max(0);
    scroll
        .saturating_add(lines.saturating_mul(line))
        .saturating_add(pages.saturating_mul(page))
        .clamp(0, most)
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
/// Where the VCR bar waits, off screen, while `ds_ui_viewdemo_in_panel` has
/// our window stand in for it. Off screen rather than hidden: a hidden panel
/// stops thinking, and the bar's think is what updates the time and slider.
const PARKED_AT: i32 = -20_000;
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
    /// The Settings tab's scroll buttons: a page up (-1) or down (1).
    ScrollSettings(i32),
    /// Forget the players picked on the Demos tab.
    ClearPlayers,
    /// The Demos tab's Back, Forward and Up buttons (#779).
    DemoBack,
    DemoForward,
    DemoUp,
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
    if command.eq_ignore_ascii_case("demo_back") {
        return Action::DemoBack;
    }
    if command.eq_ignore_ascii_case("demo_forward") {
        return Action::DemoForward;
    }
    if command.eq_ignore_ascii_case("demo_up") {
        return Action::DemoUp;
    }
    if command.eq_ignore_ascii_case("reset_settings") {
        return Action::ResetSettings;
    }
    if command.eq_ignore_ascii_case(crate::settings_tab::SCROLL_UP_COMMAND) {
        return Action::ScrollSettings(-1);
    }
    if command.eq_ignore_ascii_case(crate::settings_tab::SCROLL_DOWN_COMMAND) {
        return Action::ScrollSettings(1);
    }
    if command.eq_ignore_ascii_case("clear_players") {
        return Action::ClearPlayers;
    }
    match VCR_COMMANDS
        .iter()
        .find(|c| c.eq_ignore_ascii_case(command))
    {
        Some(c) => Action::Vcr(c),
        None => Action::Own,
    }
}

/// Whether a Playback tab button's action moves the demo, so a review's
/// lock refuses it (#826): the VCR bar's commands, Go, and console lines that
/// seek, change speed or load.
fn action_moves_the_demo(action: &Action) -> bool {
    match action {
        Action::Vcr(_) | Action::Goto => true,
        Action::Engine(line) => crate::review_lock::moves_the_demo(line),
        _ => false,
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

/// The loading strip's panel name, in `DodStudio.res` (#797).
const LOAD_STRIP: &str = "LoadStrip";
/// How far the pages' controls move down while the strip shows.
const LOAD_STRIP_TALL: i32 = 20;

/// Where the loading strip goes on a page `page_wide` across: along its top,
/// above the controls, which [`LOAD_STRIP_TALL`] pushes down.
fn load_strip_bounds(page_wide: i32) -> (i32, i32, i32, i32) {
    (8, 2, (page_wide - 16).max(1), LOAD_STRIP_TALL - 2)
}

/// What the loading strip says: how far the demo player has read the demo
/// and how far the highlights analysis has got, each while it runs; `None`
/// once both are done, and the strip goes.
fn load_strip_text(demo: Option<u32>, highlights: Option<u32>) -> Option<String> {
    let parts: Vec<String> = [("Demo", demo), ("Highlights", highlights)]
        .into_iter()
        .filter_map(|(what, percent)| Some(format!("{what} {}%", percent?)))
        .collect();
    (!parts.is_empty()).then(|| format!("Loading: {}", parts.join("   ")))
}

/// Every control's `"helptext"` in a `.res` file, by its name (an older layout's
/// `"tooltiptext"` too; vgui2 shows that one as a tooltip on a check box):
/// what the help line shows for it. (GameUI reads the key but shows no tooltip, so
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
                if (key.eq_ignore_ascii_case("helptext") || key.eq_ignore_ascii_case("tooltiptext"))
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

/// The layouts' folder: `dod_addon\dodstudio_ui` when the game has
/// `-addons` (every DoD Studio launch, #412) and `dod_addon` has the folder
/// or `dod` doesn't, else `dod\dodstudio_ui`, the place from before #415.
/// The engine loads the layouts through its own search paths, which only
/// include `dod_addon` with `-addons`, ahead of `dod`.
fn res_dir() -> std::path::PathBuf {
    let game = crate::game_folder::game_root();
    let addon = game.join(crate::game_folder::addon_name());
    let dod = game.join(crate::game_folder::name());
    let base = if ui_in_addon(
        crate::texture_hires::addons_on(),
        addon.join(RES_DIR).is_dir(),
        dod.join(RES_DIR).is_dir(),
    ) {
        addon
    } else {
        dod
    };
    base.join(RES_DIR)
}

/// `<game>\dod`, where demos are named from: the layouts' folder can be
/// in `dod_addon`, so it isn't this one's child.
fn dod_dir() -> std::path::PathBuf {
    crate::texture_hires::game_dir()
}

/// Whether the layouts live in `dod_addon`: only with `-addons`, and then as
/// the HD files pick (`texture_hires::hd_home`).
fn ui_in_addon(addons: bool, in_addon: bool, in_dod: bool) -> bool {
    addons && crate::texture_hires::hd_home(in_addon, in_dod) == crate::texture_hires::HdHome::Addon
}

#[cfg(target_arch = "x86")]
mod hook {
    use std::ffi::{c_char, c_void};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};

    use super::*;

    /// The stock VCR bar's loading progress (#465).
    mod demo_bar_progress;
    /// The Demos tab (#409, #779).
    mod demos_tab;
    /// The Demos tab's counting bar (#409).
    mod folder_progress;
    /// The icons in the Demos tab's list (#779).
    mod list_icons;
    /// The Playback tab's loading progress (#465).
    mod load_progress;
    /// The loading strip along the top of every tab (#797).
    mod load_strip;
    /// The demo controls greyed out while a review runs (#826).
    pub(super) mod lock_controls;
    /// The Demos tab's Player box, a dropdown narrowed as you type (#565).
    mod player_picker;
    /// The review's kill range and note form over the demo (#1139).
    mod review_form;
    /// The Review tab (#623).
    pub(super) mod review_tab;
    /// The sort arrow in the lists' headings (#611).
    mod sort_arrows;
    /// The Highlights tab (#565).
    mod streaks_tab;
    /// The hidden VCR bar's slider kept in step with playback (#663).
    mod vcr_sync;

    use demos_tab::*;

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
    type ComboAddItemFn =
        unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_void) -> i32;
    type ListItemIdFn = unsafe extern "thiscall" fn(*mut c_void, i32) -> i32;
    type ListSetVisibleFn = unsafe extern "thiscall" fn(*mut c_void, i32, u32);
    type AddColumnRangeFn = unsafe extern "thiscall" fn(
        *mut c_void,
        i32,
        *const c_char,
        *const c_char,
        i32,
        i32,
        i32,
        i32,
    );

    /// Adds a column to `list` at `index`, with the width range
    /// [`column_width_range`] gives it (#1092).
    unsafe fn add_column(
        list: *mut c_void,
        index: i32,
        key: &CStr,
        heading: &CStr,
        width: i32,
        flags: i32,
    ) {
        let (min, max) = column_width_range(width, flags);
        unsafe {
            let add: AddColumnRangeFn = slot(list, LIST_SLOT_ADD_COLUMN_HEADER_RANGE);
            add(
                list,
                index,
                key.as_ptr(),
                heading.as_ptr(),
                width,
                min,
                max,
                flags,
            );
        }
    }
    type ListIntVoidFn = unsafe extern "thiscall" fn(*mut c_void, i32);
    type ListIntBoolFn = unsafe extern "thiscall" fn(*mut c_void, i32, u32);
    type SetStringFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char, *const c_char);
    type SetIntFn = unsafe extern "thiscall" fn(*mut c_void, *const c_char, i32);
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
    /// The console key opened the console window and our tab took its place:
    /// the game thinks its console is open until the key closes it again.
    static CONSOLE_OPENED_BY_KEY: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);

    const FOCUS_ENTRY_FRAMES: u32 = 5;

    /// Each class's own `OnCommand`, for whatever our handler passes on.
    static FRAME_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    static PAGE_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    static PAGE_ON_KEY: AtomicUsize = AtomicUsize::new(0);
    static PAGE_ON_KEY_PRESSED: AtomicUsize = AtomicUsize::new(0);
    static PAGE_ON_WHEEL: AtomicUsize = AtomicUsize::new(0);
    /// Frames left to put the focus back on the Console tab's input line
    /// after Up or Down (#786).
    static CONSOLE_REFOCUS: AtomicUsize = AtomicUsize::new(0);

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

    /// Whether a check box on `page` is ticked, or `missing` when the layout
    /// has none.
    unsafe fn box_ticked_or(vgui: &Vgui, page: Vpanel, name: &str, missing: bool) -> bool {
        unsafe {
            let Ok((base, build)) = gameui() else {
                return missing;
            };
            match vgui.child_named(page, name).map(|vp| vgui.object(vp)) {
                Some(o)
                    if !o.is_null()
                        && *(o as *const usize) == base + build.check_button_vftable =>
                {
                    let is_selected: IsSelectedFn = slot(o, BUTTON_SLOT_IS_SELECTED);
                    is_selected(o) & 0xff != 0
                }
                _ => missing,
            }
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

        /// Whether `vp` and every panel above it are visible. ESC hides the
        /// menu around our window and leaves the window's own flag set, so
        /// [`Self::visible`] alone still says it is open.
        unsafe fn shown(&self, vp: Vpanel) -> bool {
            let mut panel = vp;
            for _ in 0..32 {
                if panel == 0 {
                    return true;
                }
                if !unsafe { self.visible(panel) } {
                    return false;
                }
                panel = unsafe { self.parent_of(panel) };
            }
            true
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

        /// Whether `vp`, kept from an earlier frame, is still GameUI's popup
        /// `name`. A kept handle can outlive its panel: on the 25th Anniversary
        /// build the VCR bar went away after ESC closed it, and asking vgui2
        /// about the old one, `object` included, crashed the game.
        unsafe fn still(&self, name: &str, vp: Vpanel) -> bool {
            unsafe { self.popup(name) == Some(vp) }
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
        /// The bar, when it had been closed (its X) and we showed it again
        /// off screen: a hidden bar doesn't update the slider and time it
        /// lends us. Hidden again when it comes back on screen.
        revived: Option<Vpanel>,
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
        /// Each bound dropdown's text as last set or seen, the same way.
        /// The frames left to wait for one to show what it was given.
        choices: Vec<(Vpanel, String, u8)>,
        /// How far the Settings tab is scrolled down, in pixels.
        settings_scroll: i32,
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
        /// The tab size last logged, so the log gets one line per size.
        logged: Option<(i32, i32)>,
    }

    thread_local! {
        // Only the engine's main thread polls, so no lock is needed.
        static LENT: std::cell::RefCell<Lent> = const {
            std::cell::RefCell::new(Lent {
                borrowed: Vec::new(),
                parked_from: None,
                revived: None,
                minimum: (0, 0),
                reported: Vec::new(),
                console_home: None,
                list_offset: None,
                list_set: None,
                boxes: Vec::new(),
                choices: Vec::new(),
                settings_scroll: 0,
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
                        (
                            PANEL_SLOT_ON_MOUSE_WHEELED,
                            page_on_wheel as *const () as usize,
                        ),
                    ],
                );
                PAGE_ON_COMMAND.store(own[0], Ordering::Release);
                PAGE_ON_KEY.store(own[1], Ordering::Release);
                PAGE_ON_KEY_PRESSED.store(own[2], Ordering::Release);
                PAGE_ON_WHEEL.store(own[3], Ordering::Release);
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
                    logged: None,
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

            // The Demos tab's Type dropdown: All, POV, HLTV, starting on All.
            let demos_page =
                vpanel_of(PAGE_OBJECTS[DEMOS_PAGE].load(Ordering::Acquire) as *mut c_void);
            if let Some(o) = vgui
                .child_named(demos_page, DEMO_TYPE)
                .map(|vp| vgui.object(vp))
                .filter(|o| !o.is_null() && *(*o as *const usize) == base + build.combo_box_vftable)
            {
                let add_item: ComboAddItemFn = slot(o, COMBO_SLOT_ADD_ITEM);
                for item in DEMO_TYPES {
                    add_item(o, item.as_ptr(), std::ptr::null());
                }
                let activate_row: ListIntVoidFn = slot(o, COMBO_SLOT_ACTIVATE_ITEM_BY_ROW);
                activate_row(o, 0);
            }
            // The Player match dropdown: Any, All, starting on Any.
            if let Some(o) = vgui
                .child_named(demos_page, PLAYER_MATCH)
                .map(|vp| vgui.object(vp))
                .filter(|o| !o.is_null() && *(*o as *const usize) == base + build.combo_box_vftable)
            {
                let add_item: ComboAddItemFn = slot(o, COMBO_SLOT_ADD_ITEM);
                for item in PLAYER_MATCHES {
                    add_item(o, item.as_ptr(), std::ptr::null());
                }
                let activate_row: ListIntVoidFn = slot(o, COMBO_SLOT_ACTIVATE_ITEM_BY_ROW);
                activate_row(o, 0);
            }
            // The Go to dropdown: the heading, then the places (#779).
            fill_place_box(&vgui, base, build);
            // The Settings tab's dropdowns: each row's choices (#899).
            fill_settings_choices(&vgui, base, build);

            // Our own Load Demo window, never shown: the Demos tab borrows its
            // list and Load button. It fills its list as it is built.
            let dialog = allocate(base, build, build.frame_size + 4)?;
            let dialog_ctor: SheetCtor = std::mem::transmute(base + build.file_dialog_ctor);
            // Its list has Back, Forward and Up, so no up row (#779).
            crate::demo_list_folders::set_tab_dialog(dialog as usize);
            crate::demo_list_folders::tab_fill(|| {
                dialog_ctor(dialog, frame, c"DodStudioDemoList".as_ptr())
            });
            vgui.set_visible(vpanel_of(dialog), false);
            DEMO_LIST_ON_COMMAND.store(
                own_on_command(dialog, demo_list_on_command as *const () as usize),
                Ordering::Release,
            );
            DEMO_DIALOG.store(dialog as usize, Ordering::Release);
            add_demo_columns(&vgui, dialog);
            refill_demo_list();
            streaks_tab::build(&vgui, base, build, frame)?;
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
                        // The window's place is its rect, not GetAbsPos: for
                        // the hidden console that answers (0, 0), which put
                        // the window off-screen on every other key and made
                        // the list flicker between there and the input line.
                        let (sx, sy, _, _) = vgui.rect(source);
                        // A hidden list sits wherever it was left (0, 0 before
                        // the first key); only a showing one was placed by
                        // the console.
                        if vgui.visible(vp) && lent.list_set != Some((px, py)) && (px, py) != want {
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
                let settings_page =
                    vpanel_of(PAGE_OBJECTS[SETTINGS_PAGE].load(Ordering::Acquire) as *mut c_void);
                if page == settings_page {
                    let scroll = lent.settings_scroll;
                    let Some(design) = lent.designs.iter_mut().find(|d| d.page == page) else {
                        continue;
                    };
                    let (scroll, content) = place_settings(vgui, &design.controls, scroll, w, h);
                    if design.logged != Some((w, h)) {
                        design.logged = Some((w, h));
                        crate::debug::report(&format!(
                            "studio_panel: tab Settings is {w}x{h}, its rows {content} tall ({})",
                            if content > h - load_strip::inset() {
                                "it scrolls"
                            } else {
                                "all in view"
                            }
                        ));
                    }
                    lent.settings_scroll = scroll;
                    continue;
                }
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
                // Measured from where the controls end, so a tab that came out
                // bigger than drawn still keeps its bottom row at the bottom
                // (#609).
                let rects: Vec<_> = design.controls.iter().map(|&(_, at)| at).collect();
                // The loading strip takes the top of the tab (#797): fit the
                // controls to the rest and move them down under it.
                let inset = load_strip::inset();
                let (fit_design, fit_now) =
                    crate::window_layout::content_frame(&rects, size, (w, (h - inset).max(1)));
                if design.logged != Some((w, h)) {
                    design.logged = Some((w, h));
                    crate::debug::report(&format!(
                        "studio_panel: tab {} is {w}x{h}, laid out for {}x{}, fitted as {}x{} of {}x{}",
                        vgui.name(page),
                        size.0,
                        size.1,
                        fit_now.0,
                        fit_now.1,
                        fit_design.0,
                        fit_design.1
                    ));
                }
                for &(control, at) in &design.controls {
                    // A tab's list fills whatever height the tab gains (#612).
                    let want = if FILL_HEIGHT.contains(&vgui.name(control).as_str()) {
                        crate::window_layout::fit_rect_tall(at, fit_design, fit_now)
                    } else {
                        crate::window_layout::fit_rect(at, fit_design, fit_now)
                    };
                    vgui.place(control, (want.0, want.1 + inset, want.2, want.3));
                }
            }
        }
    }

    /// A control's place: x, y, width, height.
    type Rect = (i32, i32, i32, i32);

    /// Lays the Settings tab out scrolled (#899): its rows are taller than the
    /// tab, so each control keeps its `.res` place less `scroll`, and only
    /// those wholly in view show. The scroll buttons stay at the tab's right
    /// edge, top and bottom, greyed out at either end and hidden while every
    /// row fits. The folder boxes that need another one on also stay hidden
    /// (#787). Returns the scroll, after this frame's wheel and buttons, and
    /// how tall the rows are.
    unsafe fn place_settings(
        vgui: &Vgui,
        controls: &[(Vpanel, Rect)],
        scroll: i32,
        w: i32,
        h: i32,
    ) -> (i32, i32) {
        use crate::settings_tab::{SCROLL_DOWN, SCROLL_UP};
        let inset = load_strip::inset();
        let named: Vec<(String, Vpanel, Rect)> = controls
            .iter()
            .map(|&(c, at)| (unsafe { vgui.name(c) }, c, at))
            .collect();
        let button = |name: &str| {
            named
                .iter()
                .find(|(n, _, _)| n == name)
                .map(|&(_, c, at)| (c, at))
        };
        let rows = || {
            named
                .iter()
                .filter(|(n, _, _)| n != SCROLL_UP && n != SCROLL_DOWN)
        };
        let line = button(SCROLL_UP).map_or(22, |(_, at)| at.3.max(1));
        let top = rows().map(|(_, _, at)| at.1).min().unwrap_or(0);
        let content = rows().map(|(_, _, at)| at.1 + at.3).max().unwrap_or(0) + top;
        let view = (h - inset).max(1);
        let scroll = settings_scroll(
            scroll,
            SETTINGS_SCROLL_LINES.swap(0, Ordering::AcqRel),
            SETTINGS_SCROLL_PAGES.swap(0, Ordering::AcqRel),
            line,
            view,
            content,
        );
        let most = (content - view).max(0);
        let folders = crate::demo_list_folders::enabled();
        let counts = crate::demo_list_folders::SHOW_COUNTS.on();
        unsafe {
            for (name, control, at) in rows() {
                let y = at.1 - scroll + inset;
                let in_view = y >= inset && y + at.3 <= h;
                let shown = bound_cvar(name)
                    .and_then(|cvar| {
                        folder_box_shown(&crate::names::current_name(cvar), folders, counts)
                    })
                    .unwrap_or(true);
                vgui.place(*control, (at.0, y, at.2, at.3));
                if vgui.visible(*control) != (in_view && shown) {
                    vgui.set_visible(*control, in_view && shown);
                }
            }
            for (name, at_top, enabled) in [
                (SCROLL_UP, true, scroll > 0),
                (SCROLL_DOWN, false, scroll < most),
            ] {
                let Some((control, at)) = button(name) else {
                    continue;
                };
                let y = if at_top { inset + 4 } else { h - at.3 - 4 };
                vgui.place(control, (w - at.2 - 8, y, at.2, at.3));
                if vgui.visible(control) != (most > 0) {
                    vgui.set_visible(control, most > 0);
                }
                let object = vgui.object(control);
                if !object.is_null() {
                    let is_enabled: IsSelectedFn = slot(object, PANEL_SLOT_IS_ENABLED);
                    if (is_enabled(object) & 0xff != 0) != enabled {
                        let set_enabled: ListIntVoidFn = slot(object, PANEL_SLOT_SET_ENABLED);
                        set_enabled(object, enabled as i32);
                    }
                }
            }
        }
        (scroll, content)
    }

    /// Each page's `OnMouseWheeled`: on the Settings tab the wheel scrolls
    /// its rows, 3 lines a notch as a list scrolls; any other tab passes it on.
    unsafe extern "thiscall" fn page_on_wheel(this: *mut c_void, delta: i32) {
        let settings = PAGE_OBJECTS[SETTINGS_PAGE].load(Ordering::Acquire) as *mut c_void;
        if this == settings {
            SETTINGS_SCROLL_LINES.fetch_add(delta.saturating_mul(-3), Ordering::AcqRel);
            return;
        }
        let original = PAGE_ON_WHEEL.load(Ordering::Acquire);
        if original != 0 {
            // Safety: the class's own OnMouseWheeled, from its vftable.
            let original: KeyFn = unsafe { std::mem::transmute(original) };
            unsafe { original(this, delta) };
        }
    }

    /// Fills the Settings tab's dropdowns: each row's choices, or the HD
    /// styles built on disk (#899). `sync_settings` then shows each cvar's
    /// value.
    unsafe fn fill_settings_choices(vgui: &Vgui, base: usize, build: &Build) {
        use crate::settings_tab::Kind;
        unsafe {
            let page =
                vpanel_of(PAGE_OBJECTS[SETTINGS_PAGE].load(Ordering::Acquire) as *mut c_void);
            for control in vgui.child_list(page) {
                let name = vgui.name(control);
                let Some(row) = bound_cvar(&name)
                    .and_then(|cvar| crate::settings_tab::row(&crate::names::current_name(cvar)))
                else {
                    continue;
                };
                let object = vgui.object(control);
                if object.is_null() || *(object as *const usize) != base + build.combo_box_vftable {
                    continue;
                }
                let items: Vec<String> = match row.kind {
                    Kind::Choice(choices) => choices.iter().map(|(_, t)| t.to_string()).collect(),
                    Kind::Styles => {
                        let styles = crate::settings_tab::style_choices(
                            &crate::texture_hires::built_styles(),
                            &crate::texture_hires::current_style(),
                            crate::texture_hires::DEFAULT_STYLE,
                        );
                        if let Ok(mut kept) = SETTINGS_STYLES.write() {
                            kept.clone_from(&styles);
                        }
                        styles
                    }
                    Kind::Check => continue,
                };
                let add_item: ComboAddItemFn = slot(object, COMBO_SLOT_ADD_ITEM);
                for item in items {
                    if let Ok(item) = std::ffi::CString::new(item) {
                        add_item(object, item.as_ptr(), std::ptr::null());
                    }
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
            // Over an entry in the open Player list: all of that player's
            // names, which the entry itself cuts short (#781).
            if let Some(names) = player_picker::hovered_help(vgui.rect(line).2) {
                text = names;
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

    /// Keeps every `cvar_<name>` control on the Settings tab and its cvar in
    /// step: a click or a pick sets the cvar and saves it, and a cvar set
    /// elsewhere (the console, a config) moves the control. A check box is
    /// on/off; a dropdown shows its row's choices (#899).
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
                // A Settings.res edited under an older DLL still names the
                // pre-#555 cvars; the layout is ours, so the box still works.
                let current = crate::names::current_name(cvar);
                let cvar: &str = &current;
                let object = vgui.object(control);
                let Ok(c_name) = std::ffi::CString::new(cvar) else {
                    continue;
                };
                if object.is_null() {
                    continue;
                }
                if *(object as *const usize) == base + build.combo_box_vftable {
                    sync_choice(lent, control, object, cvar, &c_name);
                    continue;
                }
                if *(object as *const usize) != base + build.check_button_vftable {
                    continue;
                }
                let is_selected: IsSelectedFn = slot(object, BUTTON_SLOT_IS_SELECTED);
                let set_selected: SetSelectedFn = slot(object, BUTTON_SLOT_SET_SELECTED);
                let checked = is_selected(object) & 0xff != 0;
                let cvar_on = (engfuncs.pfn_get_cvar_float)(c_name.as_ptr()) != 0.0;
                let last = lent
                    .boxes
                    .iter()
                    .find(|(c, _)| *c == control)
                    .map(|(_, v)| *v);
                let (set, redraw) = settle(checked, last, cvar_on);
                let now = match set {
                    Some(value) => {
                        let before = cvar_text(&c_name);
                        if let Ok(line) =
                            std::ffi::CString::new(format!("{cvar} {}\n", value as u8))
                        {
                            crate::engine::client_cmd(&line);
                        }
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

    /// One bound dropdown: its choices are its row's in
    /// [`crate::settings_tab::AREAS`], or the HD styles it was filled with.
    unsafe fn sync_choice(
        lent: &mut Lent,
        control: Vpanel,
        object: *mut c_void,
        cvar: &str,
        c_name: &CStr,
    ) {
        use crate::settings_tab::{ChoiceStep, Kind};
        let Some(row) = crate::settings_tab::row(cvar) else {
            return;
        };
        let styles = SETTINGS_STYLES
            .read()
            .map(|s| s.clone())
            .unwrap_or_default();
        let owned: Vec<(&str, &str)>;
        let choices: &[(&str, &str)] = match row.kind {
            Kind::Choice(choices) => choices,
            Kind::Styles => {
                owned = styles.iter().map(|s| (s.as_str(), s.as_str())).collect();
                &owned
            }
            Kind::Check => return,
        };
        let Some(value) = cvar_text(c_name) else {
            return;
        };
        let want = crate::settings_tab::choice_text(choices, &value);
        let shown = unsafe {
            let get_text: GetTextFn = slot(object, TEXT_ENTRY_SLOT_GET_TEXT);
            let mut buf = [0u8; 128];
            get_text(object, buf.as_mut_ptr() as *mut c_char, buf.len() as i32);
            CStr::from_bytes_until_nul(&buf)
                .map(|c| c.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let (last, wait) = lent
            .choices
            .iter()
            .find(|(c, _, _)| *c == control)
            .map_or((None, 0), |(_, t, w)| (Some(t.clone()), *w));
        let (step, wait) = crate::settings_tab::settle_choice(&shown, last.as_deref(), &want, wait);
        let now = match step {
            ChoiceStep::Wait => last.unwrap_or_default(),
            ChoiceStep::Picked => {
                // Text that is none of the choices is a value set elsewhere,
                // shown as itself: nothing to set.
                if let Some(picked) = crate::settings_tab::choice_value(choices, &shown) {
                    let before = cvar_text(c_name);
                    if let Ok(line) = std::ffi::CString::new(format!("{cvar} {picked}\n")) {
                        crate::engine::client_cmd(&line);
                    }
                    save_setting(cvar, picked, before);
                }
                shown
            }
            ChoiceStep::Show => {
                unsafe {
                    match choices.iter().position(|(_, text)| *text == want) {
                        Some(row) => {
                            let activate_row: ListIntVoidFn =
                                slot(object, COMBO_SLOT_ACTIVATE_ITEM_BY_ROW);
                            activate_row(object, row as i32);
                        }
                        None => {
                            if let Ok(text) = std::ffi::CString::new(want.clone()) {
                                let set_text: SetTextFn = slot(object, TEXT_ENTRY_SLOT_SET_TEXT);
                                set_text(object, text.as_ptr());
                            }
                        }
                    }
                    crate::debug::report(&format!(
                        "studio_panel: Settings tab shows {cvar} {value} as \"{want}\""
                    ));
                }
                want
            }
            ChoiceStep::Nothing => shown,
        };
        match lent.choices.iter_mut().find(|(c, _, _)| *c == control) {
            Some(entry) => (entry.1, entry.2) = (now, wait),
            None => lent.choices.push((control, now, wait)),
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
                if vgui.still(CONSOLE, console) {
                    let (_, _, w, h) = vgui.rect(console);
                    vgui.place(console, (x, y, w, h));
                }
            }
        }
        for b in back {
            unsafe {
                if !vgui.still(b.source_name, b.source) {
                    continue; // that window is gone
                }
                vgui.set_parent(b.control, b.source);
                vgui.place(b.control, b.home);
            }
        }
    }

    unsafe fn park(vgui: &Vgui, bar: Vpanel, lent: &mut Lent) {
        // A bar other than the one recorded replaced it: the record is of a
        // deleted panel, never to be touched again.
        if lent.parked_from.is_some_and(|(kept, _, _)| kept != bar) {
            lent.parked_from = None;
        }
        if lent.revived.is_some_and(|kept| kept != bar) {
            lent.revived = None;
        }
        unsafe {
            let (x, y, w, h) = vgui.rect(bar);
            if lent.parked_from.is_none() && x != PARKED_AT {
                lent.parked_from = Some((bar, x, y));
            }
            if !vgui.visible(bar) {
                vgui.set_visible(bar, true);
                lent.revived = Some(bar);
            }
            vgui.place(bar, (PARKED_AT, PARKED_AT, w, h));
        }
        PARKED.store(bar, Ordering::Release);
    }

    unsafe fn unpark(vgui: &Vgui, lent: &mut Lent) {
        PARKED.store(0, Ordering::Release);
        let revived = lent.revived.take();
        if let Some((bar, x, y)) = lent.parked_from.take() {
            unsafe {
                if !vgui.still(VCR_BAR, bar) {
                    return;
                }
                let (_, _, w, h) = vgui.rect(bar);
                vgui.place(bar, (x, y, w, h));
                if revived == Some(bar) {
                    vgui.set_visible(bar, false);
                }
            }
        }
    }

    /// Whether our window is on screen showing its Console tab. If so, the
    /// window stays open (ESC brings it back with the menu) and the console
    /// window is made visible for the caller's `toggleconsole` to close the
    /// game's way, which goes back to the game.
    pub(super) fn back_to_game_if_on_console() -> bool {
        let Ok(vgui) = Vgui::get() else {
            return false;
        };
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
            if !vgui.shown(vp) {
                // The menu is closed around our window (ESC, or this key's
                // own trip back to the game): it is still open on the Console
                // tab, but not on screen, so the key opens it afresh. Without
                // this check, it took two presses. A console the
                // key left open behind the tab is closed the game's way first.
                if CONSOLE_OPENED_BY_KEY.swap(false, Ordering::AcqRel) {
                    if let Some(window) = vgui.popup(CONSOLE) {
                        vgui.set_visible(window, true);
                    }
                    crate::cmd_list::call_real(&super::REAL_TOGGLECONSOLE);
                }
                crate::debug::report(
                    "studio_panel: the console key found the Console tab behind a closed menu; opening it again",
                );
                return false;
            }
            CONSOLE_OPENED_BY_KEY.store(false, Ordering::Release);
            // Visible for the engine's toggle to see it open and hide it. It
            // is hidden again before anything draws.
            if let Some(window) = vgui.popup(CONSOLE) {
                vgui.set_visible(window, true);
            }
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
            CONSOLE_OPENED_BY_KEY.store(true, Ordering::Release);
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

    /// The last state [`log_state_changes`] logged, or `u32::MAX`.
    static LAST_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(u32::MAX);

    /// Logs the menu, our window and the console window each time one of them
    /// opens or closes: for a report of the DoD Studio menu item going back
    /// to the game, which nothing else in the log explains.
    unsafe fn log_state_changes(vgui: &Vgui, vp: Vpanel) {
        unsafe {
            let menu = vgui.shown(vgui.parent_of(vp));
            let window = vgui.visible(vp);
            let console = vgui.popup(CONSOLE).is_some_and(|c| vgui.visible(c));
            let by_key = CONSOLE_OPENED_BY_KEY.load(Ordering::Relaxed);
            let state =
                menu as u32 | (window as u32) << 1 | (console as u32) << 2 | (by_key as u32) << 3;
            if LAST_STATE.swap(state, Ordering::Relaxed) != state {
                crate::debug::report(&format!(
                    "studio_panel: state: menu {}, window {}, console window {}, console opened by the key {}",
                    menu as u8, window as u8, console as u8, by_key as u8
                ));
            }
        }
    }

    /// The main menu's DoD Studio item (`ui/GameMenu.res`). GameUI runs an
    /// `engine ...` item by queueing the command and then closing the menu,
    /// as Resume Game does, so the window opened behind a closed menu and the
    /// menu flashed back. Taken in [`taskbar_on_command`] instead,
    /// it opens at once, the way Options does.
    const MENU_COMMAND: &str = "engine ds_ui_panel 1";
    /// GameUI's `CTaskbar`, whose `OnCommand` runs the main menu's items,
    /// once [`hook_taskbar`] has given it ours; and the class's own.
    static TASKBAR_HOOKED: AtomicUsize = AtomicUsize::new(0);
    static TASKBAR_ON_COMMAND: AtomicUsize = AtomicUsize::new(0);
    static TASKBAR_TRIES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    /// Wraps `CTaskbar::OnCommand` (`FRAME_SLOT_ON_COMMAND` on both builds,
    /// `verify_studio_panel.py`) once the menu is up, trying every 30 frames
    /// until then. Without it, the item still works through the engine.
    fn hook_taskbar() {
        if TASKBAR_HOOKED.load(Ordering::Relaxed) != 0
            || !TASKBAR_TRIES
                .fetch_add(1, Ordering::Relaxed)
                .is_multiple_of(30)
            || gameui().is_err()
        {
            return;
        }
        let Ok(vgui) = Vgui::get() else { return };
        unsafe {
            let Ok(taskbar) = base_panel(&vgui) else {
                return;
            };
            let own = own_on_command(taskbar, taskbar_on_command as *const () as usize);
            TASKBAR_ON_COMMAND.store(own, Ordering::Release);
            TASKBAR_HOOKED.store(taskbar as usize, Ordering::Release);
            crate::debug::report(
                "studio_panel: the main menu's DoD Studio item opens the window directly",
            );
        }
    }

    /// `CTaskbar`'s `OnCommand`: the DoD Studio item opens the window here;
    /// everything else goes to GameUI's own.
    unsafe extern "thiscall" fn taskbar_on_command(this: *mut c_void, raw: *const c_char) {
        if text(raw) == MENU_COMMAND {
            let line = match toggle(Request::Open) {
                Ok(state) => format!("the menu's DoD Studio item: {state}"),
                Err(why) => {
                    format!("the menu's DoD Studio item could not open the window -- {why}")
                }
            };
            unsafe { crate::debug::report(&format!("studio_panel: {line}")) };
            return;
        }
        let own = TASKBAR_ON_COMMAND.load(Ordering::Acquire);
        if own != 0 {
            // Safety: the class's own OnCommand, from its vftable.
            let own: OnCommandFn = unsafe { std::mem::transmute(own) };
            unsafe { own(this, raw) };
        }
    }

    /// The game's own Load Demo window, just filled: its rows get short
    /// names (`demo_list_folders::name_stock_rows`). Not ours, nor the
    /// Highlights tab's hidden one.
    fn name_stock_demo_rows() {
        if !crate::demo_list_folders::stock_naming_pending() {
            return;
        }
        let Ok(vgui) = Vgui::get() else { return };
        let mut named = false;
        unsafe {
            for (vp, name) in vgui.gameui_popups() {
                if name != DEMO_LIST && name != STREAK_LIST {
                    let dialog = vgui.object(vp);
                    if crate::demo_list_folders::name_stock_rows(dialog) {
                        named = true;
                        sort_stock_rows(dialog);
                    }
                }
            }
        }
        crate::demo_list_folders::stock_naming_frame(named);
    }

    /// The game's own Load Demo window, its rows just named: sorts them
    /// again by their short names and starts the list at the top
    /// (`list_panel`, #796). The window sorted them by their whole paths,
    /// where a demo with a space comes quoted and `"` sorts before `.`, so
    /// `../` landed below the last demo (#790); renaming a row doesn't move
    /// it. With no column chosen (the window sorted each row in as it was
    /// added): by the names, ascending.
    unsafe fn sort_stock_rows(dialog: *mut c_void) {
        let Ok((_, build)) = gameui() else { return };
        unsafe {
            let list = *((dialog as *const u8).add(build.frame_size) as *const *mut c_void);
            if list.is_null() {
                return;
            }
            crate::list_panel::after_refill(list, build.list_sort_column, Some(0));
        }
    }

    pub(super) fn poll() {
        hook_taskbar();
        // The stock bar's progress needs no window of ours.
        demo_bar_progress::update();
        name_stock_demo_rows();
        load_strip::request_highlights();
        // The stock bar's slider needs no window of ours either: a demo
        // started from the launch line (previews, batches) plays before
        // the viewdemo wrapper is in, so the window never opens, and the
        // first ESC used to seek to the demo's start (#663).
        if let Ok(vgui) = Vgui::get() {
            unsafe { vcr_sync::update(&vgui) };
            // The review's form is a window of its own, over the demo with
            // the menu closed: it needs no window of ours either (#1139).
            unsafe { review_form::update(&vgui) };
        }
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
            refocus_console_entry(&vgui);
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
            log_state_changes(&vgui, vp);
            fit_sheet(&vgui, frame);
            LENT.with(|cell| {
                let Ok(mut lent) = cell.try_borrow_mut() else {
                    return;
                };
                hold_minimum(&vgui, frame, vp, &mut lent);
                if vgui.visible(vp) {
                    load_strip::update(&vgui, vp);
                    fit_pages(&vgui, &mut lent);
                    sync_settings(&vgui, &mut lent);
                    update_help(&vgui, vp, &mut lent);
                    filter_demo_list(&vgui);
                    streaks_tab::update(&vgui);
                    review_tab::update(&vgui);
                    player_picker::update(&vgui);
                    load_progress::update(&vgui);
                    folder_progress::update(&vgui);
                    sort_arrows::update();
                }
                if !vgui.visible(vp) {
                    // Closed some other way than the console key (its X,
                    // Close): the game still thinks the console the key
                    // opened is open, and ESC would bring it back. Close it
                    // the game's way, as the key does.
                    if CONSOLE_OPENED_BY_KEY.swap(false, Ordering::AcqRel) {
                        if let Some(window) = vgui.popup(CONSOLE) {
                            vgui.set_visible(window, true);
                        }
                        crate::cmd_list::call_real(&super::REAL_TOGGLECONSOLE);
                    }
                    give_back(&vgui, &mut lent, None);
                    // The bar stays off screen while the setting is on, even
                    // with our window closed: closing it by its X brought the
                    // stock bar back. ESC -> DoD Studio, or
                    // viewdemo, opens the window again.
                    match vgui.bar() {
                        Some(bar) if viewdemo_in_panel() => park(&vgui, bar, &mut lent),
                        _ => unpark(&vgui, &mut lent),
                    }
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
        let playback = PAGE_OBJECTS[PLAYBACK_PAGE].load(Ordering::Acquire) as *mut c_void;
        let result = match action(&command) {
            // Greyed out during a review (#826); refused here too, in case a
            // click lands before the lock greys the button.
            ref a
                if this == playback && crate::review_lock::locked() && action_moves_the_demo(a) =>
            {
                Err(crate::review_lock::LOCKED_NOTE.to_string())
            }
            // The Demos and Highlights tabs' borrowed Load / Go buttons say
            // "load" too, and their parent is the page now. Their own window
            // has already acted on it; switching tabs here sent Go to Demos.
            Action::Vcr("load")
                if this != PAGE_OBJECTS[PLAYBACK_PAGE].load(Ordering::Acquire) as *mut c_void =>
            {
                Ok(())
            }
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
            Action::ClearPlayers => {
                PICKED_PLAYERS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clear();
                Ok(())
            }
            Action::DemoBack => unsafe { navigate(crate::demo_list_folders::go_back) },
            Action::DemoForward => unsafe { navigate(crate::demo_list_folders::go_forward) },
            Action::DemoUp => unsafe { navigate(crate::demo_list_folders::go_up) },
            Action::ResetSettings => {
                crate::commands::console_print(&format!("{NAME}: {}\n", reset_settings()));
                Ok(())
            }
            Action::ScrollSettings(pages) => {
                SETTINGS_SCROLL_PAGES.fetch_add(pages, Ordering::AcqRel);
                Ok(())
            }
            Action::Engine(line) => {
                // A layout edited under an older DLL keeps its pre-#555
                // names: the layouts are ours, so they still run.
                let line = crate::names::current_command(line);
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

    /// After Up or Down on the Console tab, puts the focus back on the
    /// borrowed input line for a few frames. The console dialog's history
    /// step moves the focus a frame later, to our tab button and then the
    /// window (its input line's usual parent is the dialog, not our page), so
    /// the next key went nowhere: one Up, then nothing (#786).
    unsafe fn refocus_console_entry(vgui: &Vgui) {
        let left = CONSOLE_REFOCUS.load(Ordering::Acquire);
        if left == 0 {
            return;
        }
        CONSOLE_REFOCUS.store(left - 1, Ordering::Release);
        unsafe {
            let page = vpanel_of(PAGE_OBJECTS[CONSOLE_PAGE].load(Ordering::Acquire) as *mut c_void);
            if let Some(entry) = vgui.child_named(page, CONSOLE_ENTRY) {
                let focus: SetParentFn = slot(vgui.panel, IPANEL_REQUEST_FOCUS);
                focus(vgui.panel, entry, 0);
            }
        }
    }

    /// Each page's `OnKeyCodePressed`. On the Console tab, a key the borrowed
    /// input line passes up (Tab, the arrows) goes to the console dialog, its
    /// usual parent, which does the type-ahead and the command history.
    /// Enter stays here: the dialog's base `OnKeyCodePressed` posts `Hotkey`
    /// to its default button, the borrowed Submit, which ran the line a
    /// second time after [`page_on_key`]'s Submit had cleared it, echoing an
    /// empty `] ` (#989).
    unsafe extern "thiscall" fn page_on_key_pressed(this: *mut c_void, code: i32) {
        let console_page = PAGE_OBJECTS[CONSOLE_PAGE].load(Ordering::Acquire) as *mut c_void;
        if this == console_page && (code == KEY_ENTER || code == KEY_PAD_ENTER) {
            return;
        }
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
                    if code == KEY_UP || code == KEY_DOWN {
                        CONSOLE_REFOCUS.store(3, Ordering::Release);
                    }
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
        // Enter in the box lands here without a button (#826).
        if crate::review_lock::locked() {
            return Err(crate::review_lock::LOCKED_NOTE.to_string());
        }
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
            // Enter anywhere on the tab lands here (#777): an empty box is
            // not a mistake to report, just nothing to do.
            if typed.trim().is_empty() {
                return Ok(());
            }
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

    /// Opens the window on tab `page`, building it if needed. With
    /// `bring_menu_up`, a menu that is down comes up around it: a tab named
    /// by a bind or by the review (#623) runs during play, with the menu
    /// closed.
    pub(super) fn open_on(page: usize, bring_menu_up: bool) -> Result<String, String> {
        let vgui = Vgui::get()?;
        unsafe {
            let (object, vp, _) = ensure_window(&vgui, false)?;
            if page == DEMOS_PAGE {
                refill_demo_list();
            }
            if bring_menu_up && !vgui.shown(vgui.parent_of(vp)) {
                activate_game_ui();
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
                Request::Open | Request::Reset | Request::Tab(_) => false,
            };
            if close {
                vgui.set_visible(vp, false);
                notes.push("closed".to_string());
            } else {
                // The main menu's DoD Studio item is an `engine` command, and
                // GameUI closes the menu after running one, as Resume Game
                // does. The window lives in the menu, so bring
                // the menu back up.
                if !vgui.shown(vgui.parent_of(vp)) {
                    notes.push(activate_game_ui());
                }
                notes.push(show(&vgui, object, vp, None)?);
            }
            Ok(notes.join("; "))
        }
    }

    /// Brings the main menu up through the engine's `IBaseUI`.
    fn activate_game_ui() -> String {
        let Some(base_ui) = module(c"hw.dll").and_then(|hw| interface(hw, c"BaseUI001")) else {
            return "could not bring the menu up: no BaseUI001".to_string();
        };
        unsafe {
            let activate: ActivateFn = slot(base_ui, BASEUI_SLOT_ACTIVATE_GAME_UI);
            activate(base_ui);
        }
        "brought the menu up".to_string()
    }

    /// See the outer `menu_state`.
    pub(super) fn menu_state() -> Option<(bool, bool, bool)> {
        let vgui = Vgui::get().ok()?;
        // Safety: handles read from vgui2 on the engine's thread, as poll does.
        unsafe {
            let (_, vp) = window(&vgui)?;
            let console = vgui.popup(CONSOLE).is_some_and(|c| vgui.visible(c));
            Some((vgui.shown(vgui.parent_of(vp)), vgui.visible(vp), console))
        }
    }

    /// Closes the window and, when the menu around it is up, the menu too, as
    /// Resume Game does.
    pub(super) fn close_for_playback() {
        let Ok(vgui) = Vgui::get() else { return };
        unsafe {
            let Some((_, vp)) = window(&vgui) else { return };
            vgui.set_visible(vp, false);
            if !vgui.shown(vgui.parent_of(vp)) {
                return;
            }
            let Some(base_ui) = module(c"hw.dll").and_then(|hw| interface(hw, c"BaseUI001")) else {
                return;
            };
            let hide: ActivateFn = slot(base_ui, BASEUI_SLOT_HIDE_GAME_UI);
            hide(base_ui);
        }
    }
}

/// Whether the menu is up and, with it, our window or the console window:
/// `(menu, window, console)`, or `None` before the window was first built.
/// For the review's ESC (#660): vgui2 takes ESC before any client key hook
/// sees it, so the review watches the menu come up instead.
pub(crate) fn menu_state() -> Option<(bool, bool, bool)> {
    #[cfg(target_arch = "x86")]
    return hook::menu_state();
    #[cfg(not(target_arch = "x86"))]
    None
}

/// Set by the review (#623) when a highlight starts playing; the next
/// [`poll`] closes the window and the menu, outside the review's lock.
static CLOSE_FOR_PLAYBACK: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Gets the window and the menu out of the way of playback on the next frame.
pub(crate) fn close_for_playback() {
    CLOSE_FOR_PLAYBACK.store(true, Ordering::Release);
}

/// Keeps the tab strip sized to the window, swaps in our window after
/// `viewdemo` or the console key, and wraps `toggleconsole` once the engine
/// has it. Called every frame from `commands::poll`; a few atomic loads until
/// the window has been opened.
pub fn poll() {
    wrap_toggleconsole();
    apply_saved_settings();
    #[cfg(target_arch = "x86")]
    {
        hook::poll();
        // After the loans have moved for this frame (#826).
        hook::lock_controls::poll();
        if CLOSE_FOR_PLAYBACK.swap(false, Ordering::AcqRel) {
            hook::close_for_playback();
        }
    }
}

/// The Review tab's From, To and Note boxes (#623), when they belong to the
/// highlight the review is on.
pub(crate) fn review_inputs() -> Option<(String, String, String)> {
    #[cfg(target_arch = "x86")]
    return hook::review_tab::inputs();
    #[cfg(not(target_arch = "x86"))]
    None
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

/// What `ds_ui_panel` was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    /// Bare: open it when closed, close it when open.
    Toggle,
    Open,
    Close,
    /// Write the default layouts back and rebuild it, open.
    Reset,
    /// Open it on this tab (its place in [`PAGES`]).
    Tab(usize),
}

fn request(argument: Option<&str>) -> Result<Request, String> {
    match argument {
        None => Ok(Request::Toggle),
        Some("1") | Some("open") => Ok(Request::Open),
        Some("0") | Some("close") => Ok(Request::Close),
        Some("reset") => Ok(Request::Reset),
        Some(other) => PAGES
            .iter()
            .position(|page| page.name.to_string_lossy().eq_ignore_ascii_case(other))
            .map(Request::Tab)
            .ok_or_else(|| {
                format!(
                    "unknown argument {other:?} -- ds_ui_panel [1|0|reset|<tab>]: bare opens or closes it, 1 opens, 0 closes, reset restores the default layouts, a tab's name opens it on that tab"
                )
            }),
    }
}

/// `ds_ui_panel [1|0|reset|<tab>]`: bare opens the window or closes it,
/// `1` opens it, `0` closes it, `reset` writes the default layouts back and
/// rebuilds it, a tab's name (`highlights`) opens it on that tab.
pub unsafe extern "C" fn command() {
    let result: Result<String, String> = request(argument().as_deref()).and_then(|request| {
        #[cfg(target_arch = "x86")]
        if let Request::Tab(page) = request {
            return hook::open_on(page, true);
        }
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
    fn a_column_minimum_is_never_above_its_width() {
        // Resizable: Valve's 20..10000, the minimum lowered to a narrower width.
        assert_eq!(column_width_range(100, 0), (20, 10_000));
        assert_eq!(column_width_range(12, 0), (12, 10_000));
        // Resizes with the window (2) without the fixed flag: still a range.
        assert_eq!(column_width_range(40, 2), (20, 10_000));
        // Fixed size (1, and the icon column's 1 | 4): the width both ways.
        assert_eq!(column_width_range(22, 1 | 4), (22, 22));
        // Fixed and resize-with-window together is not fixed, as in GameUI.
        assert_eq!(column_width_range(22, 3), (20, 10_000));
        for width in [1, 20, 40, 42, 210, 20_000] {
            for flags in [0, 1, 2, 3, 4, 5] {
                let (min, max) = column_width_range(width, flags);
                assert!(min <= width && width <= max, "{width} {flags}");
            }
        }
    }

    #[test]
    fn each_drag_handle_names_its_column_position() {
        // The Demos tab: Demo File was record 0 at position 0, the icons'
        // record 1 went in at position 0, then Map..Date were appended.
        let order = [1u8, 0, 2, 3, 4, 5];
        let got: Vec<_> = dragger_positions(&order, 6).collect();
        assert_eq!(got, [(1, 0), (0, 1), (2, 2), (3, 3), (4, 4), (5, 5)]);
        // A record index past the allocated records is skipped, not read.
        let got: Vec<_> = dragger_positions(&[0, 7, 1], 2).collect();
        assert_eq!(got, [(0, 0), (1, 2)]);
        assert_eq!(dragger_positions(&[], 0).count(), 0);
    }

    #[test]
    fn the_layouts_follow_the_hd_files_into_dod_addon_only_with_addons() {
        // -addons: dod_addon unless only dod has the folder.
        assert!(ui_in_addon(true, true, false));
        assert!(ui_in_addon(true, true, true));
        assert!(ui_in_addon(true, false, false));
        assert!(!ui_in_addon(true, false, true));
        // Without it the engine never reads dod_addon.
        assert!(!ui_in_addon(false, true, false));
        assert!(!ui_in_addon(false, true, true));
    }

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
        assert!(boxes.contains(&"ds_ui_viewdemo_in_panel"));
        assert!(boxes.contains(&"ds_ui_console_in_panel"));
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
                let full = format!("{}{name}", console_name!(""));
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
    fn the_command_takes_1_0_or_reset() {
        assert_eq!(request(None), Ok(Request::Toggle));
        assert_eq!(request(Some("1")), Ok(Request::Open));
        assert_eq!(request(Some("0")), Ok(Request::Close));
        assert_eq!(request(Some("reset")), Ok(Request::Reset));
        assert_eq!(request(Some("highlights")), Ok(Request::Tab(STREAKS_PAGE)));
        assert_eq!(request(Some("demos")), Ok(Request::Tab(DEMOS_PAGE)));
        assert!(request(Some("2")).is_err());
    }

    #[test]
    fn button_commands_are_sorted_into_what_they_do() {
        assert_eq!(action("play"), Action::Vcr("play"));
        assert_eq!(action("Faster"), Action::Vcr("faster"));
        assert_eq!(
            action("engine ds_debug_status"),
            Action::Engine("ds_debug_status")
        );
        assert_eq!(action("Close"), Action::Own);
        assert_eq!(action("goto"), Action::Goto);
        assert_eq!(action("demo_back"), Action::DemoBack);
        assert_eq!(action("Demo_Forward"), Action::DemoForward);
        assert_eq!(action("demo_up"), Action::DemoUp);
        assert_eq!(action("reset_settings"), Action::ResetSettings);
        assert_eq!(action("settings_up"), Action::ScrollSettings(-1));
        assert_eq!(action("settings_down"), Action::ScrollSettings(1));
    }

    #[test]
    fn the_settings_tab_scrolls_between_its_top_and_its_last_row() {
        // 22-pixel lines, a 244-pixel view of 800 pixels of rows.
        assert_eq!(settings_scroll(0, 3, 0, 22, 244, 800), 66);
        assert_eq!(settings_scroll(0, -3, 0, 22, 244, 800), 0);
        assert_eq!(settings_scroll(0, 0, 1, 22, 244, 800), 222);
        assert_eq!(settings_scroll(500, 0, 1, 22, 244, 800), 556);
        assert_eq!(settings_scroll(556, 3, 0, 22, 244, 800), 556);
        // A window tall enough for every row doesn't scroll.
        assert_eq!(settings_scroll(100, 3, 1, 22, 900, 800), 0);
    }

    /// Every Playback tab button that moves the demo is refused during a
    /// review (#826); the decals and "use this tab" buttons are not.
    #[test]
    fn a_review_refuses_the_playback_buttons_that_move_the_demo() {
        let res = include_str!("../ui/Playback.res");
        let mut commands = res
            .lines()
            .filter_map(|l| l.trim().strip_prefix("\"Command\""))
            .map(|v| v.trim().trim_matches('"').to_string())
            .collect::<Vec<_>>();
        commands.push("goto".to_string());
        assert!(commands.len() > 10, "{commands:?}");
        for command in &commands {
            let keeps = command.contains("clear_decals") || command.contains("viewdemo_in_panel");
            assert_eq!(action_moves_the_demo(&action(command)), !keeps, "{command}");
        }
    }

    #[test]
    fn the_loading_strip_names_each_load_still_running() {
        assert_eq!(
            load_strip_text(Some(45), Some(60)).as_deref(),
            Some("Loading: Demo 45%   Highlights 60%")
        );
        assert_eq!(
            load_strip_text(Some(0), None).as_deref(),
            Some("Loading: Demo 0%")
        );
        assert_eq!(
            load_strip_text(None, Some(100)).as_deref(),
            Some("Loading: Highlights 100%")
        );
        assert_eq!(load_strip_text(None, None), None);
    }

    #[test]
    fn the_loading_strip_sits_above_the_controls_it_pushes_down() {
        let (x, y, w, h) = load_strip_bounds(540);
        assert_eq!((x, w), (8, 524));
        assert!(y >= 0 && y + h <= LOAD_STRIP_TALL);
        assert!(load_strip_bounds(4).2 >= 1);
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
        assert_eq!(PAGES[DEMOS_PAGE].name, c"Demos");
        assert_eq!(PAGES[STREAKS_PAGE].name, c"Highlights");
        assert_eq!(PAGES[CONSOLE_PAGE].name, c"Console");
        assert_eq!(PAGES[SETTINGS_PAGE].name, c"Settings");
        assert_eq!(PAGES[COMMANDS_PAGE].name, c"Commands");
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

//! Studio preferences (Studio v3.1): a centred dialog inside the Studio with the tabs
//! General, Appearance, Saving and export, Copy and paste, Editing and Shortcuts. It is
//! THE one source of the Studio's settings rows (Settings > Studio is only a button that
//! opens it). Changes take effect and save by themselves; Esc / Close closes it.
//!
//! The host (the Studio window) owns a `Dialog` while it is open:
//! `Dialog::open(default_folder)`, show `view(window)` mapped to its own message, feed every `Msg` to
//! `update`, and act on the `Out` (`Changed`: persist with
//! `config::update(|c| dialog.apply(c))`; `OpenGeneral`: open Settings > General; `Close`: drop it). `theme_of` gives the Studio window's theme.
use iced::widget::{column, container};
use iced::{Element, Fill, Length, Size, widget::tooltip::Position};

use crate::features::snip::screens::studio::keys;
use crate::shared::app_language::{S, tr};
use crate::shared::config::ZoomStep;
use crate::shared::config::{
    self, Config, EditorConfig, Format, STUDIO_THEME_FOLLOW, StudioBackdrop, StudioOpenIn,
    StudioPrefs, StudioSide, StudioWheel, StudioZoom,
};
use crate::shared::kit::button::{self, Kind, label};
use crate::shared::kit::dropdown::dropdown;
use crate::shared::kit::field::field;
use crate::shared::kit::scroll::scroll;
use crate::shared::kit::segmented::{boolean, segmented};
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{error, muted};
use crate::shared::kit::themes::{self, THEMES};
use crate::shared::kit::tokens::{Role, radius, space};
use crate::shared::kit::tooltip::tip;
use crate::shared::kit::{divider, modal, option_list};
use crate::t;

/// The dialog's size (logical px); it never exceeds the window less `MARGIN`.
const WIDTH: f32 = 960.0;
const HEIGHT: f32 = 640.0;
const WIDE: f32 = 1280.0;
const TALL: f32 = 900.0;
/// The section list's width.
const NAV: f32 = 232.0;
const MARGIN: f32 = 12.0;

/// The card's size: `WIDTH` x `HEIGHT` in a normal window, growing with a big one (half
/// its width up to `WIDE`, 70 % of its height up to `TALL`), never past the window less
/// `MARGIN`.
fn card_size(window: Size) -> (f32, f32) {
    let w = (window.width * 0.5).clamp(WIDTH, WIDE);
    let h = (window.height * 0.7).clamp(HEIGHT, TALL);
    (
        w.min(window.width - 2.0 * MARGIN).max(0.0),
        h.min(window.height - 2.0 * MARGIN).max(0.0),
    )
}

/// The rows' column never grows past this: a label and its control stay together in a big window.
const CONTENT: f32 = 720.0;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    General,
    Appearance,
    Saving,
    CopyPaste,
    Editing,
    Shortcuts,
}

impl Tab {
    const ALL: [Tab; 6] = [
        Tab::General,
        Tab::Appearance,
        Tab::Saving,
        Tab::CopyPaste,
        Tab::Editing,
        Tab::Shortcuts,
    ];

    /// The one line under the section's title.
    fn subtitle(self) -> &'static str {
        match self {
            Tab::General => t!(pf_sub_general),
            Tab::Appearance => t!(pf_sub_appearance),
            Tab::Saving => t!(pf_sub_saving),
            Tab::CopyPaste => t!(pf_sub_copy),
            Tab::Editing => t!(pf_sub_editing),
            Tab::Shortcuts => t!(pf_sub_shortcuts),
        }
    }

    /// Its icon (lucide, from the bundled subset).
    fn icon(self) -> char {
        match self {
            Tab::General => '\u{e157}',    // settings
            Tab::Appearance => '\u{e5a0}', // blend
            Tab::Saving => '\u{e0b6}',     // download
            Tab::CopyPaste => '\u{e3eb}',  // clipboard-paste
            Tab::Editing => '\u{e1f8}',    // pencil
            Tab::Shortcuts => '\u{e0a5}',  // corner-down-left (the Enter key)
        }
    }

    fn name(self) -> &'static str {
        match self {
            Tab::General => t!(nav_general),
            Tab::Appearance => t!(nav_appearance),
            Tab::Saving => t!(pf_tab_saving),
            Tab::CopyPaste => t!(pf_tab_copy),
            Tab::Editing => t!(pf_tab_editing),
            Tab::Shortcuts => t!(st_shortcuts),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Msg {
    Tab(Tab),
    /// The Studio's settings with one changed.
    Prefs(Box<StudioPrefs>),
    /// Save size (`snip.max_edge`).
    MaxEdge(u32),
    /// Save folder's button: Settings opens on General (the one home of the folder).
    OpenGeneral,
    ResetToolStyles,
    /// Interface size (percent, one of `CHAT_ZOOM`): the Studio sets it (C3).
    UiZoom(u16),
    /// Keyboard shortcuts: record a new key for a command (its field was pressed), the
    /// chord pressed, back to the default key, every key back to the defaults.
    Record(keys::Cmd),
    Recorded(String),
    ResetKey(keys::Cmd),
    ResetKeys,
    Close,
}

/// What the host does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    None,
    /// Something changed: persist with `Dialog::apply` and re-read the config.
    Changed,
    /// Open Settings > General (the Save folder).
    OpenGeneral,
    Close,
}

/// A theme in the dropdown: its id (`"follow"` or a theme's) and its name.
#[derive(Debug, Clone, PartialEq)]
struct Choice {
    id: &'static str,
    name: String,
}

impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

pub struct Dialog {
    tab: Tab,
    prefs: StudioPrefs,
    max_edge: u32,
    /// The editor's remembered tool options differ from the defaults.
    tool_styles_set: bool,
    reset_styles: bool,
    /// What this dialog last read or wrote: `apply` lays only what differs from it
    /// on the file, so a setting changed elsewhere meanwhile survives.
    opened: (StudioPrefs, u32),
    themes: Vec<Choice>,
    /// The Studio's interface size now, percent (the Studio keeps it: `Msg::UiZoom`).
    pub ui_zoom: u16,
    /// The command whose new key is awaited, and a chord that was refused (and why).
    recording: Option<keys::Cmd>,
    refused: Option<(keys::Cmd, String, keys::Taken)>,
}

impl Dialog {
    /// Opens on General with the running config.
    pub fn open() -> Self {
        Self::from(&config::current())
    }

    fn from(c: &Config) -> Self {
        let follow = Choice {
            id: STUDIO_THEME_FOLLOW,
            name: tr(S::pf_theme_follow).to_string(),
        };
        let themes = std::iter::once(follow)
            .chain(THEMES.iter().map(|t| Choice {
                id: t.id,
                name: t.name.to_string(),
            }))
            .collect();
        Dialog {
            tab: Tab::General,
            prefs: c.studio.clone(),
            max_edge: c.snip.max_edge,
            tool_styles_set: c.editor != default_styles(&c.editor),
            reset_styles: false,
            opened: (c.studio.clone(), c.snip.max_edge),
            themes,
            ui_zoom: c.snip.studio.ui_zoom,
            recording: None,
            refused: None,
        }
    }

    #[cfg(test)]
    pub fn prefs(&self) -> &StudioPrefs {
        &self.prefs
    }

    #[cfg(test)]
    pub fn tab(&self) -> Tab {
        self.tab
    }

    pub fn update(&mut self, msg: Msg) -> Out {
        match msg {
            Msg::Tab(tab) => {
                self.tab = tab;
                Out::None
            }
            Msg::Prefs(next) => {
                if self.prefs.keep_unsaved && !next.keep_unsaved {
                    // off deletes the private copy, as with chats and captures
                    super::recovery::discard();
                }
                self.prefs = *next;
                Out::Changed
            }
            Msg::MaxEdge(px) => {
                self.max_edge = px;
                Out::Changed
            }
            Msg::UiZoom(z) => {
                self.ui_zoom = z; // (the Studio acts on it before this)
                Out::None
            }
            Msg::OpenGeneral => Out::OpenGeneral,
            Msg::ResetToolStyles => {
                (self.reset_styles, self.tool_styles_set) = (true, false);
                Out::Changed
            }
            Msg::Record(cmd) => {
                self.recording = Some(cmd);
                self.refused = None;
                Out::None
            }
            Msg::Recorded(chord) => {
                let Some(cmd) = self.recording.take() else {
                    return Out::None;
                };
                if chord == cmd.default_chord() {
                    return self.set_key(cmd, None);
                }
                match keys::taken(cmd, &chord) {
                    Some(why) => {
                        self.refused = Some((cmd, chord, why));
                        Out::None
                    }
                    None => self.set_key(cmd, Some(chord)),
                }
            }
            Msg::ResetKey(cmd) => self.set_key(cmd, None),
            Msg::ResetKeys => {
                self.recording = None;
                self.refused = None;
                self.prefs.shortcuts.clear();
                Out::Changed
            }
            Msg::Close => Out::Close,
        }
    }

    /// A key for `cmd` (`None`: its default again).
    fn set_key(&mut self, cmd: keys::Cmd, chord: Option<String>) -> Out {
        self.refused = None;
        match chord {
            Some(chord) => self.prefs.shortcuts.insert(cmd.id().to_string(), chord),
            None => self.prefs.shortcuts.remove(cmd.id()),
        };
        Out::Changed
    }

    /// Opens on the Keyboard shortcuts page.
    pub fn show_shortcuts(&mut self) {
        self.tab = Tab::Shortcuts;
    }

    /// A new key is awaited: the host sends the next chord (`Recorded`) and Esc stops it.
    pub fn recording(&self) -> bool {
        self.recording.is_some()
    }

    /// Esc while a key is awaited: stop waiting (the dialog stays).
    pub fn stop_recording(&mut self) {
        self.recording = None;
    }

    /// Lays the dialog's changes on `c` (what `Out::Changed` asks for): only the keys
    /// the user changed here, so a setting changed elsewhere meanwhile (another
    /// `[studio]` key, `snip.max_edge`, Settings) survives. A file name the config
    /// refuses is not written (the row says why).
    pub fn apply(&mut self, c: &mut Config) {
        use toml::{Table, Value};
        let table = |v: Result<Value, toml::ser::Error>| match v {
            Ok(Value::Table(t)) => t,
            _ => Table::new(),
        };
        let (old, new) = (
            table(Value::try_from(&self.opened.0)),
            table(Value::try_from(&self.prefs)),
        );
        let mut file = table(Value::try_from(&*c));
        let studio = match file
            .entry("studio")
            .or_insert_with(|| Value::Table(Table::new()))
        {
            Value::Table(t) => t,
            _ => return,
        };
        let refused = config::check_studio_file_name(&self.prefs.file_name).is_err();
        for (k, v) in &new {
            let kept = k == "file_name" && refused;
            if old.get(k) != Some(v) && !kept {
                studio.insert(k.clone(), v.clone());
            }
        }
        // a key the dialog emptied (skipped when empty, e.g. every shortcut reset) leaves the file
        for k in old.keys().filter(|k| !new.contains_key(*k)) {
            studio.remove(k);
        }
        if self.max_edge != self.opened.1
            && let Some(Value::Table(snip)) = file.get_mut("snip")
        {
            snip.insert("max_edge".into(), Value::Integer(i64::from(self.max_edge)));
        }
        if let Ok(text) = toml::to_string(&file) {
            let reset = std::mem::take(&mut self.reset_styles);
            *c = Config::parse(&text).0;
            if reset {
                c.editor = default_styles(&c.editor);
            }
        }
        let name = (refused).then(|| self.opened.0.file_name.clone());
        self.opened = (self.prefs.clone(), self.max_edge);
        if let Some(name) = name {
            self.opened.0.file_name = name;
        }
    }

    /// The dialog: a list of sections on the left; on the right the section's title and
    /// one-line subtitle with the X at the top, then its rows in small-headed groups.
    pub fn view(&self, window: Size) -> Element<'_, Msg> {
        let nav = column(Tab::ALL.map(|t| {
            crate::shared::kit::side_panel::section(t.icon(), t.name(), t == self.tab, Msg::Tab(t))
        }))
        .spacing(crate::shared::kit::side_panel::SECTION_GAP)
        .padding(space::SIDEBAR_PAD);
        // the list is part of the card, not a box beside it: a recessed column on the card's
        // start side, as tall as the card, its outer corners the card's own
        let nav =
            crate::shared::kit::side_panel::inset(scroll(nav).into(), NAV, radius::CARD - 1.0);
        let head = container(modal::header(
            self.tab.name(),
            Some(self.tab.subtitle()),
            Msg::Close,
        ))
        .padding([space::PAGE[1], space::PAGE[1]]);
        // the rows sit in one centred column at most CONTENT wide, so a label and its control
        // stay together however wide the dialog grows
        let page = container(container(self.rows()).width(Fill).max_width(CONTENT)).center_x(Fill);
        let body = scroll(container(page).padding([0.0, space::PAGE[1]])).height(Length::Fill);
        let page = column![head, divider::divider(), body].width(Fill);
        let content = crate::dir_row![nav, divider::vertical(Fill), page];
        let (w, h) = card_size(window);
        surface(Material::Widget, radius::CARD, content)
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .into()
    }

    /// The rows of the open section, in groups with a small heading each.
    fn rows(&self) -> Element<'_, Msg> {
        let p = &self.prefs;
        let edit = move |change: &dyn Fn(&mut StudioPrefs)| {
            let mut next = p.clone();
            change(&mut next);
            Msg::Prefs(Box::new(next))
        };
        let row = |name, desc, control| setting_row::plain(title(name), desc, control);
        let switch = move |name, desc, on: bool, set: fn(&mut StudioPrefs, bool)| {
            row(
                name,
                desc,
                boolean(name, [t!(c_off), t!(c_on)], on, move |v| {
                    edit(&|s| set(s, v))
                }),
            )
        };
        let format_row = |label, desc, now: Format, set: fn(&mut StudioPrefs, Format)| {
            row(
                label,
                desc,
                segmented(
                    &[(Format::Png, t!(s_png)), (Format::Jpeg, t!(s_jpeg))],
                    now,
                    now,
                    1.0,
                    move |f| edit(&|s| set(s, f)),
                ),
            )
        };
        type Group<'a> = (&'static str, Vec<Element<'a, Msg>>);
        let groups: Vec<Group<'_>> = match self.tab {
            Tab::General => vec![
                (
                    t!(pf_grp_opening),
                    vec![
                        row(
                            t!(ss_open_in),
                            t!(ss_open_in_d),
                            segmented(
                                &[
                                    (StudioOpenIn::Quick, t!(ss_quick)),
                                    (StudioOpenIn::Studio, t!(ss_studio)),
                                ],
                                p.open_in,
                                p.open_in,
                                1.0,
                                move |v| edit(&|s| s.open_in = v),
                            ),
                        ),
                        switch(t!(ss_reopen), t!(ss_reopen_d), p.reopen_last, |s, v| {
                            s.reopen_last = v
                        }),
                        switch(
                            t!(pf_start_screen),
                            t!(pf_start_screen_d),
                            p.start_screen,
                            |s, v| s.start_screen = v,
                        ),
                        row(
                            t!(ss_recent),
                            t!(ss_recent_d),
                            segmented(
                                &[(0, t!(c_off)), (10, "10"), (20, "20")],
                                p.recent_files,
                                p.recent_files,
                                1.0,
                                move |v| edit(&|s| s.recent_files = v),
                            ),
                        ),
                    ],
                ),
                (
                    t!(pf_grp_sidebar),
                    vec![
                        switch(
                            t!(pf_steps_start),
                            t!(pf_steps_start_d),
                            p.steps_on_start,
                            |s, v| s.steps_on_start = v,
                        ),
                        row(
                            t!(pf_steps_side),
                            t!(pf_steps_side_d),
                            segmented(
                                &[
                                    (StudioSide::Start, t!(pf_side_start)),
                                    (StudioSide::End, t!(pf_side_end)),
                                ],
                                p.steps_side,
                                p.steps_side,
                                1.0,
                                move |v| edit(&|s| s.steps_side = v),
                            ),
                        ),
                    ],
                ),
                (
                    t!(pf_grp_closing),
                    vec![
                        switch(
                            t!(ss_ask_unsaved),
                            t!(ss_ask_unsaved_d),
                            p.ask_unsaved,
                            |s, v| s.ask_unsaved = v,
                        ),
                        switch(t!(ss_keep), t!(ss_keep_d), p.keep_unsaved, |s, v| {
                            s.keep_unsaved = v
                        }),
                    ],
                ),
            ],
            Tab::Appearance => {
                let chosen = self.themes.iter().find(|c| c.id == p.theme).cloned();
                vec![(
                    t!(pf_grp_theme),
                    vec![
                        row(
                            t!(pf_theme),
                            t!(pf_theme_d),
                            dropdown(&self.themes, chosen, t!(pf_theme_follow), move |c| {
                                edit(&|s| s.theme = c.id.to_string())
                            }),
                        ),
                        row(
                            t!(st_sc_ui),
                            t!(pf_ui_size_d),
                            dropdown(&ZoomStep::ALL, Some(ZoomStep(self.ui_zoom)), "", |z| {
                                Msg::UiZoom(z.0)
                            }),
                        ),
                        row(
                            t!(ss_bg),
                            t!(ss_bg_d),
                            segmented(
                                &[
                                    (StudioBackdrop::Theme, t!(ss_bg_theme)),
                                    (StudioBackdrop::Checkerboard, t!(ss_bg_check)),
                                    (StudioBackdrop::Dark, t!(ss_bg_dark)),
                                ],
                                p.background,
                                p.background,
                                1.0,
                                move |v| edit(&|s| s.background = v),
                            ),
                        ),
                    ],
                )]
            }
            Tab::Saving => {
                let mut name = column![tip(
                    container(field(
                        &p.file_name,
                        Some(move |v: String| edit(&|s| s.file_name = v.clone())),
                        Msg::Tab(self.tab), // (Enter changes nothing; it must not close)
                    ))
                    .width(Fill),
                    t!(ss_name),
                    Position::Top
                )];
                if let Err(e) = config::check_studio_file_name(&p.file_name) {
                    name = name.push(error(Role::ROW_DESC, e));
                }
                vec![
                    (
                        t!(pf_grp_where),
                        vec![
                            row(t!(ss_name), t!(ss_name_d), name.spacing(space::GAP).into()),
                            switch(t!(ss_ask_where), t!(ss_ask_where_d), p.ask_where, |s, v| {
                                s.ask_where = v
                            }),
                            switch(t!(ss_copy_path), t!(ss_copy_path_d), p.copy_path, |s, v| {
                                s.copy_path = v
                            }),
                        ],
                    ),
                    (
                        t!(pf_grp_file),
                        vec![
                            format_row(t!(ss_format), t!(ss_format_d), p.format, |s, f| {
                                s.format = f
                            }),
                            row(
                                t!(ss_quality),
                                t!(ss_quality_d),
                                segmented(
                                    &[(70, "70 %"), (85, "85 %"), (95, "95 %")],
                                    p.quality,
                                    p.quality,
                                    1.0,
                                    move |v| edit(&|s| s.quality = v),
                                ),
                            ),
                            row(
                                t!(ss_size),
                                t!(ss_size_d),
                                segmented(
                                    &config::size_choices(),
                                    self.max_edge,
                                    self.max_edge,
                                    1.0,
                                    Msg::MaxEdge,
                                ),
                            ),
                            row(
                                t!(ss_folder),
                                t!(ss_folder_d),
                                setting_row::action(
                                    t!(ss_folder_open),
                                    Kind::Elevated,
                                    Some(Msg::OpenGeneral),
                                ),
                            ),
                        ],
                    ),
                ]
            }
            Tab::CopyPaste => vec![
                (
                    t!(pf_grp_copy),
                    vec![switch(
                        t!(ss_ctrlc),
                        t!(ss_ctrlc_d),
                        p.ctrl_c_picture,
                        |s, v| s.ctrl_c_picture = v,
                    )],
                ),
                (
                    t!(pf_grp_paste),
                    vec![
                        switch(
                            t!(ss_paste_obj),
                            t!(ss_paste_obj_d),
                            p.paste_movable,
                            |s, v| s.paste_movable = v,
                        ),
                        switch(t!(ss_grow), t!(ss_grow_d), p.grow_canvas, |s, v| {
                            s.grow_canvas = v
                        }),
                    ],
                ),
                (
                    t!(pf_grp_drag),
                    vec![format_row(
                        t!(ss_drag),
                        t!(ss_drag_d),
                        p.drag_type,
                        |s, f| s.drag_type = f,
                    )],
                ),
            ],
            Tab::Editing => {
                let (desc, press) = match self.tool_styles_set {
                    true => (t!(ss_tool_styles_d), Some(Msg::ResetToolStyles)),
                    false => (t!(ss_tool_styles_done), None),
                };
                vec![
                    (
                        t!(pf_grp_tools),
                        vec![
                            row(
                                t!(ss_tool_styles),
                                desc,
                                setting_row::action(t!(c_reset), Kind::Elevated, press),
                            ),
                            switch(t!(ss_snap), t!(ss_snap_d), p.snap, |s, v| s.snap = v),
                            switch(t!(ss_grid), t!(ss_grid_d), p.pixel_grid, |s, v| {
                                s.pixel_grid = v
                            }),
                        ],
                    ),
                    (
                        t!(pf_grp_view),
                        vec![
                            row(
                                t!(ss_zoom),
                                t!(ss_zoom_d),
                                segmented(
                                    &[
                                        (StudioZoom::Fit, t!(ss_zoom_fit)),
                                        (StudioZoom::Actual, t!(ss_zoom_100)),
                                    ],
                                    p.zoom_on_open,
                                    p.zoom_on_open,
                                    1.0,
                                    move |v| edit(&|s| s.zoom_on_open = v),
                                ),
                            ),
                            row(
                                t!(ss_wheel),
                                t!(ss_wheel_d),
                                segmented(
                                    &[
                                        (StudioWheel::Scroll, t!(ss_wheel_scroll)),
                                        (StudioWheel::Zoom, t!(ss_wheel_zoom)),
                                    ],
                                    p.wheel,
                                    p.wheel,
                                    1.0,
                                    move |v| edit(&|s| s.wheel = v),
                                ),
                            ),
                        ],
                    ),
                ]
            }
            Tab::Shortcuts => self.shortcut_groups(),
        };
        let mut page = column![].spacing(space::PAGE[1]);
        for (heading, rows) in groups {
            let mut group = column![]
                .spacing(space::GAP)
                .align_x(crate::shared::kit::dir::start());
            if !heading.is_empty() {
                group =
                    group.push(container(muted(Role::SECTION, heading)).padding([0.0, space::GAP]));
            }
            page = page.push(group.push(setting_row::card(rows)));
        }
        container(page).padding([space::PAGE[1], 0.0]).into()
    }

    /// Keyboard shortcuts: each command's key is a field to press and record, with Reset;
    /// a refused key says why; the editor's fixed keys follow as a reference.
    fn shortcut_groups(&self) -> Vec<(&'static str, Vec<Element<'_, Msg>>)> {
        use crate::shared::kit::shortcut_field::{State, shortcut_field};
        let commands: Vec<Element<'_, Msg>> = keys::Cmd::ALL
            .into_iter()
            .map(|cmd| {
                let now = self
                    .prefs
                    .shortcuts
                    .get(cmd.id())
                    .map_or(cmd.default_chord(), String::as_str);
                let parts: Vec<String> = now.split('+').map(String::from).collect();
                let refused = self.refused.as_ref().filter(|(c, ..)| *c == cmd);
                let state = match (self.recording == Some(cmd), refused) {
                    (true, _) => State::Recording,
                    (false, Some(_)) => State::Conflict,
                    _ => State::Rest,
                };
                let field = shortcut_field(
                    parts,
                    state,
                    t!(pf_key_press),
                    t!(pf_key_none),
                    Msg::Record(cmd),
                );
                let custom = self.prefs.shortcuts.contains_key(cmd.id());
                let reset = button::button(
                    label(t!(c_reset)),
                    Kind::Elevated,
                    custom.then_some(Msg::ResetKey(cmd)),
                );
                let control = setting_row::field_with(field, [reset]);
                let desc: String = match refused {
                    Some((_, chord, keys::Taken::Fixed)) => {
                        crate::shared::app_language::fmt(S::pf_key_fixed, &[chord])
                    }
                    Some((_, chord, keys::Taken::Command(other))) => {
                        crate::shared::app_language::fmt(
                            S::pf_key_taken,
                            &[chord, &tr(other.name())],
                        )
                    }
                    None => String::new(),
                };
                setting_row::plain(title(tr(cmd.name())), desc, control)
            })
            .collect();
        let fixed: Vec<Element<'_, Msg>> = keys::FIXED_ROWS
            .iter()
            .map(|(what, keys)| {
                let caps = iced::widget::row(keys.iter().map(|key| {
                    let parts: Vec<String> = key.split('+').map(String::from).collect();
                    option_list::caps(&parts, false)
                }))
                .spacing(space::GAP)
                .wrap();
                setting_row::plain(title(tr(*what)), "", caps)
            })
            .collect();
        let reset_all = setting_row::plain(
            title(t!(pf_keys_reset_all)),
            t!(pf_keys_note),
            setting_row::action(
                t!(c_reset),
                Kind::Elevated,
                (!self.prefs.shortcuts.is_empty()).then_some(Msg::ResetKeys),
            ),
        );
        vec![
            (t!(pf_grp_commands), commands),
            (t!(pf_grp_fixed), fixed),
            ("", vec![reset_all]),
        ]
    }
}

/// `c`'s editor options back at the defaults (Line assist is a setting, not a tool style).
fn default_styles(c: &EditorConfig) -> EditorConfig {
    EditorConfig {
        line_assist: c.line_assist,
        ..Default::default()
    }
}

/// The theme the Studio window draws with: `studio.theme`'s, or `None` to follow
/// OpenCircle's own (the host `themes::set`s it around the Studio's view only).
pub fn theme_of(prefs: &StudioPrefs) -> Option<&'static themes::Theme> {
    THEMES.iter().copied().find(|t| t.id == prefs.theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::app_language::{self, Lang};

    fn dialog() -> Dialog {
        Dialog::from(&Config::default())
    }

    /// W91.1: a change in the dialog is a `Changed` the host persists; `apply` writes
    /// it (and the Save size) into the config, and nothing else.
    #[test]
    fn a_change_is_applied_to_the_config() {
        let mut d = dialog();
        let mut p = d.prefs().clone();
        p.theme = "obsidian".into();
        p.steps_side = StudioSide::Start;
        p.steps_on_start = false;
        p.start_screen = false;
        assert_eq!(d.update(Msg::Prefs(Box::new(p))), Out::Changed);
        assert_eq!(d.update(Msg::MaxEdge(1568)), Out::Changed);
        let mut c = Config::default();
        d.apply(&mut c);
        assert_eq!(c.studio.theme, "obsidian");
        assert_eq!(c.studio.steps_side, StudioSide::Start);
        assert!(!c.studio.steps_on_start && !c.studio.start_screen);
        assert_eq!(c.snip.max_edge, 1568);
        assert_eq!(c.snip.studio, Config::default().snip.studio);
    }

    /// W91.1: the six tabs, the Save folder's button (F11: it opens General), Close.
    #[test]
    fn tabs_folder_and_close() {
        let mut d = dialog();
        assert_eq!(d.tab(), Tab::General);
        for tab in Tab::ALL {
            assert_eq!(d.update(Msg::Tab(tab)), Out::None);
            assert_eq!(d.tab(), tab);
        }
        assert_eq!(d.update(Msg::OpenGeneral), Out::OpenGeneral);
        assert_eq!(d.update(Msg::Close), Out::Close);
    }

    /// W91.1: a refused file name is not written (the row says why).
    #[test]
    fn a_bad_file_name_is_not_saved() {
        let mut d = dialog();
        let mut p = d.prefs().clone();
        p.file_name = "a:b".into();
        d.update(Msg::Prefs(Box::new(p)));
        let mut c = Config::default();
        d.apply(&mut c);
        assert_eq!(c.studio.file_name, config::STUDIO_FILE_NAME);
    }

    /// W91.1: Reset tool styles clears the editor's remembered options once, keeps
    /// Line assist (a setting, not a style).
    #[test]
    fn reset_tool_styles_keeps_line_assist() {
        let mut c = Config::default();
        c.editor.colour = Some("#ff0000".into());
        c.editor.line_assist = false;
        let mut d = Dialog::from(&c);
        assert!(d.tool_styles_set);
        assert_eq!(d.update(Msg::ResetToolStyles), Out::Changed);
        assert!(!d.tool_styles_set);
        d.apply(&mut c);
        assert_eq!(c.editor.colour, None);
        assert!(!c.editor.line_assist);
        c.editor.colour = Some("#00ff00".into());
        d.apply(&mut c);
        assert!(c.editor.colour.is_some(), "the reset ran once");
    }

    /// W91.1: the Studio's theme: "follow" (the default) has none; an id is that theme.
    #[test]
    fn the_studio_theme_follows_or_is_its_own() {
        let mut p = StudioPrefs::default();
        assert!(theme_of(&p).is_none());
        p.theme = "daylight".into();
        assert_eq!(theme_of(&p).map(|t| t.id), Some("daylight"));
        // every id the config accepts is a built-in theme
        for id in config::DARK_THEMES.iter().chain(&config::LIGHT_THEMES) {
            p.theme = (*id).into();
            assert!(theme_of(&p).is_some(), "{id}");
        }
    }

    /// W91.1: the dialog, every tab, in English and Arabic (look at each after an update).
    #[test]
    fn the_dialog_matches_its_golden_images() {
        use crate::shared::kit::testing::{golden, shot};
        crate::shared::kit::text::load();
        let size = Size::new(1280.0, 800.0);
        golden::each_theme(|theme, failed| {
            for (lang, tag) in [(Lang::En, ""), (Lang::Ar, "-ar")] {
                app_language::set(lang);
                for tab in Tab::ALL {
                    let mut d = dialog();
                    d.update(Msg::Tab(tab));
                    let file = format!("studio-prefs-{tab:?}{tag}-{}", theme.id).to_lowercase();
                    let lost = shot::out_of_reach(d.view(size), size);
                    assert!(lost.is_empty(), "{file}: {lost:?}");
                    failed.extend(golden::check("screens", &file, size, &[1.0], || {
                        d.view(size)
                    }));
                }
            }
            app_language::set(Lang::En);
        });
    }

    /// W90d.2: in a big window (3440x1440) the card grows and the section list stays INSIDE it,
    /// at the start side, as tall as the card (EN and AR).
    #[test]
    fn the_dialog_in_a_wide_window_is_one_card() {
        use crate::shared::kit::testing::{golden, shot};
        crate::shared::kit::text::load();
        let size = Size::new(3440.0, 1440.0);
        let (w, h) = card_size(size);
        assert!(w > WIDTH && h > HEIGHT, "grows: {w} x {h}");
        assert_eq!(card_size(Size::new(1280.0, 800.0)), (WIDTH, HEIGHT));
        assert_eq!(card_size(Size::new(500.0, 400.0)), (476.0, 376.0));
        {
            // a narrow window: the card (and the list inside it) fits it
            let narrow = Size::new(774.0, 700.0);
            app_language::set(Lang::Ar);
            let d = dialog();
            crate::shared::kit::testing::golden::check(
                "screens",
                "studio-prefs-narrow-ar-sapphire",
                narrow,
                &[1.0],
                || d.view(narrow),
            );
            app_language::set(Lang::En);
        }
        golden::each_theme(|theme, failed| {
            for (lang, tag) in [(Lang::En, ""), (Lang::Ar, "-ar")] {
                app_language::set(lang);
                let file = format!("studio-prefs-wide{tag}-{}", theme.id);
                let d = dialog();
                let lost = shot::out_of_reach(d.view(size), size);
                assert!(lost.is_empty(), "{file}: {lost:?}");
                failed.extend(golden::check("screens", &file, size, &[1.0], || {
                    d.view(size)
                }));
            }
            app_language::set(Lang::En);
        });
    }

    /// W106.2: in a big window a row's label and its control stay together (the rows' column is
    /// at most CONTENT wide), and in Arabic the label is at the right, the control at the left.
    #[test]
    fn w106_a_label_and_its_control_stay_together_and_mirror() {
        use crate::shared::kit::testing::shot;
        crate::shared::kit::text::load();
        let size = Size::new(3440.0, 1440.0);
        let rect_of = |texts: &[(iced::Rectangle, String)], s: &str| {
            texts
                .iter()
                .find(|(_, t)| t == s)
                .map(|(r, _)| *r)
                .unwrap_or_else(|| panic!("{s:?} not drawn: {texts:?}"))
        };
        for (lang, rtl) in [(Lang::En, false), (Lang::Ar, true)] {
            app_language::set(lang);
            let d = dialog();
            let texts = shot::texts_at(d.view(size), size);
            let label = rect_of(&texts, tr(S::ss_open_in));
            let control = rect_of(&texts, tr(S::ss_quick));
            let span = label.x.max(control.x + control.width) - label.x.min(control.x);
            assert!(span <= CONTENT, "{lang:?}: label to control spans {span}");
            assert_eq!(
                label.x > control.x,
                rtl,
                "{lang:?}: the label {label:?} / control {control:?} sit at the wrong ends"
            );
        }
        app_language::set(Lang::En);
    }

    /// W91.1: apply writes only what the dialog changed; a value changed elsewhere
    /// while it was open (snip.max_edge, another [studio] key) is kept.
    #[test]
    fn apply_keeps_what_changed_elsewhere() {
        let mut d = dialog();
        let mut p = d.prefs().clone();
        p.theme = "obsidian".into();
        d.update(Msg::Prefs(Box::new(p)));
        // meanwhile Settings / another window changed other keys on the file
        let mut c = Config::default();
        c.snip.max_edge = 2048;
        c.studio.snap = false;
        c.studio.recent_files = 20;
        d.apply(&mut c);
        assert_eq!(c.studio.theme, "obsidian", "the dialog's change");
        assert_eq!(
            c.snip.max_edge, 2048,
            "not overwritten by the dialog's old value"
        );
        assert!(!c.studio.snap && c.studio.recent_files == 20);
        // the next change is measured against the last one written
        d.update(Msg::MaxEdge(1024));
        c.studio.snap = true;
        d.apply(&mut c);
        assert_eq!((c.snip.max_edge, c.studio.snap), (1024, true));
        assert_eq!(c.studio.theme, "obsidian");
    }
}

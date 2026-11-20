//! Studio preferences (Studio v3.1): a centred dialog inside the Studio with the tabs
//! General, Appearance, Saving and export, Copy and paste, Editing and Shortcuts. It is
//! THE one source of the Studio's settings rows (Settings > Studio is only a button that
//! opens it). Changes take effect and save by themselves; Esc / Close closes it.
//!
//! The host (the Studio window) owns a `Dialog` while it is open:
//! `Dialog::open(default_folder)`, show `view(window)` mapped to its own message, feed every `Msg` to
//! `update`, and act on the `Out` (`Changed`: persist with
//! `config::update(|c| dialog.apply(c))`; `Browse`: pick a folder, answer with
//! `set_folder`; `Close`: drop it). `theme_of` gives the Studio window's theme.
#![allow(dead_code)] // the Studio window wires the dialog in (W90), then this goes
use iced::widget::{column, container};
use iced::{Center, Element, Fill, Length, Size, widget::tooltip::Position};

use crate::features::snip::screens::studio::keys;
use crate::shared::app_language::{S, tr};
use crate::shared::config::{
    self, Config, EditorConfig, Format, STUDIO_THEME_FOLLOW, StudioBackdrop, StudioOpenIn,
    StudioPrefs, StudioSide, StudioWheel, StudioZoom,
};
use crate::shared::kit::button::{self, Kind, label};
use crate::shared::kit::dropdown::dropdown;
use crate::shared::kit::field::field;
use crate::shared::kit::scroll::scroll;
use crate::shared::kit::segmented::{boolean, inline, segmented};
use crate::shared::kit::setting_row::{self, title};
use crate::shared::kit::surface::{Material, surface};
use crate::shared::kit::text::{error, text};
use crate::shared::kit::themes::{self, THEMES};
use crate::shared::kit::tokens::{Role, radius, space};
use crate::shared::kit::tooltip::tip;
use crate::shared::kit::{divider, option_list};
use crate::t;

/// The dialog's size (logical px); it never exceeds the window less `MARGIN`.
const WIDTH: f32 = 760.0;
const HEIGHT: f32 = 600.0;
const MARGIN: f32 = 12.0;

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

    fn name(self) -> &'static str {
        match self {
            Tab::General => t!(nav_general),
            Tab::Appearance => t!(nav_appearance),
            Tab::Saving => t!(pf_tab_saving),
            Tab::CopyPaste => t!(pf_tab_copy),
            Tab::Editing => t!(pf_tab_editing),
            Tab::Shortcuts => t!(nav_shortcuts),
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
    BrowseFolder,
    ClearFolder,
    ResetToolStyles,
    Close,
}

/// What the host does next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    None,
    /// Something changed: persist with `Dialog::apply` and re-read the config.
    Changed,
    /// Show the folder picker; answer `Dialog::set_folder`.
    Browse,
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
    default_folder: Option<String>,
    themes: Vec<Choice>,
}

impl Dialog {
    /// Opens on General with the running config; `default_folder`: where Save starts when
    /// no folder is set (`circle::actions::save::default_folder`, which the host knows).
    pub fn open(default_folder: Option<std::path::PathBuf>) -> Self {
        let mut d = Self::from(&config::current());
        d.default_folder = default_folder.map(|p| p.to_string_lossy().into_owned());
        d
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
            default_folder: None,
            themes,
        }
    }

    pub fn prefs(&self) -> &StudioPrefs {
        &self.prefs
    }

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
            Msg::BrowseFolder => Out::Browse,
            Msg::ClearFolder => {
                self.prefs.folder.clear();
                Out::Changed
            }
            Msg::ResetToolStyles => {
                (self.reset_styles, self.tool_styles_set) = (true, false);
                Out::Changed
            }
            Msg::Close => Out::Close,
        }
    }

    /// The folder picker's answer (`None`: cancelled).
    pub fn set_folder(&mut self, dir: Option<std::path::PathBuf>) -> Out {
        match dir {
            Some(dir) => {
                self.prefs.folder = dir.to_string_lossy().into_owned();
                Out::Changed
            }
            None => Out::None,
        }
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

    pub fn view(&self, window: Size) -> Element<'_, Msg> {
        let head = crate::dir_row![
            text(Role::CARD_HEADING, t!(pf_title)).width(Fill),
            button::button(label(t!(st_close)), Kind::Elevated, Some(Msg::Close)),
        ]
        .align_y(Center);
        let tabs = Tab::ALL.map(|t| (t, t.name()));
        let bar = inline(&tabs, self.tab, self.tab, 1.0, Msg::Tab);
        let body =
            scroll(container(self.rows()).padding([0.0, space::PAGE[1]])).height(Length::Fill);
        let content = column![
            container(column![head, bar].spacing(space::GAP)).padding(space::PAGE),
            divider::divider(),
            body
        ];
        let w = (window.width - 2.0 * MARGIN).clamp(0.0, WIDTH);
        let h = (window.height - 2.0 * MARGIN).clamp(0.0, HEIGHT);
        surface(Material::Widget, radius::CARD, content)
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .into()
    }

    /// The rows of the open tab.
    fn rows(&self) -> Element<'_, Msg> {
        let p = &self.prefs;
        let edit = move |change: &dyn Fn(&mut StudioPrefs)| {
            let mut next = p.clone();
            change(&mut next);
            Msg::Prefs(Box::new(next))
        };
        let row = |name, desc, control| setting_row::row(title(name), desc, control);
        let switch = move |name, desc, on: bool, set: fn(&mut StudioPrefs, bool)| {
            row(
                name,
                desc,
                boolean([t!(c_off), t!(c_on)], on, on, 1.0, move |v| {
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
        let rows: Vec<Element<'_, Msg>> = match self.tab {
            Tab::General => vec![
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
                    t!(ss_ask_unsaved),
                    t!(ss_ask_unsaved_d),
                    p.ask_unsaved,
                    |s, v| s.ask_unsaved = v,
                ),
                switch(t!(ss_keep), t!(ss_keep_d), p.keep_unsaved, |s, v| {
                    s.keep_unsaved = v
                }),
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
                switch(
                    t!(pf_start_screen),
                    t!(pf_start_screen_d),
                    p.start_screen,
                    |s, v| s.start_screen = v,
                ),
            ],
            Tab::Appearance => {
                let chosen = self.themes.iter().find(|c| c.id == p.theme).cloned();
                vec![
                    row(
                        t!(pf_theme),
                        t!(pf_theme_d),
                        dropdown(&self.themes, chosen, t!(pf_theme_follow), move |c| {
                            edit(&|s| s.theme = c.id.to_string())
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
                ]
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
                    row(t!(ss_folder), t!(ss_folder_d), self.folder()),
                    row(t!(ss_name), t!(ss_name_d), name.spacing(space::GAP).into()),
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
                    switch(t!(ss_ask_where), t!(ss_ask_where_d), p.ask_where, |s, v| {
                        s.ask_where = v
                    }),
                    switch(t!(ss_copy_path), t!(ss_copy_path_d), p.copy_path, |s, v| {
                        s.copy_path = v
                    }),
                ]
            }
            Tab::CopyPaste => vec![
                switch(t!(ss_ctrlc), t!(ss_ctrlc_d), p.ctrl_c_picture, |s, v| {
                    s.ctrl_c_picture = v
                }),
                switch(
                    t!(ss_paste_obj),
                    t!(ss_paste_obj_d),
                    p.paste_movable,
                    |s, v| s.paste_movable = v,
                ),
                switch(t!(ss_grow), t!(ss_grow_d), p.grow_canvas, |s, v| {
                    s.grow_canvas = v
                }),
                format_row(t!(ss_drag), t!(ss_drag_d), p.drag_type, |s, f| {
                    s.drag_type = f
                }),
            ],
            Tab::Editing => {
                let (desc, press) = match self.tool_styles_set {
                    true => (t!(ss_tool_styles_d), Some(Msg::ResetToolStyles)),
                    false => (t!(ss_tool_styles_done), None),
                };
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
                ]
            }
            Tab::Shortcuts => shortcut_rows(),
        };
        column(rows).into()
    }

    /// The default folder: the field (Browse… too), Browse… and Reset.
    fn folder(&self) -> Element<'_, Msg> {
        let folder = self.prefs.folder.as_str();
        let shown = match folder.is_empty() {
            true => (self.default_folder.clone()).unwrap_or_else(|| t!(g_folder_system).into()),
            false => folder.to_string(),
        };
        let value =
            container(field(&shown, None::<fn(String) -> Msg>, Msg::BrowseFolder)).width(Fill);
        let browse = button::button(label(t!(c_browse)), Kind::Elevated, Some(Msg::BrowseFolder));
        let reset = button::button(
            label(t!(c_reset)),
            Kind::Elevated,
            (!folder.is_empty()).then_some(Msg::ClearFolder),
        );
        setting_row::field_with(
            tip(value, t!(g_folder), Position::Top),
            [
                tip(browse, t!(g_folder_tip_browse), Position::Bottom),
                tip(reset, t!(g_folder_tip_reset), Position::Bottom),
            ],
        )
        .into()
    }
}

/// `c`'s editor options back at the defaults (Line assist is a setting, not a tool style).
fn default_styles(c: &EditorConfig) -> EditorConfig {
    EditorConfig {
        line_assist: c.line_assist,
        ..Default::default()
    }
}

/// The Studio's keys as a reference (the keys themselves are `keys`); the Snip key
/// is the user's own, in Settings > Shortcuts.
fn shortcut_rows<'a>() -> Vec<Element<'a, Msg>> {
    use keys as k;
    let list: [(S, &[&str]); 11] = [
        (S::st_sc_file, &[k::OPEN, k::SAVE, k::SAVE_AS]),
        (S::st_sc_copy, &[k::COPY, k::CUT, k::PASTE]),
        (S::st_sc_whole, &[k::COPY_PICTURE]),
        (S::st_sc_dup, &[k::DUPLICATE, k::DELETE, k::SELECT_ALL]),
        (S::st_sc_undo, &[k::UNDO, k::REDO]),
        (S::st_sc_nudge, &[k::NUDGE, k::NUDGE_FAR]),
        (
            S::st_sc_order,
            &[k::FORWARD, k::BACKWARD, k::FRONT, k::BACK],
        ),
        (S::st_sc_steps, &[k::ADD_STEPS]),
        (S::st_sc_panels, &[k::LEFT_PANEL]),
        (S::st_sc_zoom, &[k::ZOOM_IN, k::ZOOM_OUT, k::FIT, k::ACTUAL]),
        (S::st_sc_pan, &[k::PAN]),
    ];
    let mut rows: Vec<Element<'a, Msg>> = list
        .into_iter()
        .map(|(what, keys)| {
            // one cap set per key name, wrapping when there are many
            let caps = iced::widget::row(keys.iter().map(|key| {
                let parts: Vec<String> = key.split('+').map(String::from).collect();
                option_list::caps(&parts, false)
            }))
            .spacing(space::GAP)
            .wrap();
            setting_row::row(title(tr(what)), "", caps)
        })
        .collect();
    rows.push(
        container(crate::shared::kit::text::sub(
            Role::ROW_DESC,
            t!(pf_keys_note),
        ))
        .padding([space::GAP, 0.0])
        .into(),
    );
    rows
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
        p.steps_side = StudioSide::End;
        p.steps_on_start = false;
        p.start_screen = false;
        assert_eq!(d.update(Msg::Prefs(Box::new(p))), Out::Changed);
        assert_eq!(d.update(Msg::MaxEdge(1568)), Out::Changed);
        let mut c = Config::default();
        d.apply(&mut c);
        assert_eq!(c.studio.theme, "obsidian");
        assert_eq!(c.studio.steps_side, StudioSide::End);
        assert!(!c.studio.steps_on_start && !c.studio.start_screen);
        assert_eq!(c.snip.max_edge, 1568);
        assert_eq!(c.snip.studio, Config::default().snip.studio);
    }

    /// W91.1: the six tabs, the folder picker's round trip, Close.
    #[test]
    fn tabs_folder_and_close() {
        let mut d = dialog();
        assert_eq!(d.tab(), Tab::General);
        for tab in Tab::ALL {
            assert_eq!(d.update(Msg::Tab(tab)), Out::None);
            assert_eq!(d.tab(), tab);
        }
        assert_eq!(d.update(Msg::BrowseFolder), Out::Browse);
        let dir = std::env::temp_dir();
        assert_eq!(d.set_folder(Some(dir.clone())), Out::Changed);
        assert_eq!(d.prefs().folder, dir.to_string_lossy());
        assert_eq!(
            d.set_folder(None),
            Out::None,
            "a cancelled picker changes nothing"
        );
        assert_eq!(d.update(Msg::ClearFolder), Out::Changed);
        assert_eq!(d.prefs().folder, "");
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

//! Setup › I/O: the I/O Setup window, laid out like Pro Tools': Input, Output and Bus tabs. The
//! Input tab lists the main paths with their sub-paths (active, name, format) beside the channel
//! grid of the input device, where a click under a hardware channel feeds the path from that
//! channel on, and the buttons below make, remove and reset paths. Every change is a `setup.io`
//! command, so it is one undo step and the CLI, control channel and MCP can do the same.

use crate::SoundApp;
use crate::theme::Tokens;
use egui::{Align2, CornerRadius, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use serde_json::{Value, json};
use soundcraft_model::{ChannelFormat, InputPath, MAX_INPUT_CHANNEL};
use std::collections::BTreeSet;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Tab {
    #[default]
    Input,
    Output,
    Bus,
}

/// What the window keeps between frames (none of it is session state).
#[derive(Clone, Default)]
struct State {
    tab: Tab,
    /// The selected path's name (New Sub-Path and Delete Path act on it).
    selected: Option<String>,
    /// Main paths whose sub-paths are folded away.
    collapsed: BTreeSet<String>,
    /// The name being edited: (path, text).
    renaming: Option<(String, String)>,
    new_path: Option<NewPath>,
    default_format: Option<ChannelFormat>,
}

/// The New Paths dialog's row and options.
#[derive(Clone)]
struct NewPath {
    count: u32,
    format: ChannelFormat,
    name: String,
    assign: bool,
    sub_paths: bool,
}

/// Cell size in the grid.
const CELL: f32 = 22.0;
const ROW: f32 = 18.0;

pub fn io_setup(app: &mut SoundApp, ctx: &egui::Context) {
    let mut open = app.ui.show_io_setup;
    if !open {
        return;
    }
    let id = egui::Id::new("io_setup_state");
    let mut st: State = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    egui::Window::new("I/O Setup").open(&mut open).default_size(vec2(960.0, 560.0)).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (tab, label) in [(Tab::Input, "Input"), (Tab::Output, "Output"), (Tab::Bus, "Bus")] {
                if ui.selectable_label(st.tab == tab, label).clicked() {
                    st.tab = tab;
                }
            }
        });
        ui.separator();
        match st.tab {
            Tab::Input => input_tab(app, ui, &mut st),
            Tab::Output => output_tab(app, ui),
            Tab::Bus => bus_tab(app, ui),
        }
    });
    if st.new_path.is_some() {
        new_paths_dialog(app, ctx, &mut st);
    }
    ctx.data_mut(|d| d.insert_temp(id, st));
    app.ui.show_io_setup = open;
}

/// The input device the grid shows and how many inputs it has: the open one, else the one the
/// Playback Engine chooses (enumerated once per window), else nothing known. The count is every
/// channel the device offers, not just those the open stream carries.
fn input_device(app: &SoundApp, ctx: &egui::Context) -> (String, usize) {
    let systems = crate::windows::audio_systems(ctx, false);
    let sys = crate::windows::chosen_system(&systems, &app.ui.audio);
    let open = app.recorder.as_ref().map(|r| (r.device_name.clone(), r.channels));
    let known = sys.and_then(|s| match &open {
        Some((name, _)) => {
            s.inputs.iter().find(|d| d.name == *name).or_else(|| crate::windows::chosen_device(&s.inputs, app.ui.audio.input.as_deref()))
        }
        None => crate::windows::chosen_device(&s.inputs, app.ui.audio.input.as_deref()),
    });
    match (open, known) {
        (Some((name, ch)), Some(d)) => (name, ch.max(usize::from(d.max_channels))),
        (Some((name, ch)), None) => (name, ch),
        (None, Some(d)) => (d.name.clone(), usize::from(d.max_channels)),
        (None, None) => ("Input device".into(), 0),
    }
}

/// Columns the grid shows: the device's channels, at least eight, and every channel a path uses.
pub(crate) fn grid_columns(device_channels: usize, paths: &[InputPath]) -> usize {
    let used = paths
        .iter()
        .flat_map(|m| std::iter::once(m).chain(m.sub_paths.iter()))
        .flat_map(|p| p.channels.iter().flatten())
        .map(|c| usize::from(*c).saturating_add(1))
        .max()
        .unwrap_or(0);
    device_channels.max(used).max(8).min(usize::from(MAX_INPUT_CHANNEL).saturating_add(1))
}

/// What a click in a path's row does: on a cell the path reads, that channel is cleared; on any
/// other cell the path reads from that hardware channel on, its channels filling to the right
/// ("clicking under channel 1 fills both channel 1 and 2").
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CellClick {
    Clear(usize),
    MapFrom(u16),
}

pub(crate) fn cell_click(path: &InputPath, hw: u16) -> CellClick {
    match path.channel_on(hw) {
        Some(k) => CellClick::Clear(k),
        None => CellClick::MapFrom(hw),
    }
}

/// The label a path's row shows under hardware channel `hw`: the channel's speaker (L, R, C,
/// LFE…) when the path reads it, and M for a mono path, as Pro Tools' grid shows them.
pub(crate) fn cell_label(path: &InputPath, hw: u16) -> Option<String> {
    path.channel_on(hw).map(|k| if path.format == ChannelFormat::Mono { "M".to_string() } else { path.format.channel_label(k) })
}

fn input_tab(app: &mut SoundApp, ui: &mut Ui, st: &mut State) {
    let t = Tokens::current();
    let (device, device_channels) = input_device(app, ui.ctx());
    let paths: Vec<InputPath> = app.engine.session().inputs.clone();
    let columns = grid_columns(device_channels, &paths);
    let mut pending: Vec<Value> = Vec::new();
    let mut select: Option<Option<String>> = None;
    let button_h = 120.0;
    egui::ScrollArea::both().id_salt("io-in-scroll").max_height((ui.available_height() - button_h).max(120.0)).show(ui, |ui| {
        egui::Grid::new("io-in-grid").num_columns(4 + columns).spacing(vec2(2.0, 1.0)).min_col_width(0.0).show(ui, |ui| {
            // The device's name over its channels, then the column headings.
            ui.label("");
            ui.label("");
            ui.label("");
            ui.label("");
            for c in 0..columns {
                // The device's name runs over its channel columns; painted, so it widens none of them.
                let (r, _) = ui.allocate_exact_size(vec2(CELL, ROW), Sense::hover());
                if c == 0 {
                    ui.painter().text(r.left_center(), Align2::LEFT_CENTER, &device, egui::FontId::proportional(11.0), t.text);
                }
            }
            ui.end_row();
            ui.label("");
            ui.label("");
            ui.label(RichText::new("Name").strong().size(11.0));
            ui.label(RichText::new("Format").strong().size(11.0));
            for c in 0..columns {
                let (r, _) = ui.allocate_exact_size(vec2(CELL, ROW), Sense::hover());
                ui.painter().text(r.center(), Align2::CENTER_CENTER, format!("{}", c + 1), egui::FontId::proportional(10.0), t.text_dim);
            }
            ui.end_row();
            for main in &paths {
                path_row(ui, st, main, None, columns, &mut pending, &mut select);
                if !st.collapsed.contains(&main.name) {
                    for sub in &main.sub_paths {
                        path_row(ui, st, sub, Some(main), columns, &mut pending, &mut select);
                    }
                }
            }
        });
    });
    if let Some(sel) = select {
        st.selected = sel;
    }
    ui.add_space(6.0);
    let selected_main = st.selected.as_deref().and_then(|n| app.engine.session().find_input_path(n)).is_some_and(|(_, sub)| sub.is_none());
    let selected_any = st.selected.as_deref().and_then(|n| app.engine.session().find_input_path(n)).is_some();
    ui.horizontal(|ui| {
        if ui.button("New Path...").clicked() {
            st.new_path = Some(NewPath {
                count: 1,
                format: st.default_format.unwrap_or(ChannelFormat::Stereo),
                name: String::new(),
                assign: true,
                sub_paths: true,
            });
        }
        if ui.add_enabled(selected_main, egui::Button::new("New Sub-Path")).on_hover_text("A path within the selected main path").clicked()
            && let Some(name) = &st.selected
        {
            pending.push(json!({"action": "create_input_subpath", "path": name, "format": "Mono"}));
        }
        if ui.add_enabled(selected_any, egui::Button::new("Delete Path")).clicked()
            && let Some(name) = st.selected.take()
        {
            pending.push(json!({"action": "delete_input", "name": name}));
        }
        let every = if device_channels == 0 { 8 } else { device_channels };
        if ui
            .button("Default")
            .on_hover_text(format!("Every input of the device ({every}): a stereo path per channel pair, with mono sub-paths"))
            .clicked()
        {
            pending.push(json!({"action": "default_inputs", "channels": every}));
            st.selected = None;
        }
        ui.add_space(12.0);
        ui.label("Default Format:");
        let shown = st.default_format.unwrap_or(ChannelFormat::Stereo);
        egui::ComboBox::from_id_salt("io-default-format").selected_text(shown.label()).show_ui(ui, |ui| {
            for f in formats_up_to(16) {
                if ui.selectable_label(f == shown, f.label()).clicked() {
                    st.default_format = Some(f);
                }
            }
        });
    });
    ui.label(
        RichText::new("Click a cell to feed the path from that channel on; click a lit cell to clear it. Double-click a name to rename it. A sub-path reads its main path's channels only. Inactive paths are shown in italics and feed nothing.")
            .small()
            .color(t.text_dim),
    );
    for p in pending {
        if let Err(e) = app.run("setup.io", p) {
            app.ui.status = e;
        }
    }
}

/// One row of the grid: the path's controls, then a cell per hardware channel.
fn path_row(
    ui: &mut Ui,
    st: &mut State,
    path: &InputPath,
    main: Option<&InputPath>,
    columns: usize,
    pending: &mut Vec<Value>,
    select: &mut Option<Option<String>>,
) {
    let t = Tokens::current();
    let is_sub = main.is_some();
    let active = path.active && main.is_none_or(|m| m.active);
    let selected = st.selected.as_deref() == Some(path.name.as_str());
    // Active.
    let mut on = path.active;
    if crate::widgets::checkbox(ui, &mut on, "").on_hover_text(if is_sub { "Active (with its main path)" } else { "Active" }).changed() {
        pending.push(json!({"action": "set_input_active", "name": path.name, "active": on}));
    }
    // Fold.
    if is_sub || path.sub_paths.is_empty() {
        ui.label("");
    } else {
        let folded = st.collapsed.contains(&path.name);
        if ui.add(egui::Button::new(if folded { "▶" } else { "▼" }).frame(false).small()).clicked() {
            if folded {
                st.collapsed.remove(&path.name);
            } else {
                st.collapsed.insert(path.name.clone());
            }
        }
    }
    // Name: select on click, rename on double-click.
    match &mut st.renaming {
        Some((name, text)) if name == &path.name => {
            let r = ui.add(egui::TextEdit::singleline(text).desired_width(if is_sub { 160.0 } else { 180.0 }));
            r.request_focus();
            let done = r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter));
            let cancel = ui.input(|i| i.key_pressed(egui::Key::Escape));
            if cancel {
                st.renaming = None;
            } else if done {
                let to = text.trim().to_string();
                if !to.is_empty() && to != path.name {
                    pending.push(json!({"action": "rename_input", "name": path.name, "new_name": to}));
                    *select = Some(Some(to));
                }
                st.renaming = None;
            }
        }
        _ => {
            let mut label = RichText::new(if is_sub { format!("    {}", path.name) } else { path.name.clone() }).size(12.0);
            if !active {
                label = label.italics().color(t.text_dim);
            }
            let r = ui.selectable_label(selected, label).on_hover_text("Click to select; double-click to rename");
            if r.double_clicked() {
                st.renaming = Some((path.name.clone(), path.name.clone()));
            } else if r.clicked() {
                *select = Some(Some(path.name.clone()));
            }
        }
    }
    // Format: a sub-path fits within its main path.
    let max = main.map_or(16, |m| m.width());
    egui::ComboBox::from_id_salt(("io-fmt", &path.name)).width(70.0).selected_text(RichText::new(path.format.label()).size(11.0)).show_ui(ui, |ui| {
        for f in formats_up_to(max) {
            if ui.selectable_label(f == path.format, f.label()).clicked() && f != path.format {
                pending.push(json!({"action": "set_input_format", "name": path.name, "format": f.label()}));
            }
        }
    });
    // The cells.
    for c in 0..columns {
        let hw = u16::try_from(c).unwrap_or(u16::MAX);
        let enabled = main.is_none_or(|m| m.channels.contains(&Some(hw)));
        let label = cell_label(path, hw);
        let (r, resp) = ui.allocate_exact_size(vec2(CELL, ROW), if enabled { Sense::click() } else { Sense::hover() });
        let p = ui.painter();
        let fill = if !enabled {
            t.panel_bg
        } else if label.is_some() {
            if active { t.button_hi } else { t.button }
        } else if resp.hovered() {
            t.panel_bg2
        } else {
            t.slot_bg
        };
        p.rect(r.shrink(0.5), CornerRadius::same(2), fill, Stroke::new(1.0, if selected { t.accent } else { t.border }), StrokeKind::Inside);
        if let Some(l) = &label {
            p.text(r.center(), Align2::CENTER_CENTER, l, egui::FontId::proportional(10.0), if active { t.text } else { t.text_dim });
        }
        if resp.clicked() {
            *select = Some(Some(path.name.clone()));
            match cell_click(path, hw) {
                CellClick::Clear(k) => pending.push(json!({"action": "set_input_channel", "name": path.name, "channel": k, "hardware": null})),
                CellClick::MapFrom(hw) => pending.push(json!({"action": "map_input", "name": path.name, "hardware": hw})),
            }
        }
    }
    ui.end_row();
}

/// The formats a path may have, up to `max` channels (Ambisonics has no speakers to map).
fn formats_up_to(max: usize) -> impl Iterator<Item = ChannelFormat> {
    ChannelFormat::ALL.into_iter().filter(move |f| !f.is_ambisonic() && f.channels() <= max)
}

/// New Paths: how many, their format and name, and the two options Pro Tools' dialog has.
fn new_paths_dialog(app: &mut SoundApp, ctx: &egui::Context, st: &mut State) {
    let Some(draft) = &mut st.new_path else { return };
    let mut close = false;
    let mut create = false;
    egui::Window::new("New Paths").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0)).show(ctx, |ui| {
        egui::Grid::new("io-new").num_columns(3).spacing(vec2(10.0, 6.0)).show(ui, |ui| {
            ui.label(RichText::new("Number of new paths").small());
            ui.label(RichText::new("Path format").small());
            ui.label(RichText::new("Path name").small());
            ui.end_row();
            ui.add(egui::DragValue::new(&mut draft.count).range(1..=64));
            egui::ComboBox::from_id_salt("io-new-format").selected_text(draft.format.label()).show_ui(ui, |ui| {
                for f in formats_up_to(16) {
                    ui.selectable_value(&mut draft.format, f, f.label());
                }
            });
            let r = ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(180.0).hint_text("Name"));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                create = true;
            }
            ui.end_row();
        });
        crate::widgets::checkbox(ui, &mut draft.assign, "Add default channel assignments");
        crate::widgets::checkbox(ui, &mut draft.sub_paths, "Auto-create sub-paths");
        ui.label(RichText::new("Several paths are numbered: Name 1, Name 2, …").small().weak());
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                close = true;
            }
            if ui.add_enabled(!draft.name.trim().is_empty(), egui::Button::new("Create")).clicked() {
                create = true;
            }
        });
    });
    if create && !draft.name.trim().is_empty() {
        let device_channels = input_device(app, ctx).1;
        let mut p = json!({
            "action": "create_input",
            "name": draft.name.trim(),
            "format": draft.format.label(),
            "count": draft.count,
            "assign": draft.assign,
            "sub_paths": draft.sub_paths,
        });
        if device_channels > 0 {
            p["device_channels"] = json!(device_channels);
        }
        match app.run("setup.io", p) {
            Ok(_) => close = true,
            Err(e) => app.ui.status = e,
        }
    }
    if close {
        st.new_path = None;
    }
}

/// Output: the main output's format and the output paths.
fn output_tab(app: &mut SoundApp, ui: &mut Ui) {
    let main = app.engine.session().main_format();
    ui.horizontal(|ui| {
        ui.label("Main output");
        egui::ComboBox::from_id_salt("main_output_format").selected_text(main.label()).show_ui(ui, |ui| {
            for f in ChannelFormat::ALL.into_iter().filter(|f| (2..=16).contains(&f.channels())) {
                if ui.selectable_label(f == main, f.label()).clicked() && f != main {
                    let _ = app.run("setup.main_format", json!({"format": f.label()}));
                }
            }
        });
        ui.label(RichText::new(format!("{} ch", main.channels())).small());
    });
    for o in app.engine.session().outputs.clone() {
        ui.label(format!("{} — {} from channel {}", o.name, o.format.label(), o.first_channel + 1));
    }
}

/// Bus: the internal mix busses.
fn bus_tab(app: &mut SoundApp, ui: &mut Ui) {
    let busses: Vec<(String, String)> = app.engine.session().busses.iter().map(|b| (b.name.clone(), b.format.label().to_string())).collect();
    egui::Grid::new("busses").num_columns(3).striped(true).show(ui, |ui| {
        for (name, fmt) in busses {
            let key = egui::Id::new(("bus_name", name.clone()));
            let mut edit: String = ui.ctx().memory(|m| m.data.get_temp(key)).unwrap_or_else(|| name.clone());
            let r = ui.text_edit_singleline(&mut edit);
            ui.ctx().memory_mut(|m| m.data.insert_temp(key, edit.clone()));
            if r.lost_focus() && edit != name {
                let _ = app.run("setup.io", json!({"action": "rename_bus", "name": name, "new_name": edit}));
            }
            ui.label(fmt);
            if ui.small_button("Delete").clicked() {
                let _ = app.run("setup.io", json!({"action": "delete_bus", "name": name}));
            }
            ui.end_row();
        }
    });
    if ui.button("New Bus").clicked() {
        let n = app.engine.session().busses.len() + 1;
        let _ = app.run("mix.new_bus", json!({"name": format!("Bus {n}")}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundcraft_model::Session;

    #[test]
    fn the_grid_is_as_wide_as_the_device_or_the_paths_but_at_least_eight() {
        let paths = Session::default_input_paths(4);
        assert_eq!(grid_columns(2, &paths), 8);
        assert_eq!(grid_columns(16, &paths), 16);
        let mut wide = paths.clone();
        wide[0].channels[1] = Some(31);
        assert_eq!(grid_columns(2, &wide), 32, "a path on channel 32 needs the column");
        wide[1].sub_paths[0].channels[0] = Some(40);
        assert_eq!(grid_columns(2, &wide), 41, "sub-paths count too");
        wide[0].channels[0] = Some(u16::MAX);
        assert_eq!(grid_columns(2, &wide), usize::from(MAX_INPUT_CHANNEL) + 1, "capped");
    }

    #[test]
    fn a_click_clears_a_lit_cell_or_feeds_the_path_from_the_clicked_channel() {
        let p = InputPath::assigned("In 3-4", ChannelFormat::Stereo, 2);
        assert_eq!(cell_click(&p, 2), CellClick::Clear(0));
        assert_eq!(cell_click(&p, 3), CellClick::Clear(1));
        assert_eq!(cell_click(&p, 5), CellClick::MapFrom(5));
        assert_eq!(cell_label(&p, 2).as_deref(), Some("L"));
        assert_eq!(cell_label(&p, 3).as_deref(), Some("R"));
        assert_eq!(cell_label(&p, 4), None);
        let five = InputPath::assigned("Surr", ChannelFormat::Surround51, 0);
        assert_eq!(cell_label(&five, 3).as_deref(), Some("LFE"));
        assert_eq!(cell_label(&five, 5).as_deref(), Some("Rs"));
        assert_eq!(cell_label(&InputPath::assigned("In 1", ChannelFormat::Mono, 0), 0).as_deref(), Some("M"));
    }

    #[test]
    fn formats_offered_fit_the_width() {
        let mono: Vec<ChannelFormat> = formats_up_to(1).collect();
        assert_eq!(mono, [ChannelFormat::Mono]);
        assert!(formats_up_to(2).any(|f| f == ChannelFormat::Stereo));
        assert!(formats_up_to(16).all(|f| !f.is_ambisonic() && f.channels() <= 16));
    }
}

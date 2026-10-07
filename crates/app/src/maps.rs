//! Map browser (M9): `M` opens a list of the test level (the default) and every `.bsp` found on this
//! machine. Type to filter, Up/Down/PageUp/PageDown to move, Enter or a click to load, `M` or Esc to
//! close.
//!
//! Maps are found in `CSMOVE_MAP_DIRS` (a path list), `./maps`, and the CS:GO install of every Steam
//! library (`csgo/maps` and its `workshop` folders). Nothing is copied; maps are read where they are.

use std::path::{Path, PathBuf};

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::ButtonState;
use bevy::prelude::*;
use bevy::text::{FontSize, FontSource};
use bevy::window::{CursorGrabMode, CursorOptions};

use crate::input::ViewAngles;
use crate::level::{self, CurrentLevel, Geometry, LevelEntity};
use crate::sim::Sim;

/// Rows shown at once.
const ROWS: usize = 18;

#[derive(Clone, Debug)]
pub struct MapEntry {
    pub label: String,
    /// `None` is the test level.
    pub path: Option<PathBuf>,
}

#[derive(Resource)]
pub struct MapMenu {
    pub open: bool,
    pub entries: Vec<MapEntry>,
    pub filter: String,
    /// Index into the filtered list.
    pub selected: usize,
    pub status: String,
    request: Option<Option<PathBuf>>,
    dirty: bool,
}

impl MapMenu {
    pub fn new(current: Option<&Path>) -> Self {
        let mut entries = discover();
        if let Some(p) = current {
            if !entries.iter().any(|e| e.path.as_deref() == Some(p)) {
                entries.push(MapEntry { label: label_for(p), path: Some(p.to_path_buf()) });
            }
        }
        Self { open: false, entries, filter: String::new(), selected: 0, status: String::new(), request: None, dirty: true }
    }

    fn filtered(&self) -> Vec<usize> {
        let f = self.filter.to_lowercase();
        (0..self.entries.len()).filter(|&i| f.is_empty() || self.entries[i].label.to_lowercase().contains(&f)).collect()
    }
}

/// Geometry for the level chosen on the command line, spawned once at startup.
#[derive(Resource)]
pub struct StartGeometry(pub Option<Geometry>);

fn label_for(p: &Path) -> String {
    let stem = p.file_stem().map_or_else(|| p.display().to_string(), |s| s.to_string_lossy().into_owned());
    if p.components().any(|c| c.as_os_str().eq_ignore_ascii_case("workshop")) {
        format!("{stem}  (workshop)")
    } else {
        stem
    }
}

/// Steam library roots from `libraryfolders.vdf`.
fn steam_libraries() -> Vec<PathBuf> {
    let mut vdfs = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Some(p) = std::env::var_os(var) {
            vdfs.push(PathBuf::from(p).join("Steam/steamapps/libraryfolders.vdf"));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        vdfs.push(home.join(".steam/steam/steamapps/libraryfolders.vdf"));
        vdfs.push(home.join(".local/share/Steam/steamapps/libraryfolders.vdf"));
    }
    let mut out = Vec::new();
    for vdf in vdfs {
        let Ok(text) = std::fs::read_to_string(&vdf) else { continue };
        for line in text.lines() {
            let tokens: Vec<&str> = line.split('"').collect();
            // `"path"		"D:\\SteamLibrary"` splits into ["", "path", "\t\t", "D:\\\\SteamLibrary", ""].
            if tokens.len() >= 4 && tokens[1] == "path" {
                out.push(PathBuf::from(tokens[3].replace("\\\\", "\\")));
            }
        }
    }
    out
}

/// Every `.bsp` in the search folders and their immediate subfolders, test level first.
pub fn discover() -> Vec<MapEntry> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(v) = std::env::var_os("CSMOVE_MAP_DIRS") {
        dirs.extend(std::env::split_paths(&v));
    }
    dirs.push(PathBuf::from("maps"));
    for lib in steam_libraries() {
        let maps = lib.join("steamapps/common/Counter-Strike Global Offensive/csgo/maps");
        dirs.push(maps.clone());
        if let Ok(rd) = std::fs::read_dir(maps.join("workshop")) {
            dirs.extend(rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for d in &dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            let is_bsp = p.extension().is_some_and(|x| x.eq_ignore_ascii_case("bsp"));
            if is_bsp && p.is_file() {
                let key = std::fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
                if !found.iter().any(|q| std::fs::canonicalize(q).unwrap_or_else(|_| q.clone()) == key) {
                    found.push(p);
                }
            }
        }
    }
    let mut entries: Vec<MapEntry> = found.into_iter().map(|p| MapEntry { label: label_for(&p), path: Some(p) }).collect();
    entries.sort_by_key(|e| e.label.to_lowercase());
    entries.insert(0, MapEntry { label: "Test level (default)".into(), path: None });
    entries
}

#[derive(Component)]
pub struct MenuPanel;
#[derive(Component)]
pub struct MenuHeader;
#[derive(Component)]
pub struct MenuList;
#[derive(Component)]
pub struct MenuRow(usize);

fn mono(size: f32) -> TextFont {
    TextFont::from(FontSource::Monospace).with_font_size(FontSize::Px(size))
}

pub fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Percent(25.0),
                top: Val::Percent(12.0),
                width: Val::Percent(50.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(10.0)),
                row_gap: Val::Px(2.0),
                ..Default::default()
            },
            BackgroundColor(Color::srgba(0.05, 0.06, 0.08, 0.92)),
            Visibility::Hidden,
            GlobalZIndex(10),
            MenuPanel,
        ))
        .with_children(|p| {
            p.spawn((Text::new(""), mono(16.0), TextColor(Color::srgb(1.0, 0.95, 0.6)), MenuHeader));
            p.spawn((Node { flex_direction: FlexDirection::Column, ..Default::default() }, MenuList));
        });
}

/// `M` toggles the menu; while it is open it owns the keyboard (movement input stops).
pub fn menu_input(
    keys: Res<ButtonInput<KeyCode>>,
    mut typed: MessageReader<KeyboardInput>,
    mut menu: ResMut<MapMenu>,
    mut cursor: Single<&mut CursorOptions>,
) {
    if !menu.open {
        typed.clear();
        if keys.just_pressed(KeyCode::KeyM) {
            menu.open = true;
            menu.dirty = true;
            cursor.visible = true;
            cursor.grab_mode = CursorGrabMode::None;
        }
        return;
    }
    if keys.just_pressed(KeyCode::Escape) || keys.just_pressed(KeyCode::KeyM) && menu.filter.is_empty() {
        typed.clear();
        menu.open = false;
        menu.dirty = true;
        return;
    }
    let count = menu.filtered().len();
    let mut sel = menu.selected as isize;
    let page = ROWS as isize;
    for (key, step) in [(KeyCode::ArrowDown, 1), (KeyCode::ArrowUp, -1), (KeyCode::PageDown, page), (KeyCode::PageUp, -page)] {
        if keys.just_pressed(key) {
            sel += step;
        }
    }
    if keys.just_pressed(KeyCode::Home) {
        sel = 0;
    }
    if keys.just_pressed(KeyCode::End) {
        sel = count as isize - 1;
    }
    let sel = sel.clamp(0, (count as isize - 1).max(0)) as usize;
    if sel != menu.selected {
        menu.selected = sel;
        menu.dirty = true;
    }
    for ev in typed.read() {
        if ev.state != ButtonState::Pressed {
            continue;
        }
        match &ev.logical_key {
            Key::Backspace => {
                menu.filter.pop();
            }
            Key::Character(s) if s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') => {
                // `M` closes the menu only while the filter is empty, so it can still be typed after.
                if !(menu.filter.is_empty() && s.eq_ignore_ascii_case("m")) {
                    menu.filter.push_str(&s.to_lowercase());
                }
            }
            _ => continue,
        }
        menu.selected = 0;
        menu.dirty = true;
    }
    if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        if let Some(&i) = menu.filtered().get(menu.selected) {
            menu.request = Some(menu.entries[i].path.clone());
        }
    }
}

pub fn menu_clicks(rows: Query<(&Interaction, &MenuRow), Changed<Interaction>>, mut menu: ResMut<MapMenu>) {
    for (i, row) in &rows {
        if *i == Interaction::Pressed {
            menu.request = Some(menu.entries[row.0].path.clone());
        }
    }
}

/// Rebuild the visible rows after any change.
pub fn draw_menu(
    mut commands: Commands,
    mut menu: ResMut<MapMenu>,
    level: Res<CurrentLevel>,
    mut panel: Single<&mut Visibility, With<MenuPanel>>,
    mut header: Single<&mut Text, With<MenuHeader>>,
    list: Single<(Entity, Option<&Children>), With<MenuList>>,
) {
    if !menu.dirty {
        return;
    }
    menu.dirty = false;
    **panel = if menu.open { Visibility::Visible } else { Visibility::Hidden };
    let (list, children) = *list;
    for c in children.into_iter().flatten() {
        commands.entity(*c).despawn();
    }
    if !menu.open {
        return;
    }
    let shown = menu.filtered();
    header.0 = format!(
        "Maps ({} of {})   current: {}\nfilter: {}_\n{}",
        shown.len(),
        menu.entries.len(),
        level.name,
        menu.filter,
        if menu.status.is_empty() { "type to filter, Up/Down, Enter or click to load, Esc to close" } else { &menu.status }
    );
    let first = menu.selected.saturating_sub(ROWS / 2).min(shown.len().saturating_sub(ROWS));
    commands.entity(list).with_children(|p| {
        for (k, &i) in shown.iter().enumerate().skip(first).take(ROWS) {
            let e = &menu.entries[i];
            let is_current = e.path == level.path;
            let bg = if k == menu.selected { Color::srgba(0.25, 0.45, 0.85, 0.9) } else { Color::srgba(0.0, 0.0, 0.0, 0.0) };
            let text = format!("{}{}", if is_current { "* " } else { "  " }, e.label);
            p.spawn((Button, Node { padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)), ..Default::default() }, BackgroundColor(bg), MenuRow(i)))
                .with_child((Text::new(text), mono(15.0), TextColor(Color::srgb(0.92, 0.94, 0.96))));
        }
    });
}

/// Load the requested level: new collision in the sim, new geometry on screen, player at its spawn.
#[allow(clippy::too_many_arguments)]
pub fn load_requested(
    mut commands: Commands,
    mut menu: ResMut<MapMenu>,
    mut sim: ResMut<Sim>,
    mut angles: ResMut<ViewAngles>,
    mut current: ResMut<CurrentLevel>,
    old: Query<Entity, With<LevelEntity>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    ghost: Option<Res<crate::ghost::Ghost>>,
    mut cursor: Single<&mut CursorOptions>,
) {
    let Some(path) = menu.request.take() else { return };
    menu.dirty = true;
    if ghost.is_some() {
        menu.status = "a ghost run uses the test level; maps can't change".into();
        return;
    }
    match level::load(path.as_deref()) {
        Ok(l) => {
            for e in &old {
                commands.entity(e).despawn();
            }
            level::spawn_level(&mut commands, &mut meshes, &mut materials, &mut images, &l.geometry);
            let a = l.info.areas[0].clone();
            sim.change_level(l.world, (a.spawn, a.yaw));
            *angles = ViewAngles { pitch: 0.0, yaw: a.yaw };
            info!("loaded {} ({} areas)", l.info.name, l.info.areas.len());
            menu.status = format!("loaded {}", l.info.name);
            *current = l.info;
            menu.open = false;
            cursor.visible = false;
            cursor.grab_mode = CursorGrabMode::Locked;
        }
        Err(e) => {
            warn!("{e}");
            menu.status = e;
        }
    }
}

pub fn setup_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut start: ResMut<StartGeometry>,
) {
    if let Some(g) = start.0.take() {
        level::spawn_level(&mut commands, &mut meshes, &mut materials, &mut images, &g);
    }
}

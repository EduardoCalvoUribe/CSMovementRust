//! Bevy host for the `movement` crate: window, input, fixed-tick sim, camera, level, HUD (plan §6).
//!
//! `csmove --check <file.replay>` replays a recording headlessly and compares it with the live run.
//! `csmove --ghost <capture>` plays a real-server capture with the captured hull drawn as a ghost.
//! `csmove --map <file.bsp>` starts on a BSP map instead of the test level; `M` in game picks maps.

mod camera;
mod ghost;
mod hud;
mod input;
mod level;
mod maps;
mod modes;
mod record;
mod sim;

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy::prelude::*;
use bevy::window::PresentMode;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 3 && args[1] == "--check" {
        match record::check(std::path::Path::new(&args[2])) {
            Ok(msg) => {
                println!("{msg}");
                return;
            }
            Err(msg) => {
                eprintln!("{msg}");
                std::process::exit(1);
            }
        }
    }

    let map_arg = args.iter().position(|a| a == "--map").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let loaded = level::load(map_arg.as_deref()).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1)
    });
    let first = &loaded.info.areas[0];
    let spawn = (first.spawn, first.yaw);
    let mut sim = sim::Sim::new(loaded.world, spawn);
    let ghost = if args.len() == 3 && args[1] == "--ghost" {
        let mut g = ghost::Ghost::load(std::path::Path::new(&args[2])).unwrap_or_else(|e| {
            eprintln!("{e}");
            std::process::exit(1)
        });
        if let Err(e) = g.reset(&mut sim) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        Some(g)
    } else {
        None
    };
    let menu = maps::MapMenu::new(map_arg.as_deref());

    let mut app = App::new();
    if let Some(g) = ghost {
        app.insert_resource(g);
    }
    app
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "CSMovementRust".into(),
                present_mode: PresentMode::AutoNoVsync,
                ..Default::default()
            }),
            ..Default::default()
        }))
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.53, 0.68, 0.85)))
        .insert_resource(Time::<Fixed>::from_hz(64.0))
        .insert_resource(sim)
        .insert_resource(loaded.info)
        .insert_resource(maps::StartGeometry(Some(loaded.geometry)))
        .insert_resource(menu)
        .init_resource::<input::MouseSettings>()
        .init_resource::<input::ViewAngles>()
        .init_resource::<input::HeldInput>()
        .add_systems(Startup, (camera::spawn_camera, hud::spawn_hud, maps::spawn_menu, maps::setup_level))
        .add_systems(
            RunFixedMainLoop,
            (maps::menu_input, input::grab_cursor, input::accumulate_input, modes::hotkeys, sim::sync_timestep)
                .chain()
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        )
        .add_systems(First, sim::detect_discarded_time)
        .add_systems(FixedUpdate, sim::fixed_tick)
        .add_systems(
            Update,
            (camera::update_camera, camera::update_signs, hud::update_hud, hud::debug_gizmos, ghost::ghost_gizmos).chain(),
        )
        .add_systems(Update, (maps::menu_clicks, maps::load_requested, maps::draw_menu).chain())
        .run();
}

#[cfg(test)]
mod tests {
    use movement::trace::TraceWorld;

    #[test]
    fn spawns_are_clear_of_solid() {
        let level = crate::level::describe();
        let world = crate::level::build_world(&level);
        for a in &level.areas {
            assert!(!world.point_solid(a.spawn, movement::Hull::STAND), "{} spawn is inside solid", a.name);
        }
    }

    #[test]
    fn spawn_settles_on_ground() {
        let level = crate::level::describe();
        let world = crate::level::build_world(&level);
        for a in &level.areas {
            let mut s = crate::sim::Sim::new(world.clone().into(), (a.spawn, a.yaw));
            for _ in 0..16 {
                s.step(movement::UserCmd::default());
            }
            assert!(s.state.on_ground(), "{} spawn doesn't settle", a.name);
            assert!((s.state.origin.z - a.spawn.z).abs() < 0.01, "{} spawn fell", a.name);
        }
    }

    #[test]
    fn runway_jump_measures_to_block_top() {
        use movement::cmd::Buttons;
        let level = crate::level::describe();
        let world = crate::level::build_world(&level);
        let a = &level.areas[1];
        let mut s = crate::sim::Sim::new(world.into(), (a.spawn, a.yaw));
        let cmd = |b| movement::UserCmd::from_buttons(0, movement::Vec3::ZERO, b);
        for _ in 0..96 { s.step(cmd(Buttons::FORWARD)); }
        s.step(cmd(Buttons::FORWARD | Buttons::JUMP));
        for i in 0..80 { s.step(cmd(if i < 20 { Buttons::FORWARD } else { Buttons::NONE })); }
        // Grounded while hovering up to 2 units above the block; the report measures to the block top.
        let r = s.last_report.clone().unwrap();
        assert!((r.block_height).abs() < 1e-3, "{r:?}");
        assert!(r.landing_origin_raw.z > r.landing_origin.z);
    }
}

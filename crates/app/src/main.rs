//! Bevy host for the `movement` crate: window, input, fixed-tick sim, camera, level, HUD (plan §6).
//!
//! `csmove --check <file.replay>` replays a recording headlessly and compares it with the live run.

mod camera;
mod hud;
mod input;
mod level;
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

    let level = level::describe();
    let world = level::build_world(&level);
    let spawn = (level.areas[0].spawn, level.areas[0].yaw);

    App::new()
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
        .insert_resource(sim::Sim::new(world, spawn))
        .insert_resource(modes::LevelInfo(level))
        .init_resource::<input::MouseSettings>()
        .init_resource::<input::ViewAngles>()
        .init_resource::<input::HeldInput>()
        .add_systems(Startup, (camera::spawn_camera, hud::spawn_hud, setup_level))
        .add_systems(
            RunFixedMainLoop,
            (input::grab_cursor, input::accumulate_input, modes::hotkeys, sim::sync_timestep)
                .chain()
                .in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
        )
        .add_systems(First, sim::detect_discarded_time)
        .add_systems(FixedUpdate, sim::fixed_tick)
        .add_systems(
            Update,
            (camera::update_camera, camera::update_signs, hud::update_hud, hud::debug_gizmos).chain(),
        )
        .run();
}

fn setup_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    level: Res<modes::LevelInfo>,
) {
    level::spawn_level(&mut commands, &mut meshes, &mut materials, &mut images, &level.0);
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
            let mut s = crate::sim::Sim::new(world.clone(), (a.spawn, a.yaw));
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
        let mut s = crate::sim::Sim::new(world, (a.spawn, a.yaw));
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

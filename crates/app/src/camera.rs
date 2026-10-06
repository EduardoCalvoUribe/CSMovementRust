//! First-person camera with render interpolation (plan §6.2). Display only; nothing reads back from it.

use bevy::camera::Projection;
use bevy::prelude::*;
use movement::Vec3 as SVec3;

use crate::input::ViewAngles;
use crate::level::SignLabel;
use crate::sim::Sim;

/// Standing and crouched view offsets (plan §6.2, to confirm against S3/S21).
pub const VIEW_STAND: f32 = 64.0;
pub const VIEW_DUCK: f32 = 46.0;

/// Source (x fwd, y left, z up) to Bevy (x right, y up, z back). A rotation, so handedness is kept.
pub fn to_bevy(v: SVec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y)
}

/// Camera rotation for Source (pitch, yaw) in degrees. Source pitch is positive looking down.
pub fn view_rotation(pitch: f32, yaw: f32) -> Quat {
    Quat::from_rotation_y((yaw - 90.0).to_radians()) * Quat::from_rotation_x(-pitch.to_radians())
}

/// Eye height above the origin from the duck fraction.
pub fn eye_offset(duck_amount: f32) -> f32 {
    VIEW_STAND - (VIEW_STAND - VIEW_DUCK) * duck_amount
}

#[derive(Component)]
pub struct PlayerCamera;

pub fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 74.0f32.to_radians(),
            near: 1.0,
            far: 30000.0,
            ..Default::default()
        }),
        Transform::from_translation(Vec3::new(0.0, 64.0, 0.0)),
        PlayerCamera,
    ));
    commands.spawn((
        DirectionalLight { illuminance: 9000.0, shadow_maps_enabled: false, ..Default::default() },
        Transform::default().looking_to(Vec3::new(-0.4, -1.0, -0.25), Vec3::Y),
    ));
    commands.insert_resource(GlobalAmbientLight { brightness: 900.0, ..Default::default() });
}

/// Interpolate between the previous and current tick by the fixed-step overstep fraction.
pub fn update_camera(
    sim: Res<Sim>,
    angles: Res<ViewAngles>,
    fixed: Res<Time<Fixed>>,
    mut cam: Single<&mut Transform, With<PlayerCamera>>,
) {
    let a = if sim.paused { 1.0 } else { fixed.overstep_fraction().clamp(0.0, 1.0) };
    let prev = sim.prev_state.origin;
    let cur = sim.state.origin;
    let lerp = |p: f32, c: f32| p + (c - p) * a;
    let eye = lerp(eye_offset(sim.prev_state.duck_amount), eye_offset(sim.state.duck_amount));
    let pos = SVec3::new(lerp(prev.x, cur.x), lerp(prev.y, cur.y), lerp(prev.z, cur.z) + eye);
    cam.translation = to_bevy(pos);
    // View angles are applied at render rate; the sim samples them once per tick.
    cam.rotation = view_rotation(angles.pitch, angles.yaw);
}

/// Place each sign's overlay text at its projected world position.
pub fn update_signs(
    camera: Single<(&Camera, &GlobalTransform), With<PlayerCamera>>,
    mut signs: Query<(&SignLabel, &mut Node, &mut Visibility)>,
) {
    let (camera, cam_tf) = camera.into_inner();
    for (label, mut node, mut vis) in &mut signs {
        let dist = cam_tf.translation().distance(label.world);
        match camera.world_to_viewport(cam_tf, label.world) {
            Ok(p) if dist < 1500.0 => {
                node.left = Val::Px(p.x);
                node.top = Val::Px(p.y);
                *vis = Visibility::Visible;
            }
            _ => *vis = Visibility::Hidden,
        }
    }
}

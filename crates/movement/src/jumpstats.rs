//! Jump statistics: classification, distance, sync, strafes [Ref §16]. The classification rules are ours
//! and are documented in `docs/jumpstats-notes.md`.

use crate::cmd::UserCmd;
use crate::instrument::TechniqueFlags;
use crate::math::Vec3;
use crate::state::{MoveType, PlayerState};
use crate::trace::TraceWorld;

/// Reporting convention: add the hull width to horizontal displacement for non-ladder jumps [Ref §16].
pub const DISTANCE_OFFSET: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JumpType {
    LongJump,
    Bhop,
    MultiBhop,
    WeirdJump,
    LadderJump,
    /// Left the ground without jumping.
    Fall,
}

impl JumpType {
    pub fn short(self) -> &'static str {
        match self {
            JumpType::LongJump => "LJ",
            JumpType::Bhop => "BH",
            JumpType::MultiBhop => "MBH",
            JumpType::WeirdJump => "WJ",
            JumpType::LadderJump => "LAJ",
            JumpType::Fall => "Fall",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strafe {
    pub gain: f32,
    pub loss: f32,
    pub ticks: u32,
    pub sync_ticks: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct JumpReport {
    pub jump_type: JumpType,
    pub takeoff_origin: Vec3,
    pub landing_origin_raw: Vec3,
    pub landing_origin: Vec3,
    /// Horizontal displacement (corrected landing) plus `DISTANCE_OFFSET` where applicable.
    pub distance: f32,
    /// Horizontal displacement of the corrected landing without the offset.
    pub distance_no_offset: f32,
    /// Same, from the raw final-command origin.
    pub distance_raw: f32,
    pub pre_speed: f32,
    pub takeoff_speed: f32,
    pub max_speed: f32,
    pub height: f32,
    pub block_height: f32,
    pub airtime_ticks: u32,
    pub strafes: Vec<Strafe>,
    pub sync: f32,
    pub perfect: bool,
    pub jumpbug: bool,
    pub edgebug_in_air: bool,
    pub duckbug_landing: bool,
}

#[derive(Clone, Debug)]
struct InAir {
    jump_type: JumpType,
    takeoff_origin: Vec3,
    pre_speed: f32,
    takeoff_speed: f32,
    max_speed: f32,
    max_z: f32,
    ticks: u32,
    sync_ticks: u32,
    strafes: Vec<Strafe>,
    last_side_sign: i8,
    perfect: bool,
    jumpbug: bool,
    edgebug: bool,
}

#[derive(Clone, Debug, Default)]
pub struct JumpTracker {
    air: Option<InAir>,
    ground_ticks: u32,
    last_type: Option<JumpType>,
    last_speed: f32,
    pub last_report: Option<JumpReport>,
    /// Speed at the most recent takeoff, for the HUD "pre" readout.
    pub last_takeoff_speed: f32,
}

impl JumpTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn in_air(&self) -> bool {
        self.air.is_some()
    }

    /// Feed one command. `before`/`after` are the states around `process_movement`. `world` is used to
    /// find the floor under a landing that was grounded while still hovering above it [Ref §16].
    #[allow(clippy::too_many_arguments)]
    pub fn tick(
        &mut self,
        world: &dyn TraceWorld,
        before: &PlayerState,
        after: &PlayerState,
        cmd: &UserCmd,
        flags: TechniqueFlags,
        gravity: f32,
        dt: f32,
    ) -> Option<&JumpReport> {
        let mut finished = false;
        let speed_before = before.velocity.length_2d();
        let speed_after = after.velocity.length_2d();

        // A jumpbug lands and takes off within one command.
        if flags.jumpbug && self.air.is_some() {
            self.finish(world, before, after, true, gravity, dt);
            finished = true;
        }

        if self.air.is_some() && after.on_ground() && after.move_type == MoveType::Walk {
            // Landing.
            self.finish(world, before, after, flags.duckbug, gravity, dt);
            finished = true;
        } else if self.air.is_some() && after.move_type == MoveType::Ladder {
            self.air = None;
        } else if let Some(air) = &mut self.air {
            {
                air.ticks += 1;
                air.max_speed = air.max_speed.max(speed_after);
                air.max_z = air.max_z.max(after.origin.z);
                air.edgebug |= flags.edgebug;
                let side = if cmd.side_move > 0.0 {
                    1
                } else if cmd.side_move < 0.0 {
                    -1
                } else {
                    0
                };
                if side != 0 && side != air.last_side_sign {
                    air.strafes.push(Strafe { gain: 0.0, loss: 0.0, ticks: 0, sync_ticks: 0 });
                    air.last_side_sign = side;
                }
                let d = speed_after - speed_before;
                if d > 0.0 {
                    air.sync_ticks += 1;
                }
                if let Some(s) = air.strafes.last_mut() {
                    s.ticks += 1;
                    if d > 0.0 {
                        s.gain += d;
                        s.sync_ticks += 1;
                    } else {
                        s.loss -= d;
                    }
                }
            }
        }

        if self.air.is_none() && !after.on_ground() && after.move_type == MoveType::Walk {
            // Takeoff.
            let jump_type = if before.move_type == MoveType::Ladder {
                JumpType::LadderJump
            } else if !flags.jumped && !flags.jumpbug {
                JumpType::Fall
            } else if self.ground_ticks <= 1 || flags.jumpbug {
                match self.last_type {
                    Some(JumpType::Fall) => JumpType::WeirdJump,
                    Some(JumpType::Bhop) | Some(JumpType::MultiBhop) => JumpType::MultiBhop,
                    Some(_) => JumpType::Bhop,
                    None => JumpType::LongJump,
                }
            } else {
                JumpType::LongJump
            };
            let pre = if flags.jumpbug { self.last_speed } else { speed_before };
            self.air = Some(InAir {
                jump_type,
                takeoff_origin: before.origin,
                pre_speed: pre,
                takeoff_speed: speed_after,
                max_speed: speed_after,
                max_z: after.origin.z,
                ticks: 1,
                sync_ticks: 0,
                strafes: Vec::new(),
                last_side_sign: 0,
                perfect: self.ground_ticks <= 1,
                jumpbug: flags.jumpbug,
                edgebug: false,
            });
            if jump_type != JumpType::Fall {
                self.last_takeoff_speed = speed_after;
            }
        }

        if after.on_ground() {
            self.ground_ticks = self.ground_ticks.saturating_add(1);
            if self.ground_ticks > 1 {
                self.last_type = None;
            }
        } else {
            self.ground_ticks = 0;
        }
        self.last_speed = speed_after;

        if finished {
            self.last_report.as_ref().filter(|r| r.jump_type != JumpType::Fall)
        } else {
            None
        }
    }

    /// Estimate where the feet reached the floor during the landing command [Ref §16]. Grounding can
    /// happen up to the 2-unit probe above the floor, so first find the floor height under the final origin,
    /// then move from the previous origin along the velocity the sweep used (start velocity, vertical part
    /// minus half a gravity step) until the feet reach it.
    fn corrected_landing(world: &dyn TraceWorld, before: &PlayerState, after: &PlayerState, gravity: f32, dt: f32) -> Vec3 {
        let raw = after.origin;
        let down = Vec3::new(raw.x, raw.y, raw.z - 2.0 * crate::config::MovementConfig::vanilla().ground_probe);
        let tr = world.trace_hull(raw, down, after.hull());
        let floor_z = if tr.fraction < 1.0 && !tr.start_solid { tr.end_pos.z } else { raw.z };
        let vz = before.velocity.z - gravity * 0.5 * dt;
        if vz >= 0.0 {
            return Vec3::new(raw.x, raw.y, floor_z);
        }
        let t = ((floor_z - before.origin.z) / vz).max(0.0);
        Vec3::new(before.origin.x + before.velocity.x * t, before.origin.y + before.velocity.y * t, floor_z)
    }

    fn finish(
        &mut self,
        world: &dyn TraceWorld,
        before: &PlayerState,
        after: &PlayerState,
        duckbug: bool,
        gravity: f32,
        dt: f32,
    ) {
        let Some(air) = self.air.take() else { return };
        let raw = after.origin;
        let landing = Self::corrected_landing(world, before, after, gravity, dt);
        let horiz = |a: Vec3, b: Vec3| ((b.x - a.x) * (b.x - a.x) + (b.y - a.y) * (b.y - a.y)).sqrt();
        let d = horiz(air.takeoff_origin, landing);
        let offset = if air.jump_type == JumpType::LadderJump { 0.0 } else { DISTANCE_OFFSET };
        let sync = if air.ticks > 0 { air.sync_ticks as f32 / air.ticks as f32 * 100.0 } else { 0.0 };
        self.last_type = Some(air.jump_type);
        self.last_report = Some(JumpReport {
            jump_type: air.jump_type,
            takeoff_origin: air.takeoff_origin,
            landing_origin_raw: raw,
            landing_origin: landing,
            distance: d + offset,
            distance_no_offset: d,
            distance_raw: horiz(air.takeoff_origin, raw) + offset,
            pre_speed: air.pre_speed,
            takeoff_speed: air.takeoff_speed,
            max_speed: air.max_speed,
            height: air.max_z - air.takeoff_origin.z,
            block_height: landing.z - air.takeoff_origin.z,
            airtime_ticks: air.ticks,
            strafes: air.strafes,
            sync,
            perfect: air.perfect,
            jumpbug: air.jumpbug,
            edgebug_in_air: air.edgebug,
            duckbug_landing: duckbug,
        });
    }
}

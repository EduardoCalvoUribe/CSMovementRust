//! `Duck`, `FinishDuck`, `FinishUnDuck`, `CanUnduck` [Ref §10, §15].
//!
//! Grounded transitions keep the feet in place and swap the hull only when the transition completes.
//! Airborne transitions complete immediately and shift the origin by half the hull height difference so
//! the hull center stays put [Ref §10.1]. Both re-run categorization, which is what lets an airborne
//! unduck create support before the jump and landing code runs [Ref §15].

use crate::cmd::Buttons;
use crate::instrument::{DuckEvent, DuckKind, MoveObserver};
use crate::math::{approach, Vec3};
use crate::pipeline::Mover;
use crate::trace::{Hull, TraceWorld};

/// Half of (72 - 54): the airborne origin shift [Ref §10.1].
pub fn airborne_duck_shift() -> f32 {
    ((Hull::STAND.maxs.z - Hull::STAND.mins.z) - (Hull::DUCK.maxs.z - Hull::DUCK.mins.z)) * 0.5
}

impl<W: TraceWorld + ?Sized, O: MoveObserver + ?Sized> Mover<'_, W, O> {
    pub(crate) fn duck(&mut self) {
        let held = self.mv.buttons.contains(Buttons::DUCK);
        let was_held = self.mv.old_buttons.contains(Buttons::DUCK);
        // Crouch fatigue: each press or release slows the next transitions [Ref §10.3].
        if held != was_held {
            self.state.duck_speed = (self.state.duck_speed - self.cfg.duck_speed_penalty).max(self.cfg.duck_speed_min);
        }

        let in_air = !self.state.on_ground();
        let rate = self.dt * self.state.duck_speed;

        if held {
            if !self.state.ducked {
                self.state.ducking = true;
                self.state.duck_amount = approach(1.0, self.state.duck_amount, rate);
                if in_air || self.state.duck_amount >= 1.0 {
                    self.finish_duck();
                }
            } else {
                self.state.duck_amount = approach(1.0, self.state.duck_amount, rate);
                self.state.ducking = self.state.duck_amount < 1.0;
            }
        } else if self.state.ducked {
            if self.can_unduck() {
                self.state.ducking = true;
                self.state.duck_amount = approach(0.0, self.state.duck_amount, rate);
                if in_air || self.state.duck_amount <= 0.0 {
                    self.finish_unduck();
                }
            } else {
                // Blocked overhead: stay crouched.
                self.state.duck_amount = approach(1.0, self.state.duck_amount, rate);
                self.state.ducking = false;
            }
        } else if self.state.ducking || self.state.duck_amount > 0.0 {
            // Released before the hull changed.
            self.state.duck_amount = approach(0.0, self.state.duck_amount, rate);
            self.state.ducking = self.state.duck_amount > 0.0;
        }
    }

    fn finish_duck(&mut self) {
        let before = self.motion();
        let airborne = !self.state.on_ground();
        if !self.state.ducked {
            if airborne {
                self.state.origin.z += airborne_duck_shift();
                self.state.duck_amount = 1.0;
            }
            self.state.ducked = true;
        }
        self.state.ducking = self.state.duck_amount < 1.0;
        let ev = DuckEvent { kind: DuckKind::FinishDuck, airborne, before, after: self.motion() };
        self.obs.on_duck(&ev);
        self.categorize_position();
    }

    fn unduck_origin(&self) -> Vec3 {
        let mut o = self.state.origin;
        if !self.state.on_ground() {
            o.z -= airborne_duck_shift();
        }
        o
    }

    /// The standing hull must sweep from the current origin to the unducked origin [Ref §10.2].
    pub(crate) fn can_unduck(&self) -> bool {
        let new_origin = self.unduck_origin();
        let tr = self.world.trace_hull(self.state.origin, new_origin, Hull::STAND);
        !(tr.start_solid || tr.fraction != 1.0)
    }

    fn finish_unduck(&mut self) {
        let before = self.motion();
        let airborne = !self.state.on_ground();
        let new_origin = self.unduck_origin();
        self.state.ducked = false;
        self.state.ducking = false;
        self.state.duck_amount = 0.0;
        self.state.origin = new_origin;
        let ev = DuckEvent { kind: DuckKind::FinishUnduck, airborne, before, after: self.motion() };
        self.obs.on_duck(&ev);
        self.categorize_position();
    }
}

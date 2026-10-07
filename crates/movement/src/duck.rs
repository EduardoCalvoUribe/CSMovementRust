//! `Duck`, `FinishDuck`, `FinishUnDuck`, `CanUnduck` [Ref §10, §15].
//!
//! Grounded transitions keep the feet in place: the duck hull comes in when `duck_amount` reaches 1, and
//! the standing hull returns partway through an unduck. Airborne transitions complete immediately and
//! shift the origin by half the hull height difference so the hull center stays put [Ref §10.1]. Rates
//! and thresholds were measured on CS:GO (docs/divergences.md D5, D6). Both re-run categorization, which is what lets an airborne
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
        self.duck_transition();
        self.handle_ducking_speed_crop();
    }

    /// Scale inputs and the move maximum by `1 - 0.66 d` [Ref §10.3], using the duck amount this
    /// command's transition produced (CS:GO crops after the transition; SDK 2013 crops before it).
    fn handle_ducking_speed_crop(&mut self) {
        let d = self.state.duck_amount;
        if d > 0.0 {
            let k = 1.0 - crate::pipeline::DUCK_CROP * d;
            self.mv.forward_move *= k;
            self.mv.side_move *= k;
            self.mv.up_move *= k;
            self.mv.max_speed *= k;
        }
    }

    fn duck_transition(&mut self) {
        let held = self.mv.buttons.contains(Buttons::DUCK);
        let was_held = self.mv.old_buttons.contains(Buttons::DUCK);
        // Crouch fatigue [Ref §10.3]: a press or release costs duck speed, then it recovers; both happen
        // here, before this command's transition uses the speed (measured, docs/divergences.md D5).
        if held != was_held {
            self.state.duck_speed = (self.state.duck_speed - self.cfg.duck_speed_penalty).max(self.cfg.duck_speed_min);
        }
        // Too fatigued: the press is ignored and the player keeps unducking (crouch spam). Checked before
        // this command's recovery; the transition below uses the recovered speed.
        let wants_duck = held && self.state.duck_speed >= self.cfg.duck_refuse_below;
        self.state.duck_speed =
            approach(self.cfg.duck_speed_ideal, self.state.duck_speed, self.dt * self.cfg.duck_speed_recovery);
        let in_air = !self.state.on_ground();

        if wants_duck {
            if in_air {
                // Airborne ducks complete at once, with the hull shift [Ref §10.1].
                if !self.state.ducked {
                    self.finish_duck();
                }
                return;
            }
            let rate = self.state.duck_speed * self.cfg.duck_down_scale * self.dt;
            self.state.duck_amount = approach(1.0, self.state.duck_amount, rate);
            self.state.ducking = self.state.duck_amount < 1.0;
            if !self.state.ducked && self.state.duck_amount >= 1.0 {
                self.finish_duck();
            }
            return;
        }

        let rate = self.state.duck_speed.max(self.cfg.unduck_speed_min) * self.dt;
        if self.state.ducked {
            if in_air {
                if self.can_unduck() {
                    self.finish_unduck();
                }
                return;
            }
            if !self.can_unduck() {
                // Blocked overhead: stay crouched.
                self.state.duck_amount = approach(1.0, self.state.duck_amount, rate);
                self.state.ducking = false;
                return;
            }
            self.state.duck_amount = approach(0.0, self.state.duck_amount, rate);
            self.state.ducking = self.state.duck_amount > 0.0;
            // On the ground the standing hull returns partway through the unduck, feet in place.
            if self.state.duck_amount <= self.cfg.unduck_hull_amount {
                self.finish_unduck();
            }
        } else if self.state.duck_amount > 0.0 {
            self.state.duck_amount = approach(0.0, self.state.duck_amount, rate);
            self.state.ducking = self.state.duck_amount > 0.0;
        } else {
            self.state.ducking = false;
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
        if airborne {
            self.state.ducking = false;
            self.state.duck_amount = 0.0;
        }
        self.state.origin = new_origin;
        let ev = DuckEvent { kind: DuckKind::FinishUnduck, airborne, before, after: self.motion() };
        self.obs.on_duck(&ev);
        self.categorize_position();
    }
}

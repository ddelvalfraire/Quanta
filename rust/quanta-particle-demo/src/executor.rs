//! [`ParticleExecutor`] — applies 2D input direction to entity velocity,
//! integrates velocity into position, and clamps to world bounds.
//!
//! Plug in via `RunServerArgs.executor_factory = particle_executor_factory(hz)`.
//! Never returns `WasmTrap` — malformed inputs and NaN directions leave
//! state unchanged rather than crashing the island thread.

use crate::input::{parse_datagram, ParticleInputPayload};
use crate::schema::{
    initial_state, particle_field_indices, particle_schema, MAX_VELOCITY, WORLD_BOUND,
};

use quanta_core_rs::delta::encoder::{dequantize, quantize_field, read_state, write_state};
use quanta_realtime_server::tick::{HandleResult, TickMessage, WasmExecutor, WasmTrap};
use quanta_realtime_server::types::EntitySlot;

/// Acceleration applied when a direction is pressed (units/sec^2).
const ACCELERATION: f32 = 200.0;

/// Target damping per second when no input is held. Per-tick damping is
/// derived from this so the effective drag is tick-rate-independent:
/// `damping_per_tick = DAMPING_PER_SECOND.powf(1.0 / tick_rate_hz)`.
const DAMPING_PER_SECOND: f32 = 0.358;

pub struct ParticleExecutor {
    tick_dt_secs: f32,
    damping_per_tick: f32,
}

impl ParticleExecutor {
    /// `tick_rate_hz` must match the owning island's `TickEngineConfig.tick_rate_hz`
    /// so physics integration matches the tick loop cadence.
    pub fn new(tick_rate_hz: u8) -> Self {
        assert!(tick_rate_hz > 0, "tick_rate_hz must be > 0");
        let dt = 1.0 / tick_rate_hz as f32;
        Self {
            tick_dt_secs: dt,
            damping_per_tick: DAMPING_PER_SECOND.powf(dt),
        }
    }
}

impl Default for ParticleExecutor {
    /// Defaults to 20 Hz, matching `TickEngineConfig::default()`.
    fn default() -> Self {
        Self::new(20)
    }
}

impl WasmExecutor for ParticleExecutor {
    fn call_handle_message(
        &mut self,
        _entity: EntitySlot,
        state: &[u8],
        message: &TickMessage,
    ) -> Result<HandleResult, WasmTrap> {
        let input_payload = match message {
            TickMessage::Input { payload, .. } => payload,
            _ => {
                return Ok(HandleResult {
                    state: state.to_vec(),
                    effects: vec![],
                });
            }
        };

        // Invalid wire payloads are dropped — never trap the simulation.
        let Ok(input) = parse_datagram(input_payload) else {
            return Ok(HandleResult {
                state: state.to_vec(),
                effects: vec![],
            });
        };

        let schema = particle_schema();
        let ix = particle_field_indices();

        let state_owned = if state.is_empty() {
            initial_state()
        } else {
            state.to_vec()
        };
        let Ok(values) = read_state(schema, &state_owned) else {
            return Ok(HandleResult {
                state: state_owned,
                effects: vec![],
            });
        };

        let q_pos_x = schema.fields[ix.pos_x].quantization.as_ref().unwrap();
        let q_pos_z = schema.fields[ix.pos_z].quantization.as_ref().unwrap();
        let q_vel_x = schema.fields[ix.vel_x].quantization.as_ref().unwrap();
        let q_vel_z = schema.fields[ix.vel_z].quantization.as_ref().unwrap();

        let mut pos_x = dequantize(values[ix.pos_x], q_pos_x) as f32;
        let mut pos_z = dequantize(values[ix.pos_z], q_pos_z) as f32;
        let mut vel_x = dequantize(values[ix.vel_x], q_vel_x) as f32;
        let mut vel_z = dequantize(values[ix.vel_z], q_vel_z) as f32;

        let ParticleInputPayload { dir_x, dir_z, .. } = input;

        // Normalize + guard against NaN/inf from malicious clients.
        let magnitude = (dir_x * dir_x + dir_z * dir_z).sqrt();
        let has_input = magnitude > 1e-5 && magnitude.is_finite();
        let (ndx, ndz) = if has_input {
            (dir_x / magnitude, dir_z / magnitude)
        } else {
            (0.0, 0.0)
        };

        if has_input {
            vel_x += ndx * ACCELERATION * self.tick_dt_secs;
            vel_z += ndz * ACCELERATION * self.tick_dt_secs;
        } else {
            vel_x *= self.damping_per_tick;
            vel_z *= self.damping_per_tick;
        }

        let vmag = (vel_x * vel_x + vel_z * vel_z).sqrt();
        if vmag > MAX_VELOCITY {
            vel_x = vel_x / vmag * MAX_VELOCITY;
            vel_z = vel_z / vmag * MAX_VELOCITY;
        }

        pos_x = (pos_x + vel_x * self.tick_dt_secs).clamp(-WORLD_BOUND, WORLD_BOUND);
        pos_z = (pos_z + vel_z * self.tick_dt_secs).clamp(-WORLD_BOUND, WORLD_BOUND);

        let mut new_values = values;
        new_values[ix.pos_x] = quantize_field(pos_x as f64, q_pos_x, "pos-x").expect("clamped");
        new_values[ix.pos_z] = quantize_field(pos_z as f64, q_pos_z, "pos-z").expect("clamped");
        new_values[ix.vel_x] = quantize_field(vel_x as f64, q_vel_x, "vel-x").expect("clamped");
        new_values[ix.vel_z] = quantize_field(vel_z as f64, q_vel_z, "vel-z").expect("clamped");

        Ok(HandleResult {
            state: write_state(schema, &new_values),
            effects: vec![],
        })
    }

    fn extract_position(&self, state: &[u8]) -> (f32, f32, f32) {
        if state.is_empty() {
            return (0.0, 0.0, 0.0);
        }
        let schema = particle_schema();
        let ix = particle_field_indices();
        let Ok(values) = read_state(schema, state) else {
            return (0.0, 0.0, 0.0);
        };
        let q_x = schema.fields[ix.pos_x].quantization.as_ref().unwrap();
        let q_z = schema.fields[ix.pos_z].quantization.as_ref().unwrap();
        let x = dequantize(values[ix.pos_x], q_x) as f32;
        let z = dequantize(values[ix.pos_z], q_z) as f32;
        (x, 0.0, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{encode_datagram, ParticleInputPayload};
    use quanta_realtime_server::tick::SessionId;

    fn make_input(dir_x: f32, dir_z: f32) -> TickMessage {
        let payload = encode_datagram(&ParticleInputPayload {
            entity_slot: 0,
            input_seq: 1,
            dir_x,
            dir_z,
            actions: 0,
            dt_ms: 50,
        });
        TickMessage::Input {
            session_id: SessionId::from("test"),
            input_seq: 1,
            payload: payload.to_vec(),
        }
    }

    fn decode_pos(state: &[u8]) -> (f32, f32) {
        let schema = particle_schema();
        let ix = particle_field_indices();
        let values = read_state(schema, state).unwrap();
        (
            dequantize(
                values[ix.pos_x],
                schema.fields[ix.pos_x].quantization.as_ref().unwrap(),
            ) as f32,
            dequantize(
                values[ix.pos_z],
                schema.fields[ix.pos_z].quantization.as_ref().unwrap(),
            ) as f32,
        )
    }

    #[test]
    fn input_advances_position_in_x() {
        let mut exec = ParticleExecutor::default();
        let mut state = initial_state();
        for _ in 0..10 {
            state = exec
                .call_handle_message(EntitySlot(0), &state, &make_input(1.0, 0.0))
                .unwrap()
                .state;
        }
        let (x, _z) = decode_pos(&state);
        assert!(
            x > 1.0,
            "pos-x should advance to positive after 10 ticks, got {x}"
        );
    }

    #[test]
    fn zero_input_damps_velocity() {
        let mut exec = ParticleExecutor::default();
        let mut state = initial_state();
        for _ in 0..5 {
            state = exec
                .call_handle_message(EntitySlot(0), &state, &make_input(1.0, 0.0))
                .unwrap()
                .state;
        }
        for _ in 0..100 {
            state = exec
                .call_handle_message(EntitySlot(0), &state, &make_input(0.0, 0.0))
                .unwrap()
                .state;
        }
        let schema = particle_schema();
        let ix = particle_field_indices();
        let values = read_state(schema, &state).unwrap();
        let q = schema.fields[ix.vel_x].quantization.as_ref().unwrap();
        let vx = dequantize(values[ix.vel_x], q) as f32;
        assert!(vx.abs() < 1.0, "velocity should damp toward zero, got {vx}");
    }

    #[test]
    fn position_clamps_at_world_bound() {
        let mut exec = ParticleExecutor::default();
        let mut state = initial_state();
        for _ in 0..10_000 {
            state = exec
                .call_handle_message(EntitySlot(0), &state, &make_input(1.0, 0.0))
                .unwrap()
                .state;
        }
        let (x, _) = decode_pos(&state);
        assert!(
            x >= WORLD_BOUND - 1.0 && x <= WORLD_BOUND + 0.5,
            "should reach world bound, got {x}"
        );
    }

    #[test]
    fn invalid_payload_is_ignored() {
        let mut exec = ParticleExecutor::default();
        let state = initial_state();
        let msg = TickMessage::Input {
            session_id: SessionId::from("t"),
            input_seq: 1,
            payload: vec![0u8; 3],
        };
        let res = exec
            .call_handle_message(EntitySlot(0), &state, &msg)
            .unwrap();
        assert_eq!(res.state, state, "state unchanged on parse error");
    }

    #[test]
    fn nan_direction_is_ignored_as_zero() {
        let mut exec = ParticleExecutor::default();
        let state = initial_state();
        let msg = make_input(f32::NAN, 0.0);
        let res = exec
            .call_handle_message(EntitySlot(0), &state, &msg)
            .unwrap();
        let (x, _) = decode_pos(&res.state);
        assert!(x.abs() < 0.5, "NaN direction must not propagate into state");
    }
}

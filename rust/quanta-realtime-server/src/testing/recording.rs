use std::path::Path;

use crate::tick::*;
use crate::types::EntitySlot;

/// A recorded input event, serializable for replay.
#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone)]
pub struct RecordedInput {
    pub entity_slot: u32,
    pub session_id: String,
    pub input_seq: u32,
    pub payload: Vec<u8>,
}

/// A recorded bridge effect, serializable for replay comparison.
#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone)]
pub enum RecordedEffect {
    SendRemote { target: String, payload: Vec<u8> },
    Persist { entity_count: u32 },
    EmitTelemetry { event: String },
}

/// A single tick's recorded data.
#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone)]
pub struct TickRecord {
    pub tick_number: u64,
    pub inputs: Vec<RecordedInput>,
    pub effects: Vec<RecordedEffect>,
    pub entity_checksums: Vec<(u32, u64)>,
}

/// A full island recording that can be saved/loaded for replay.
#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone)]
pub struct IslandRecording {
    pub tick_rate_hz: u8,
    pub initial_entities: Vec<(u32, Vec<u8>)>,
    pub tick_records: Vec<TickRecord>,
}

impl IslandRecording {
    /// Save to a `.qrec` file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = bitcode::encode(self);
        std::fs::write(path, bytes)
    }

    /// Load from a `.qrec` file.
    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        bitcode::decode(&bytes).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
        })
    }
}

/// Result of replaying a recording.
#[derive(Debug)]
pub struct ReplayResult {
    pub ok: bool,
    pub divergence: Option<Divergence>,
}

/// Where replay diverged from the recording.
#[derive(Debug)]
pub struct Divergence {
    pub tick: u64,
    pub entity: u32,
    pub expected: u64,
    pub actual: u64,
}

impl RecordedEffect {
    pub fn from_bridge_effect(effect: &BridgeEffect) -> Self {
        match effect {
            BridgeEffect::SendRemote { target, payload } => RecordedEffect::SendRemote {
                target: target.clone(),
                payload: payload.clone(),
            },
            BridgeEffect::Persist { entity_states } => RecordedEffect::Persist {
                entity_count: entity_states.len() as u32,
            },
            BridgeEffect::EmitTelemetry { event } => RecordedEffect::EmitTelemetry {
                event: event.clone(),
            },
        }
    }
}

/// Replay a recording through a TestHarness, comparing entity checksums.
///
/// The `checksum_fn` computes a u64 hash from entity state bytes (e.g. xxh3).
pub fn replay<F>(
    recording: &IslandRecording,
    wasm: Box<dyn WasmExecutor>,
    checksum_fn: F,
) -> ReplayResult
where
    F: Fn(&[u8]) -> u64,
{
    use super::TestHarnessBuilder;

    let mut harness = TestHarnessBuilder::new()
        .tick_rate(recording.tick_rate_hz)
        .wasm(wasm)
        .build();

    for (slot, state) in &recording.initial_entities {
        harness.add_entity(EntitySlot(*slot), state.clone(), None);
    }

    for record in &recording.tick_records {
        // Inject inputs for this tick
        for input in &record.inputs {
            harness.send_input(ClientInput {
                session_id: SessionId::from(input.session_id.as_str()),
                entity_slot: EntitySlot(input.entity_slot),
                input_seq: input.input_seq,
                payload: input.payload.clone(),
            });
        }

        harness.tick();

        // Verify entity checksums
        for &(slot, expected_checksum) in &record.entity_checksums {
            if let Some(state) = harness.get_entity_state(&EntitySlot(slot)) {
                let actual = checksum_fn(state);
                if actual != expected_checksum {
                    return ReplayResult {
                        ok: false,
                        divergence: Some(Divergence {
                            tick: record.tick_number,
                            entity: slot,
                            expected: expected_checksum,
                            actual,
                        }),
                    };
                }
            }
        }
    }

    ReplayResult {
        ok: true,
        divergence: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::TestHarnessBuilder;

    fn xxh3(data: &[u8]) -> u64 {
        xxhash_rust::xxh3::xxh3_64(data)
    }

    /// Record a short sequence, replay it, verify no divergence.
    #[test]
    fn record_and_replay_no_divergence() {
        // Step 1: Record
        let wasm = IncrementWasm;
        let mut harness = TestHarnessBuilder::new()
            .wasm(Box::new(wasm))
            .build();

        harness.add_entity(EntitySlot(1), vec![0], None);
        let initial_entities = vec![(1u32, vec![0u8])];

        let mut tick_records = Vec::new();

        for seq in 1..=5 {
            let input = RecordedInput {
                entity_slot: 1,
                session_id: "p1".into(),
                input_seq: seq,
                payload: vec![],
            };

            harness.send_input(ClientInput {
                session_id: SessionId::from("p1"),
                entity_slot: EntitySlot(1),
                input_seq: seq,
                payload: vec![],
            });

            harness.tick();

            let state = harness.get_entity_state(&EntitySlot(1)).unwrap();
            let checksum = xxh3(state);

            let effects: Vec<RecordedEffect> = harness
                .take_effects()
                .iter()
                .map(RecordedEffect::from_bridge_effect)
                .collect();

            tick_records.push(TickRecord {
                tick_number: harness.current_tick() - 1,
                inputs: vec![input],
                effects,
                entity_checksums: vec![(1, checksum)],
            });
        }

        let recording = IslandRecording {
            tick_rate_hz: 20,
            initial_entities,
            tick_records,
        };

        // Step 2: Replay with same WASM
        let result = replay(&recording, Box::new(IncrementWasm), xxh3);
        assert!(result.ok, "replay should match");
        assert!(result.divergence.is_none());
    }

    /// Replay with a different WASM executor, verify divergence detected.
    #[test]
    fn replay_detects_divergence() {
        let initial_entities = vec![(1u32, vec![0u8])];

        // Build recording with IncrementWasm
        let mut harness = TestHarnessBuilder::new()
            .wasm(Box::new(IncrementWasm))
            .build();
        harness.add_entity(EntitySlot(1), vec![0], None);

        harness.send_input(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: EntitySlot(1),
            input_seq: 1,
            payload: vec![],
        });
        harness.tick();

        let state = harness.get_entity_state(&EntitySlot(1)).unwrap();
        let checksum = xxh3(state);

        let recording = IslandRecording {
            tick_rate_hz: 20,
            initial_entities,
            tick_records: vec![TickRecord {
                tick_number: 0,
                inputs: vec![RecordedInput {
                    entity_slot: 1,
                    session_id: "p1".into(),
                    input_seq: 1,
                    payload: vec![],
                }],
                effects: vec![],
                entity_checksums: vec![(1, checksum)],
            }],
        };

        // Replay with NoopWasmExecutor (state stays [0], won't match [1])
        let result = replay(&recording, Box::new(NoopWasmExecutor), xxh3);
        assert!(!result.ok, "replay should diverge");
        let div = result.divergence.unwrap();
        assert_eq!(div.tick, 0);
        assert_eq!(div.entity, 1);
    }

    /// Save/load roundtrip via temp file.
    #[test]
    fn save_load_roundtrip() {
        let recording = IslandRecording {
            tick_rate_hz: 30,
            initial_entities: vec![(1, vec![0, 1, 2])],
            tick_records: vec![TickRecord {
                tick_number: 0,
                inputs: vec![],
                effects: vec![],
                entity_checksums: vec![(1, 12345)],
            }],
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_recording.qrec");

        recording.save(&path).unwrap();
        let loaded = IslandRecording::load(&path).unwrap();

        assert_eq!(loaded.tick_rate_hz, 30);
        assert_eq!(loaded.initial_entities.len(), 1);
        assert_eq!(loaded.tick_records.len(), 1);

        std::fs::remove_file(&path).ok();
    }

    /// Simple WASM executor that increments the first byte.
    struct IncrementWasm;

    impl WasmExecutor for IncrementWasm {
        fn call_handle_message(
            &mut self,
            _entity: EntitySlot,
            state: &[u8],
            _message: &TickMessage,
        ) -> Result<HandleResult, WasmTrap> {
            let mut new_state = state.to_vec();
            new_state[0] = new_state[0].wrapping_add(1);
            Ok(HandleResult {
                state: new_state,
                effects: vec![],
            })
        }
    }
}

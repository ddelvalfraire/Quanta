use std::path::Path;

use crate::tick::*;
use crate::types::EntitySlot;

pub const RECORDING_FORMAT_VERSION: u16 = 1;

#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone)]
pub struct RecordedInput {
    pub entity_slot: u32,
    pub session_id: String,
    pub input_seq: u32,
    pub payload: Vec<u8>,
}

impl From<&RecordedInput> for ClientInput {
    fn from(r: &RecordedInput) -> Self {
        ClientInput {
            session_id: SessionId::from(r.session_id.as_str()),
            entity_slot: EntitySlot(r.entity_slot),
            input_seq: r.input_seq,
            payload: r.payload.clone(),
        }
    }
}

#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone)]
pub enum RecordedEffect {
    SendRemote {
        target: String,
        payload: Vec<u8>,
    },
    Persist {
        entity_count: u32,
    },
    EmitTelemetry {
        event: String,
    },
    RequestRemote {
        source_entity: u32,
        target: String,
        payload: Vec<u8>,
    },
    FireAndForget {
        target: String,
        payload: Vec<u8>,
    },
    BridgeReply {
        correlation_id: [u8; 16],
        payload: Vec<u8>,
    },
    EntityEvicted {
        entity: u32,
    },
    ZoneTransferRequest {
        player_id: String,
        source_entity: u32,
        target_zone: String,
    },
}

impl From<&BridgeEffect> for RecordedEffect {
    fn from(effect: &BridgeEffect) -> Self {
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
            BridgeEffect::RequestRemote {
                source_entity,
                target,
                payload,
            } => RecordedEffect::RequestRemote {
                source_entity: source_entity.0,
                target: target.clone(),
                payload: payload.clone(),
            },
            BridgeEffect::FireAndForget { target, payload } => RecordedEffect::FireAndForget {
                target: target.clone(),
                payload: payload.clone(),
            },
            BridgeEffect::BridgeReply {
                correlation_id,
                payload,
            } => RecordedEffect::BridgeReply {
                correlation_id: *correlation_id,
                payload: payload.clone(),
            },
            BridgeEffect::EntityEvicted { entity } => {
                RecordedEffect::EntityEvicted { entity: entity.0 }
            }
            BridgeEffect::ZoneTransferRequest {
                player_id,
                source_entity,
                target_zone,
                ..
            } => RecordedEffect::ZoneTransferRequest {
                player_id: player_id.clone(),
                source_entity: source_entity.0,
                target_zone: target_zone.0.clone(),
            },
        }
    }
}

#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone)]
pub struct TickRecord {
    pub tick_number: u64,
    pub inputs: Vec<RecordedInput>,
    pub effects: Vec<RecordedEffect>,
    pub entity_checksums: Vec<(u32, u64)>,
}

#[derive(bitcode::Encode, bitcode::Decode, Debug, Clone)]
pub struct IslandRecording {
    pub format_version: u16,
    pub tick_rate_hz: u8,
    pub initial_entities: Vec<(u32, Vec<u8>)>,
    pub tick_records: Vec<TickRecord>,
}

impl IslandRecording {
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = bitcode::encode(self);
        std::fs::write(path, bytes)
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        let recording: Self = bitcode::decode(&bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        if recording.format_version != RECORDING_FORMAT_VERSION {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "unsupported recording format version: expected {}, got {}",
                    RECORDING_FORMAT_VERSION, recording.format_version
                ),
            ));
        }
        Ok(recording)
    }
}

#[derive(Debug)]
pub struct Divergence {
    pub tick: u64,
    pub entity: u32,
    pub expected: u64,
    pub actual: u64,
}

pub fn replay<F>(
    recording: &IslandRecording,
    wasm: Box<dyn WasmExecutor>,
    checksum_fn: F,
) -> Result<(), Divergence>
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
        for input in &record.inputs {
            harness.send_input(ClientInput::from(input));
        }

        harness.tick();

        for &(slot, expected_checksum) in &record.entity_checksums {
            if let Some(state) = harness.get_entity_state(&EntitySlot(slot)) {
                let actual = checksum_fn(state);
                if actual != expected_checksum {
                    return Err(Divergence {
                        tick: record.tick_number,
                        entity: slot,
                        expected: expected_checksum,
                        actual,
                    });
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::test_executors::IncrementWasm;
    use super::super::TestHarnessBuilder;
    use super::*;

    fn xxh3(data: &[u8]) -> u64 {
        xxhash_rust::xxh3::xxh3_64(data)
    }

    #[test]
    fn record_and_replay_no_divergence() {
        let mut harness = TestHarnessBuilder::new()
            .wasm(Box::new(IncrementWasm))
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

            harness.send_input(ClientInput::from(&input));
            harness.tick();

            let state = harness.get_entity_state(&EntitySlot(1)).unwrap();
            let checksum = xxh3(state);

            let effects: Vec<RecordedEffect> = harness
                .take_effects()
                .iter()
                .map(RecordedEffect::from)
                .collect();

            tick_records.push(TickRecord {
                tick_number: harness.last_completed_tick(),
                inputs: vec![input],
                effects,
                entity_checksums: vec![(1, checksum)],
            });
        }

        let recording = IslandRecording {
            format_version: RECORDING_FORMAT_VERSION,
            tick_rate_hz: 20,
            initial_entities,
            tick_records,
        };

        replay(&recording, Box::new(IncrementWasm), xxh3).expect("replay should match");
    }

    #[test]
    fn replay_detects_divergence() {
        let initial_entities = vec![(1u32, vec![0u8])];

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
            format_version: RECORDING_FORMAT_VERSION,
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

        let div = replay(&recording, Box::new(NoopWasmExecutor), xxh3)
            .expect_err("replay should diverge");
        assert_eq!(div.tick, 0);
        assert_eq!(div.entity, 1);
    }

    #[test]
    fn save_load_roundtrip() {
        let recording = IslandRecording {
            format_version: RECORDING_FORMAT_VERSION,
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

        assert_eq!(loaded.format_version, RECORDING_FORMAT_VERSION);
        assert_eq!(loaded.tick_rate_hz, 30);
        assert_eq!(loaded.initial_entities.len(), 1);
        assert_eq!(loaded.tick_records.len(), 1);

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn load_rejects_unknown_format_version() {
        let recording = IslandRecording {
            format_version: 999,
            tick_rate_hz: 20,
            initial_entities: vec![],
            tick_records: vec![],
        };

        let dir = std::env::temp_dir();
        let path = dir.join("test_bad_version.qrec");

        recording.save(&path).unwrap();
        let err = IslandRecording::load(&path).unwrap_err();
        assert!(err
            .to_string()
            .contains("unsupported recording format version"));

        std::fs::remove_file(&path).ok();
    }
}

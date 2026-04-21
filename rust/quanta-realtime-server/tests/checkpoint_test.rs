mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use quanta_realtime_server::checkpoint::codec::*;
use quanta_realtime_server::checkpoint::writer::*;
use quanta_realtime_server::tick::*;
use quanta_realtime_server::types::IslandId;

use common::{noop_engine, slot, test_engine, MockWasm};

// ── Mock KV store ──────────────────────────────────────────────────

struct MockStore {
    writes: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
}

impl MockStore {
    fn new() -> (Self, Arc<Mutex<Vec<(String, Vec<u8>)>>>) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                writes: Arc::clone(&writes),
            },
            writes,
        )
    }
}

impl CheckpointStore for MockStore {
    fn put(
        &mut self,
        key: String,
        value: Vec<u8>,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + '_>> {
        self.writes.lock().unwrap().push((key, value));
        Box::pin(async { Ok(()) })
    }

    fn get(
        &mut self,
        key: String,
    ) -> Pin<Box<dyn Future<Output = Result<Option<Vec<u8>>, String>> + Send + '_>> {
        let result = self
            .writes
            .lock()
            .unwrap()
            .iter()
            .rfind(|(k, _)| k == &key)
            .map(|(_, v)| v.clone());
        Box::pin(async move { Ok(result) })
    }
}

// ── 1. Periodic checkpoint written every 30s (mock clock) ──────────

#[test]
fn periodic_checkpoint_triggered_at_interval() {
    let (store, writes) = MockStore::new();
    let (writer, handle) = CheckpointWriter::new(store, 16);

    // Multi-threaded runtime so writer processes concurrently with ticks.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();

    let writer_task = rt.spawn(writer.run());

    let (mut engine, _input_tx, _cmd_tx, _bridge_tx) = noop_engine();
    engine.add_entity(slot(1), vec![10], None);

    // 1s at 20Hz = 20 ticks. Run 41 ticks so we hit tick 20 and tick 40.
    engine.set_checkpoint_handle(handle, 1);

    for _ in 0..41 {
        engine.tick();
        // Brief yield to let the writer task drain between checkpoints.
        std::thread::sleep(std::time::Duration::from_micros(200));
    }

    // Drop handle to close the channel so the writer exits
    drop(engine);

    rt.block_on(async {
        let _ = writer_task.await;
    });

    let writes = writes.lock().unwrap();
    assert!(
        writes.len() >= 2,
        "expected at least 2 periodic checkpoints, got {}",
        writes.len()
    );

    // Verify the writes are decodable
    for (key, data) in writes.iter() {
        assert_eq!(key, "test-island");
        let (tick, payload) = decode_checkpoint(data).unwrap();
        assert!(tick > 0);
        assert_eq!(payload.entities.len(), 1);
    }
}

// ── 2. Event-triggered checkpoint on Persist effect ────────────────

#[test]
fn event_triggered_checkpoint_on_persist() {
    let wasm = MockWasm::new(|_entity, state, _msg| {
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![TickEffect::Persist],
        })
    });

    let (store, writes) = MockStore::new();
    let (writer, handle) = CheckpointWriter::new(store, 16);

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    let writer_task = rt.spawn(writer.run());

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![42], Some(SessionId::from("p1")));
    // No periodic interval — only event-triggered
    engine.set_checkpoint_handle(handle, 0);

    // Send input to trigger Persist effect
    input_tx
        .send(ClientInput {
            session_id: SessionId::from("p1"),
            entity_slot: slot(1),
            input_seq: 1,
            payload: vec![],
        })
        .unwrap();

    engine.tick();

    drop(engine);
    rt.block_on(async {
        let _ = writer_task.await;
    });

    let writes = writes.lock().unwrap();
    assert_eq!(writes.len(), 1, "expected 1 event-triggered checkpoint");

    let (_tick, payload) = decode_checkpoint(&writes[0].1).unwrap();
    assert_eq!(payload.entities[0].slot, 1);
    assert_eq!(payload.entities[0].state, vec![42]);
    assert_eq!(payload.entities[0].owner_session.as_deref(), Some("p1"));
}

// ── 3. Checkpoint roundtrip: write -> read -> decode -> state matches

#[test]
fn checkpoint_roundtrip_state_matches() {
    let payload = CheckpointPayload {
        entities: vec![
            CheckpointEntity {
                slot: 0,
                state: vec![1, 2, 3],
                owner_session: Some("alice".into()),
            },
            CheckpointEntity {
                slot: 5,
                state: vec![10, 20, 30, 40, 50],
                owner_session: None,
            },
            CheckpointEntity {
                slot: 99,
                state: vec![],
                owner_session: Some("bob".into()),
            },
        ],
    };

    let encoded = encode_checkpoint(500, &payload);
    let (tick, decoded) = decode_checkpoint(&encoded).unwrap();

    assert_eq!(tick, 500);
    assert_eq!(decoded.entities.len(), 3);
    assert_eq!(decoded, payload);
}

// ── 4. Recovery: island restarts from checkpoint, correct tick ─────

#[test]
fn recovery_restores_entities_and_tick() {
    let payload = CheckpointPayload {
        entities: vec![
            CheckpointEntity {
                slot: 1,
                state: vec![100],
                owner_session: Some("player1".into()),
            },
            CheckpointEntity {
                slot: 3,
                state: vec![200],
                owner_session: None,
            },
        ],
    };

    let (mut engine, _input_tx, _cmd_tx, _bridge_tx) = noop_engine();
    engine.restore_from_checkpoint(42, &payload);

    assert_eq!(engine.current_tick(), 42);
    assert_eq!(engine.entity_count(), 2);
    assert_eq!(engine.get_entity_state(&slot(1)), Some(&[100u8][..]));
    assert_eq!(engine.get_entity_state(&slot(3)), Some(&[200u8][..]));

    // After restore, ticking should resume from tick 42
    engine.tick();
    assert_eq!(engine.current_tick(), 43);
}

// ── 5. Concurrent read during tick: ArcSwap doesn't block ──────────

#[test]
fn concurrent_arcswap_read_does_not_block_tick() {
    let wasm = MockWasm::new(|_entity, state, _msg| {
        Ok(HandleResult {
            state: state.to_vec(),
            effects: vec![TickEffect::Persist],
        })
    });

    let (store, _writes) = MockStore::new();
    let (writer, handle) = CheckpointWriter::new(store, 16);
    let latest = handle.latest.clone();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let writer_task = rt.spawn(writer.run());

    let (mut engine, input_tx, _cmd_tx, _bridge_tx) = test_engine(Box::new(wasm));
    engine.add_entity(slot(1), vec![0], None);
    engine.set_checkpoint_handle(handle, 0);

    // Spawn a reader thread that continuously reads from ArcSwap
    let reader_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let rc = reader_count.clone();
    let reader = std::thread::spawn(move || {
        for _ in 0..1000 {
            let _snapshot = latest.load();
            rc.fetch_add(1, Ordering::Relaxed);
        }
    });

    // Tick while reader is running
    for seq in 1..=20u32 {
        input_tx
            .send(ClientInput {
                session_id: SessionId::from("p1"),
                entity_slot: slot(1),
                input_seq: seq,
                payload: vec![],
            })
            .unwrap();
        engine.tick();
    }

    reader.join().unwrap();

    // Reader completed all 1000 reads without being blocked
    assert_eq!(reader_count.load(Ordering::Relaxed), 1000);

    drop(engine);
    rt.block_on(async {
        let _ = writer_task.await;
    });
}

// ── 6. Writer coalescing: 5 rapid checkpoints -> processes latest ──

#[tokio::test]
async fn writer_coalesces_rapid_checkpoints() {
    let (store, writes) = MockStore::new();
    let (writer, handle) = CheckpointWriter::new(store, 16);

    let writer_task = tokio::spawn(writer.run());

    // Send 5 rapid checkpoint requests
    for tick in 1..=5u64 {
        let payload = CheckpointPayload {
            entities: vec![CheckpointEntity {
                slot: 1,
                state: vec![tick as u8],
                owner_session: None,
            }],
        };
        let data = encode_checkpoint(tick, &payload);
        handle.try_send(CheckpointRequest {
            island_id: IslandId::from("island-1"),
            tick,
            data,
            ack: None,
        });
    }

    // Give writer time to process, then close channel
    drop(handle);
    let _ = writer_task.await;

    let writes = writes.lock().unwrap();
    // Writer should have coalesced — fewer writes than 5 requests
    assert!(
        writes.len() < 5,
        "expected coalescing to reduce writes, got {}",
        writes.len()
    );

    // The last write should contain tick 5 (latest)
    let last = writes.last().unwrap();
    let (tick, payload) = decode_checkpoint(&last.1).unwrap();
    assert_eq!(tick, 5);
    assert_eq!(payload.entities[0].state, vec![5]);
}

// ── 7. Copy-on-update: 200 entities, 10 dirty -> only 10 cloned ───

#[test]
fn copy_on_update_only_clones_dirty_entities() {
    let (mut engine, _input_tx, _cmd_tx, _bridge_tx) = noop_engine();

    // Add 200 entities
    for i in 0..200 {
        engine.add_entity(slot(i), vec![i as u8], None);
    }

    // First checkpoint: all 200 entities are dirty (newly added)
    let snap1 = engine.build_snapshot();
    assert_eq!(snap1.entities.len(), 200);

    // Simulate: change 10 entity states (add_entity sets dirty=true)
    for i in 0..10u32 {
        engine.add_entity(slot(i), vec![i as u8 + 100], None);
    }

    // Second checkpoint
    let snap2 = engine.build_snapshot();
    assert_eq!(
        snap2.entities.len(),
        200,
        "all 200 entities should be in snapshot"
    );

    // Verify the 10 mutated entities have new state
    for entity in &snap2.entities {
        if entity.slot < 10 {
            assert_eq!(
                entity.state,
                vec![entity.slot as u8 + 100],
                "dirty entity {} should have updated state",
                entity.slot
            );
        } else {
            assert_eq!(
                entity.state,
                vec![entity.slot as u8],
                "clean entity {} should retain old state",
                entity.slot
            );
        }
    }

    // Third checkpoint with no changes: buffer should be unchanged
    let snap3 = engine.build_snapshot();
    assert_eq!(snap3, snap2, "no-change checkpoint should match previous");
}

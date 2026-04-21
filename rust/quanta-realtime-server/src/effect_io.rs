use crate::tick::BridgeEffect;
use crate::types::EntitySlot;

use crossbeam_channel::{Receiver, Sender, TrySendError};
use tracing::{info, warn};

const DEFAULT_BUFFER_SIZE: usize = 4096;

#[derive(Clone)]
pub struct EffectSender {
    tx: Sender<BridgeEffect>,
}

impl EffectSender {
    pub fn send(&self, effect: BridgeEffect) {
        match self.tx.try_send(effect) {
            Ok(()) => {}
            Err(TrySendError::Full(dropped)) => {
                warn!(?dropped, "effect I/O channel full, dropping effect");
            }
            Err(TrySendError::Disconnected(dropped)) => {
                warn!(?dropped, "effect I/O channel disconnected, dropping effect");
            }
        }
    }

    pub fn send_batch(&self, effects: Vec<BridgeEffect>) {
        let mut coalesced_persist: Option<Vec<(EntitySlot, Vec<u8>)>> = None;

        for effect in effects {
            match effect {
                BridgeEffect::Persist { entity_states } => {
                    coalesced_persist
                        .get_or_insert_with(Vec::new)
                        .extend(entity_states);
                }
                other => {
                    self.send(other);
                }
            }
        }

        if let Some(entity_states) = coalesced_persist {
            self.send(BridgeEffect::Persist { entity_states });
        }
    }

    pub fn pending_count(&self) -> usize {
        self.tx.len()
    }

    pub fn buffer_ratio(&self) -> f32 {
        let cap = self.tx.capacity().unwrap_or(1);
        self.tx.len() as f32 / cap as f32
    }
}

pub struct EffectReceiver {
    rx: Receiver<BridgeEffect>,
}

impl EffectReceiver {
    pub fn drain(&self) -> Vec<BridgeEffect> {
        let mut effects = Vec::new();
        while let Ok(effect) = self.rx.try_recv() {
            effects.push(effect);
        }
        effects
    }

    pub fn recv(&self) -> Option<BridgeEffect> {
        self.rx.recv().ok()
    }

    pub fn try_recv(&self) -> Option<BridgeEffect> {
        self.rx.try_recv().ok()
    }
}

pub fn effect_channel() -> (EffectSender, EffectReceiver) {
    effect_channel_with_capacity(DEFAULT_BUFFER_SIZE)
}

pub fn effect_channel_with_capacity(capacity: usize) -> (EffectSender, EffectReceiver) {
    let (tx, rx) = crossbeam_channel::bounded(capacity);
    (EffectSender { tx }, EffectReceiver { rx })
}

pub async fn run_effect_drain<F>(receiver: EffectReceiver, mut handler: F)
where
    F: FnMut(BridgeEffect) + Send + 'static,
{
    info!("effect I/O drain task started");

    loop {
        // Use tokio::task::block_in_place to bridge sync crossbeam recv with async runtime
        let effect = tokio::task::block_in_place(|| receiver.recv());

        match effect {
            Some(e) => handler(e),
            None => {
                info!("effect I/O channel closed, drain task exiting");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn send_and_drain() {
        let (tx, rx) = effect_channel_with_capacity(16);
        tx.send(BridgeEffect::EmitTelemetry {
            event: "test".into(),
        });
        tx.send(BridgeEffect::EmitTelemetry {
            event: "test2".into(),
        });

        let effects = rx.drain();
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn full_channel_drops_effect() {
        let (tx, rx) = effect_channel_with_capacity(2);
        tx.send(BridgeEffect::EmitTelemetry { event: "1".into() });
        tx.send(BridgeEffect::EmitTelemetry { event: "2".into() });
        // Channel full — this should be dropped (not panic)
        tx.send(BridgeEffect::EmitTelemetry { event: "3".into() });

        let effects = rx.drain();
        assert_eq!(effects.len(), 2);
    }

    #[test]
    fn batch_coalesces_persist() {
        let (tx, rx) = effect_channel_with_capacity(16);

        let batch = vec![
            BridgeEffect::Persist {
                entity_states: vec![(EntitySlot(1), vec![1])],
            },
            BridgeEffect::EmitTelemetry {
                event: "mid".into(),
            },
            BridgeEffect::Persist {
                entity_states: vec![(EntitySlot(2), vec![2])],
            },
        ];

        tx.send_batch(batch);

        let effects = rx.drain();
        // Should have: 1 telemetry + 1 coalesced persist
        assert_eq!(effects.len(), 2);

        let persist_count = effects
            .iter()
            .filter(|e| matches!(e, BridgeEffect::Persist { .. }))
            .count();
        assert_eq!(persist_count, 1);

        // The coalesced persist should contain both entities
        for e in &effects {
            if let BridgeEffect::Persist { entity_states } = e {
                assert_eq!(entity_states.len(), 2);
            }
        }
    }

    #[test]
    fn buffer_ratio() {
        let (tx, _rx) = effect_channel_with_capacity(4);
        assert_eq!(tx.buffer_ratio(), 0.0);
        tx.send(BridgeEffect::EmitTelemetry { event: "1".into() });
        assert_eq!(tx.buffer_ratio(), 0.25);
        tx.send(BridgeEffect::EmitTelemetry { event: "2".into() });
        assert_eq!(tx.buffer_ratio(), 0.5);
    }

    #[test]
    fn disconnected_channel_drops_gracefully() {
        let (tx, rx) = effect_channel_with_capacity(4);
        drop(rx);
        // Should not panic
        tx.send(BridgeEffect::EmitTelemetry {
            event: "orphaned".into(),
        });
    }
}

wit_bindgen::generate!({
    path: "wit/quanta-actor.wit",
    world: "actor-world",
});

use exports::quanta::actor::actor::Guest;
use quanta::actor::types::{Effect, Envelope, HandleResult};

struct CounterActor;

impl Guest for CounterActor {
    fn init(_payload: Vec<u8>) -> HandleResult {
        HandleResult {
            state: 0u64.to_le_bytes().to_vec(),
            effects: vec![],
        }
    }

    fn handle_message(state: Vec<u8>, envelope: Envelope) -> HandleResult {
        let mut counter = u64::from_le_bytes(
            state.as_slice().try_into().unwrap_or([0u8; 8]),
        );

        let payload_str = String::from_utf8_lossy(&envelope.payload);

        match payload_str.as_ref() {
            "inc" => {
                counter += 1;
                HandleResult {
                    state: counter.to_le_bytes().to_vec(),
                    effects: vec![Effect::Persist],
                }
            }
            "get" => HandleResult {
                state,
                effects: vec![Effect::Reply(counter.to_le_bytes().to_vec())],
            },
            "log" => HandleResult {
                state,
                effects: vec![Effect::Log("hello from actor".to_string())],
            },
            _ => HandleResult {
                state,
                effects: vec![],
            },
        }
    }

    fn handle_timer(state: Vec<u8>, _timer_name: String) -> HandleResult {
        let mut counter = u64::from_le_bytes(
            state.as_slice().try_into().unwrap_or([0u8; 8]),
        );
        counter += 10;
        HandleResult {
            state: counter.to_le_bytes().to_vec(),
            effects: vec![Effect::Persist],
        }
    }

    fn on_passivate(state: Vec<u8>) -> Vec<u8> {
        state
    }
}

export!(CounterActor);

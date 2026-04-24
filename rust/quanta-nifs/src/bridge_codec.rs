use quanta_core_rs::bridge::{
    decode_bridge_frame, encode_bridge_frame, BridgeHeader, BridgeMsgType,
};
use rustler::{Atom, Binary, Encoder, Env, NewBinary, Term};

use crate::safety;

mod atoms {
    rustler::atoms! {
        ok,
        error,
        msg_type,
        sequence,
        timestamp,
        correlation_id,
        activate_island,
        deactivate_island,
        player_join,
        player_leave,
        entity_command,
        state_sync,
        heartbeat,
        capacity_report,
        request,
        response,
        fire_and_forget,
        saga_failed,
    }
}

#[rustler::nif]
pub fn encode_bridge_envelope<'a>(env: Env<'a>, header: Term<'a>, payload: Binary<'a>) -> Term<'a> {
    safety::nif_safe!(env, {
        match do_encode(env, header, payload.as_slice()) {
            Ok(bytes) => {
                let mut bin = NewBinary::new(env, bytes.len());
                bin.as_mut_slice().copy_from_slice(&bytes);
                let binary: Binary = bin.into();
                (atoms::ok(), binary).encode(env)
            }
            Err(msg) => (atoms::error(), msg).encode(env),
        }
    })
}

#[rustler::nif]
pub fn decode_bridge_envelope<'a>(env: Env<'a>, frame: Binary<'a>) -> Term<'a> {
    safety::nif_safe!(env, {
        match do_decode(env, frame.as_slice()) {
            Ok((header_term, payload_bin)) => (atoms::ok(), header_term, payload_bin).encode(env),
            Err(msg) => (atoms::error(), msg).encode(env),
        }
    })
}

fn do_encode<'a>(env: Env<'a>, map: Term<'a>, payload: &[u8]) -> Result<Vec<u8>, String> {
    let msg_type = decode_msg_type_atom(env, map)?;
    let sequence = get_u64(env, map, atoms::sequence())?;
    let timestamp = get_u64(env, map, atoms::timestamp())?;
    let correlation_id = get_optional_correlation_id(env, map)?;

    let header = BridgeHeader {
        msg_type,
        sequence,
        timestamp,
        correlation_id,
    };

    Ok(encode_bridge_frame(&header, payload))
}

fn do_decode<'a>(env: Env<'a>, frame: &[u8]) -> Result<(Term<'a>, Term<'a>), String> {
    let (header, payload) =
        decode_bridge_frame(frame).map_err(|e| format!("bridge decode error: {e}"))?;

    let msg_type_atom = encode_msg_type_atom(env, header.msg_type);
    let cid_term = match header.correlation_id {
        Some(bytes) => {
            let mut bin = NewBinary::new(env, 16);
            bin.as_mut_slice().copy_from_slice(&bytes);
            let binary: Binary = bin.into();
            binary.encode(env)
        }
        None => rustler::types::atom::nil().encode(env),
    };

    let pairs: Vec<(Atom, Term)> = vec![
        (atoms::msg_type(), msg_type_atom),
        (atoms::sequence(), header.sequence.encode(env)),
        (atoms::timestamp(), header.timestamp.encode(env)),
        (atoms::correlation_id(), cid_term),
    ];

    let header_term =
        Term::map_from_pairs(env, &pairs).map_err(|_| "failed to build result map".to_string())?;

    let mut payload_bin = NewBinary::new(env, payload.len());
    payload_bin.as_mut_slice().copy_from_slice(payload);
    let payload_binary: Binary = payload_bin.into();

    Ok((header_term, payload_binary.encode(env)))
}

const MSG_TYPE_VARIANTS: [BridgeMsgType; 12] = [
    BridgeMsgType::ActivateIsland,
    BridgeMsgType::DeactivateIsland,
    BridgeMsgType::PlayerJoin,
    BridgeMsgType::PlayerLeave,
    BridgeMsgType::EntityCommand,
    BridgeMsgType::StateSync,
    BridgeMsgType::Heartbeat,
    BridgeMsgType::CapacityReport,
    BridgeMsgType::Request,
    BridgeMsgType::Response,
    BridgeMsgType::FireAndForget,
    BridgeMsgType::SagaFailed,
];

fn msg_type_atoms() -> [Atom; 12] {
    [
        atoms::activate_island(),
        atoms::deactivate_island(),
        atoms::player_join(),
        atoms::player_leave(),
        atoms::entity_command(),
        atoms::state_sync(),
        atoms::heartbeat(),
        atoms::capacity_report(),
        atoms::request(),
        atoms::response(),
        atoms::fire_and_forget(),
        atoms::saga_failed(),
    ]
}

fn decode_msg_type_atom<'a>(env: Env<'a>, map: Term<'a>) -> Result<BridgeMsgType, String> {
    let term = map_get(env, map, atoms::msg_type())?;
    let atom: Atom = term
        .decode()
        .map_err(|_| "msg_type must be an atom".to_string())?;

    for (a, variant) in msg_type_atoms().iter().zip(MSG_TYPE_VARIANTS.iter()) {
        if atom == *a {
            return Ok(*variant);
        }
    }
    Err("unknown msg_type atom".into())
}

fn encode_msg_type_atom<'a>(env: Env<'a>, msg_type: BridgeMsgType) -> Term<'a> {
    match msg_type {
        BridgeMsgType::ActivateIsland => atoms::activate_island().encode(env),
        BridgeMsgType::DeactivateIsland => atoms::deactivate_island().encode(env),
        BridgeMsgType::PlayerJoin => atoms::player_join().encode(env),
        BridgeMsgType::PlayerLeave => atoms::player_leave().encode(env),
        BridgeMsgType::EntityCommand => atoms::entity_command().encode(env),
        BridgeMsgType::StateSync => atoms::state_sync().encode(env),
        BridgeMsgType::Heartbeat => atoms::heartbeat().encode(env),
        BridgeMsgType::CapacityReport => atoms::capacity_report().encode(env),
        BridgeMsgType::Request => atoms::request().encode(env),
        BridgeMsgType::Response => atoms::response().encode(env),
        BridgeMsgType::FireAndForget => atoms::fire_and_forget().encode(env),
        BridgeMsgType::SagaFailed => atoms::saga_failed().encode(env),
    }
}

fn map_get<'a>(env: Env<'a>, map: Term<'a>, key: Atom) -> Result<Term<'a>, String> {
    map.map_get(key.encode(env))
        .map_err(|_| "missing key".to_string())
}

fn get_u64<'a>(env: Env<'a>, map: Term<'a>, key: Atom) -> Result<u64, String> {
    let term = map_get(env, map, key)?;
    term.decode::<u64>().map_err(|_| "expected integer".into())
}

fn get_optional_correlation_id<'a>(
    env: Env<'a>,
    map: Term<'a>,
) -> Result<Option<[u8; 16]>, String> {
    let term = map_get(env, map, atoms::correlation_id())?;
    if term == rustler::types::atom::nil().encode(env) {
        Ok(None)
    } else {
        let bin: Binary = term
            .decode()
            .map_err(|_| "correlation_id must be a 16-byte binary or nil".to_string())?;
        let bytes: [u8; 16] = bin
            .as_slice()
            .try_into()
            .map_err(|_| format!("correlation_id must be exactly 16 bytes, got {}", bin.len()))?;
        Ok(Some(bytes))
    }
}

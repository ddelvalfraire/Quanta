use quanta_core_rs::{EnvelopeHeader, SenderWire};
use rustler::{Atom, Binary, Encoder, Env, NewBinary, Term};

use crate::safety;

mod atoms {
    rustler::atoms! {
        ok,
        error,
        message_id,
        wall_us,
        logical,
        correlation_id,
        causation_id,
        sender,
        metadata,
        actor,
        client,
        system,
        namespace,
        id,
    }
}

#[rustler::nif]
pub fn encode_envelope_header<'a>(env: Env<'a>, header: Term<'a>) -> Term<'a> {
    safety::nif_safe!(env, {
        match do_encode(env, header) {
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
pub fn decode_envelope_header<'a>(env: Env<'a>, data: Binary<'a>) -> Term<'a> {
    safety::nif_safe!(env, {
        match do_decode(env, data.as_slice()) {
            Ok(term) => (atoms::ok(), term).encode(env),
            Err(msg) => (atoms::error(), msg).encode(env),
        }
    })
}

fn do_encode<'a>(env: Env<'a>, map: Term<'a>) -> Result<Vec<u8>, String> {
    let message_id = get_string(env, map, atoms::message_id())?;
    let wall_us = get_u64(env, map, atoms::wall_us())?;
    let logical = get_u16(env, map, atoms::logical())?;
    let correlation_id = get_optional_string(env, map, atoms::correlation_id())?;
    let causation_id = get_optional_string(env, map, atoms::causation_id())?;
    let sender_term = map_get(env, map, atoms::sender())?;
    let sender = decode_sender_term(env, sender_term)?;
    let metadata_term = map_get(env, map, atoms::metadata())?;
    let metadata = decode_metadata(metadata_term)?;

    let header = EnvelopeHeader {
        message_id,
        wall_us,
        logical,
        correlation_id,
        causation_id,
        sender,
        metadata,
    };

    Ok(bitcode::encode(&header))
}

fn do_decode<'a>(env: Env<'a>, bytes: &[u8]) -> Result<Term<'a>, String> {
    let header: EnvelopeHeader =
        bitcode::decode(bytes).map_err(|e| format!("bitcode decode error: {e}"))?;

    let sender_term = encode_sender_term(env, &header.sender);
    let metadata_term = encode_metadata(env, &header.metadata);

    let pairs: Vec<(Atom, Term)> = vec![
        (atoms::message_id(), header.message_id.encode(env)),
        (atoms::wall_us(), header.wall_us.encode(env)),
        (atoms::logical(), header.logical.encode(env)),
        (atoms::correlation_id(), header.correlation_id.encode(env)),
        (atoms::causation_id(), header.causation_id.encode(env)),
        (atoms::sender(), sender_term),
        (atoms::metadata(), metadata_term),
    ];

    Term::map_from_pairs(env, &pairs).map_err(|_| "failed to build result map".into())
}

fn decode_sender_term<'a>(env: Env<'a>, term: Term<'a>) -> Result<SenderWire, String> {
    if term.is_atom() {
        if term == atoms::system().encode(env) {
            return Ok(SenderWire::System);
        }
        if term == rustler::types::atom::nil().encode(env) {
            return Ok(SenderWire::None);
        }
        return Err("unknown sender atom".into());
    }

    let tuple = rustler::types::tuple::get_tuple(term)
        .map_err(|_| "sender must be an atom or tuple".to_string())?;

    if tuple.len() == 4 {
        let tag: Atom = tuple[0].decode().map_err(|_| "invalid sender tag")?;
        if tag == atoms::actor() {
            let ns: String = tuple[1].decode().map_err(|_| "invalid sender namespace")?;
            let typ: String = tuple[2].decode().map_err(|_| "invalid sender type")?;
            let id: String = tuple[3].decode().map_err(|_| "invalid sender id")?;
            return Ok(SenderWire::Actor {
                namespace: ns,
                typ,
                id,
            });
        }
    }

    if tuple.len() == 2 {
        let tag: Atom = tuple[0].decode().map_err(|_| "invalid sender tag")?;
        if tag == atoms::client() {
            let id: String = tuple[1].decode().map_err(|_| "invalid client id")?;
            return Ok(SenderWire::Client(id));
        }
    }

    Err("unrecognized sender format".into())
}

fn encode_sender_term<'a>(env: Env<'a>, sender: &SenderWire) -> Term<'a> {
    match sender {
        SenderWire::Actor { namespace, typ, id } => {
            (atoms::actor(), namespace.as_str(), typ.as_str(), id.as_str()).encode(env)
        }
        SenderWire::Client(id) => (atoms::client(), id.as_str()).encode(env),
        SenderWire::System => atoms::system().encode(env),
        SenderWire::None => rustler::types::atom::nil().encode(env),
    }
}

fn decode_metadata(term: Term) -> Result<Vec<(String, String)>, String> {
    let iter = rustler::types::map::MapIterator::new(term)
        .ok_or_else(|| "metadata must be a map".to_string())?;

    let mut pairs = Vec::new();
    for (k, v) in iter {
        let key: String = k.decode().map_err(|_| "metadata key must be a string")?;
        let val: String = v.decode().map_err(|_| "metadata value must be a string")?;
        pairs.push((key, val));
    }
    Ok(pairs)
}

fn encode_metadata<'a>(env: Env<'a>, metadata: &[(String, String)]) -> Term<'a> {
    let pairs: Vec<(Term, Term)> = metadata
        .iter()
        .map(|(k, v)| (k.encode(env), v.encode(env)))
        .collect();
    Term::map_from_pairs(env, &pairs).unwrap_or_else(|_| {
        let empty: &[(Term, Term)] = &[];
        Term::map_from_pairs(env, empty).unwrap()
    })
}

fn map_get<'a>(env: Env<'a>, map: Term<'a>, key: Atom) -> Result<Term<'a>, String> {
    map.map_get(key.encode(env))
        .map_err(|_| "missing key".to_string())
}

fn get_string<'a>(env: Env<'a>, map: Term<'a>, key: Atom) -> Result<String, String> {
    let term = map_get(env, map, key)?;
    term.decode::<String>().map_err(|_| "expected string".into())
}

fn get_u64<'a>(env: Env<'a>, map: Term<'a>, key: Atom) -> Result<u64, String> {
    let term = map_get(env, map, key)?;
    term.decode::<u64>().map_err(|_| "expected integer".into())
}

fn get_u16<'a>(env: Env<'a>, map: Term<'a>, key: Atom) -> Result<u16, String> {
    let term = map_get(env, map, key)?;
    term.decode::<u16>().map_err(|_| "expected integer".into())
}

fn get_optional_string<'a>(env: Env<'a>, map: Term<'a>, key: Atom) -> Result<Option<String>, String> {
    let term = map_get(env, map, key)?;
    if term == rustler::types::atom::nil().encode(env) {
        Ok(None)
    } else {
        term.decode::<String>()
            .map(Some)
            .map_err(|_| "expected string or nil".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitcode_roundtrip_header() {
        let header = EnvelopeHeader {
            message_id: "msg-1".into(),
            wall_us: 1_000_000,
            logical: 42,
            correlation_id: Some("corr-1".into()),
            causation_id: None,
            sender: SenderWire::System,
            metadata: vec![("k".into(), "v".into())],
        };

        let bytes = bitcode::encode(&header);
        let decoded: EnvelopeHeader = bitcode::decode(&bytes).unwrap();
        assert_eq!(decoded, header);
    }

    #[test]
    fn bitcode_roundtrip_all_senders() {
        for sender in [
            SenderWire::Actor {
                namespace: "ns".into(),
                typ: "t".into(),
                id: "i".into(),
            },
            SenderWire::Client("c1".into()),
            SenderWire::System,
            SenderWire::None,
        ] {
            let header = EnvelopeHeader {
                message_id: "m".into(),
                wall_us: 0,
                logical: 0,
                correlation_id: None,
                causation_id: None,
                sender,
                metadata: vec![],
            };
            let bytes = bitcode::encode(&header);
            let decoded: EnvelopeHeader = bitcode::decode(&bytes).unwrap();
            assert_eq!(decoded, header);
        }
    }
}

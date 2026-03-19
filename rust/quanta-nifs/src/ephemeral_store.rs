use std::sync::Mutex;

use loro::awareness::EphemeralStore;
use loro::LoroValue;
use rustler::{Binary, Encoder, Env, NewBinary, ResourceArc, Term};

use crate::resources::EphemeralStoreResource;

mod atoms {
    rustler::atoms! {
        ok,
        error,
        not_found,
    }
}

fn err_term<'a>(env: Env<'a>, msg: impl std::fmt::Display) -> Term<'a> {
    (atoms::error(), format!("{}", msg)).encode(env)
}

fn ok_binary<'a>(env: Env<'a>, data: &[u8]) -> Term<'a> {
    let mut bin = NewBinary::new(env, data.len());
    bin.as_mut_slice().copy_from_slice(data);
    (atoms::ok(), Binary::from(bin)).encode(env)
}

fn lock_store(
    store_arc: &ResourceArc<EphemeralStoreResource>,
) -> Result<std::sync::MutexGuard<'_, EphemeralStore>, String> {
    store_arc
        .0
        .lock()
        .map_err(|_| "ephemeral store mutex poisoned".to_string())
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_new(env: Env, timeout_ms: i64) -> Term {
    crate::macros::nif_safe!(env, {
        let store = EphemeralStore::new(timeout_ms);
        let resource = ResourceArc::new(EphemeralStoreResource(Mutex::new(store)));
        (atoms::ok(), resource).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_set<'a>(
    env: Env<'a>,
    store_arc: ResourceArc<EphemeralStoreResource>,
    key: String,
    value: Binary<'a>,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let store = match lock_store(&store_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        store.set(&key, LoroValue::from(value.as_slice().to_vec()));
        atoms::ok().encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_get(
    env: Env,
    store_arc: ResourceArc<EphemeralStoreResource>,
    key: String,
) -> Term {
    crate::macros::nif_safe!(env, {
        let store = match lock_store(&store_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        store.remove_outdated();
        match store.get(&key) {
            Some(value) => match &value {
                LoroValue::Binary(b) => {
                    let bytes: &[u8] = b;
                    ok_binary(env, bytes)
                }
                _ => return err_term(env, "unexpected non-binary LoroValue"),
            },
            None => atoms::not_found().encode(env),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_delete(
    env: Env,
    store_arc: ResourceArc<EphemeralStoreResource>,
    key: String,
) -> Term {
    crate::macros::nif_safe!(env, {
        let store = match lock_store(&store_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        store.delete(&key);
        atoms::ok().encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_get_all(env: Env, store_arc: ResourceArc<EphemeralStoreResource>) -> Term {
    crate::macros::nif_safe!(env, {
        let store = match lock_store(&store_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        store.remove_outdated();
        let all = store.get_all_states();
        let mut pairs: Vec<(Term, Term)> = Vec::with_capacity(all.len());
        for (k, v) in all.iter() {
            let key_term = k.as_str().encode(env);
            let val_term = match v {
                LoroValue::Binary(b) => {
                    let bytes: &[u8] = b;
                    let mut bin = NewBinary::new(env, bytes.len());
                    bin.as_mut_slice().copy_from_slice(bytes);
                    Binary::from(bin).encode(env)
                }
                _ => return err_term(env, "unexpected non-binary LoroValue"),
            };
            pairs.push((key_term, val_term));
        }
        let map = Term::map_from_pairs(env, &pairs).unwrap_or_else(|_| {
            let empty: &[(Term, Term)] = &[];
            Term::map_from_pairs(env, empty).unwrap()
        });
        (atoms::ok(), map).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_keys(env: Env, store_arc: ResourceArc<EphemeralStoreResource>) -> Term {
    crate::macros::nif_safe!(env, {
        let store = match lock_store(&store_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        store.remove_outdated();
        let keys = store.keys();
        let key_terms: Vec<Term> = keys.iter().map(|k| k.as_str().encode(env)).collect();
        (atoms::ok(), key_terms).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_encode(
    env: Env,
    store_arc: ResourceArc<EphemeralStoreResource>,
    key: String,
) -> Term {
    crate::macros::nif_safe!(env, {
        let store = match lock_store(&store_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        store.remove_outdated();
        let bytes = store.encode(&key);
        ok_binary(env, &bytes)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_encode_all(env: Env, store_arc: ResourceArc<EphemeralStoreResource>) -> Term {
    crate::macros::nif_safe!(env, {
        let store = match lock_store(&store_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        store.remove_outdated();
        let bytes = store.encode_all();
        ok_binary(env, &bytes)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn ephemeral_store_apply_encoded<'a>(
    env: Env<'a>,
    store_arc: ResourceArc<EphemeralStoreResource>,
    bytes: Binary<'a>,
) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        let store = match lock_store(&store_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        match store.apply(bytes.as_slice()) {
            Ok(()) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

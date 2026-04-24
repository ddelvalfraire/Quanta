use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use rustler::{Binary, Encoder, Env, LocalPid, NewBinary, OwnedEnv, ResourceArc, Term};

use super::{atoms, encode_task_panic, NatsConnectionResource, NifError};
use crate::safety::nif_safe;

#[rustler::nif]
fn kv_get_async<'a>(
    env: Env<'a>,
    conn: ResourceArc<NatsConnectionResource>,
    caller_pid: LocalPid,
    caller_ref: Term<'a>,
    bucket: String,
    key: String,
) -> Term<'a> {
    nif_safe!(env, {
        let inner = &conn.inner;

        let permit = match inner.semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return (atoms::error(), atoms::nats_backpressure()).encode(env),
        };

        let mut owned_env = OwnedEnv::new();
        let saved_ref = owned_env.save(caller_ref);
        let jetstream = inner.jetstream.clone();

        inner.runtime.spawn(async move {
            let _permit = permit;

            let result = AssertUnwindSafe(async {
                let store = jetstream
                    .get_key_value(&bucket)
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                match store.entry(&key).await {
                    Ok(Some(entry)) => {
                        if matches!(entry.operation, async_nats::jetstream::kv::Operation::Put) {
                            Ok((entry.value.to_vec(), entry.revision))
                        } else {
                            Err(NifError::NotFound)
                        }
                    }
                    Ok(None) => Err(NifError::NotFound),
                    Err(e) => Err(NifError::Other(format!("{}", e))),
                }
            })
            .catch_unwind()
            .await;

            let _ = owned_env.send_and_clear(&caller_pid, |env| {
                let ref_term = saved_ref.load(env);
                match result {
                    Ok(Ok((value, revision))) => {
                        let mut value_binary = NewBinary::new(env, value.len());
                        value_binary.as_mut_slice().copy_from_slice(&value);
                        let map = rustler::Term::map_from_pairs(
                            env,
                            &[
                                (
                                    atoms::value().encode(env),
                                    Binary::from(value_binary).encode(env),
                                ),
                                (atoms::revision().encode(env), revision.encode(env)),
                            ],
                        );
                        match map {
                            Ok(map) => (atoms::ok(), ref_term, map).encode(env),
                            Err(_) => {
                                NifError::Other("encoding_error".into()).encode_term(env, ref_term)
                            }
                        }
                    }
                    Ok(Err(nif_err)) => nif_err.encode_term(env, ref_term),
                    Err(panic) => encode_task_panic(env, ref_term, panic),
                }
            });
        });

        atoms::ok().encode(env)
    })
}

#[rustler::nif]
fn kv_put_async<'a>(
    env: Env<'a>,
    conn: ResourceArc<NatsConnectionResource>,
    caller_pid: LocalPid,
    caller_ref: Term<'a>,
    bucket: String,
    key: String,
    value: Binary<'a>,
) -> Term<'a> {
    nif_safe!(env, {
        let inner = &conn.inner;

        let permit = match inner.semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return (atoms::error(), atoms::nats_backpressure()).encode(env),
        };

        let value = value.as_slice().to_vec();
        let mut owned_env = OwnedEnv::new();
        let saved_ref = owned_env.save(caller_ref);
        let jetstream = inner.jetstream.clone();

        inner.runtime.spawn(async move {
            let _permit = permit;

            let result = AssertUnwindSafe(async {
                let store = jetstream
                    .get_key_value(&bucket)
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                store
                    .put(&key, value.into())
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))
            })
            .catch_unwind()
            .await;

            let _ = owned_env.send_and_clear(&caller_pid, |env| {
                let ref_term = saved_ref.load(env);
                match result {
                    Ok(Ok(revision)) => {
                        let map = rustler::Term::map_from_pairs(
                            env,
                            &[(atoms::revision().encode(env), revision.encode(env))],
                        );
                        match map {
                            Ok(map) => (atoms::ok(), ref_term, map).encode(env),
                            Err(_) => {
                                NifError::Other("encoding_error".into()).encode_term(env, ref_term)
                            }
                        }
                    }
                    Ok(Err(nif_err)) => nif_err.encode_term(env, ref_term),
                    Err(panic) => encode_task_panic(env, ref_term, panic),
                }
            });
        });

        atoms::ok().encode(env)
    })
}

#[rustler::nif]
fn kv_delete_async<'a>(
    env: Env<'a>,
    conn: ResourceArc<NatsConnectionResource>,
    caller_pid: LocalPid,
    caller_ref: Term<'a>,
    bucket: String,
    key: String,
) -> Term<'a> {
    nif_safe!(env, {
        let inner = &conn.inner;

        let permit = match inner.semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return (atoms::error(), atoms::nats_backpressure()).encode(env),
        };

        let mut owned_env = OwnedEnv::new();
        let saved_ref = owned_env.save(caller_ref);
        let jetstream = inner.jetstream.clone();

        inner.runtime.spawn(async move {
            let _permit = permit;

            let result = AssertUnwindSafe(async {
                let store = jetstream
                    .get_key_value(&bucket)
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                store
                    .delete(&key)
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))
            })
            .catch_unwind()
            .await;

            let _ = owned_env.send_and_clear(&caller_pid, |env| {
                let ref_term = saved_ref.load(env);
                match result {
                    Ok(Ok(())) => (atoms::ok(), ref_term).encode(env),
                    Ok(Err(nif_err)) => nif_err.encode_term(env, ref_term),
                    Err(panic) => encode_task_panic(env, ref_term, panic),
                }
            });
        });

        atoms::ok().encode(env)
    })
}

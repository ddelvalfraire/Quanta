use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use rustler::{Binary, Encoder, Env, LocalPid, OwnedEnv, ResourceArc, Term};

use super::{atoms, encode_task_panic, NatsConnectionResource, NifError};
use crate::safety::nif_safe;

#[rustler::nif]
fn js_publish_async<'a>(
    env: Env<'a>,
    conn: ResourceArc<NatsConnectionResource>,
    caller_pid: LocalPid,
    caller_ref: Term<'a>,
    subject: String,
    payload: Binary<'a>,
    expected_last_subject_seq: Option<u64>,
) -> Term<'a> {
    nif_safe!(env, {
        let inner = &conn.inner;

        let permit = match inner.semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return (atoms::error(), atoms::nats_backpressure()).encode(env),
        };

        let payload = payload.as_slice().to_vec();
        let mut owned_env = OwnedEnv::new();
        let saved_ref = owned_env.save(caller_ref);
        let jetstream = inner.jetstream.clone();

        inner.runtime.spawn(async move {
            let _permit = permit;

            let result = AssertUnwindSafe(async {
                let ack_future = if let Some(seq) = expected_last_subject_seq {
                    let mut headers = async_nats::HeaderMap::new();
                    headers.insert(
                        "Nats-Expected-Last-Subject-Sequence",
                        seq.to_string().as_str(),
                    );
                    jetstream
                        .publish_with_headers(subject, headers, payload.into())
                        .await
                } else {
                    jetstream.publish(subject, payload.into()).await
                }
                .map_err(|e| NifError::Other(format!("{}", e)))?;

                let ack = ack_future.await.map_err(|e| {
                    if matches!(
                        e.kind(),
                        async_nats::jetstream::context::PublishErrorKind::WrongLastSequence
                    ) {
                        NifError::WrongLastSequence
                    } else {
                        NifError::Other(format!("{}", e))
                    }
                })?;

                Ok::<_, NifError>((ack.stream.to_string(), ack.sequence))
            })
            .catch_unwind()
            .await;

            let _ = owned_env.send_and_clear(&caller_pid, |env| {
                let ref_term = saved_ref.load(env);
                match result {
                    Ok(Ok((stream, seq))) => {
                        let map = rustler::Term::map_from_pairs(
                            env,
                            &[
                                (atoms::stream().encode(env), stream.encode(env)),
                                (atoms::seq().encode(env), seq.encode(env)),
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

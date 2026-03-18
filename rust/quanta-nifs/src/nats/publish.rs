use rustler::{Binary, Encoder, Env, LocalPid, OwnedEnv, ResourceArc, Term};

use super::{atoms, NatsConnectionResource};
use crate::macros::nif_safe;

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

            let result = async {
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
                .map_err(|e| format!("{}", e))?;

                let ack = ack_future.await.map_err(|e| {
                    if matches!(
                        e.kind(),
                        async_nats::jetstream::context::PublishErrorKind::WrongLastSequence
                    ) {
                        "wrong_last_sequence".to_string()
                    } else {
                        format!("{}", e)
                    }
                })?;

                Ok::<_, String>((ack.stream.to_string(), ack.sequence))
            }
            .await;

            let _ = owned_env.send_and_clear(&caller_pid, |env| {
                let ref_term = saved_ref.load(env);
                match result {
                    Ok((stream, seq)) => {
                        let map = rustler::Term::map_from_pairs(
                            env,
                            &[
                                (atoms::stream().encode(env), stream.encode(env)),
                                (atoms::seq().encode(env), seq.encode(env)),
                            ],
                        )
                        .unwrap();
                        (atoms::ok(), ref_term, map).encode(env)
                    }
                    Err(reason) => {
                        let reason_term = if reason == "wrong_last_sequence" {
                            atoms::wrong_last_sequence().encode(env)
                        } else {
                            reason.encode(env)
                        };
                        (atoms::error(), ref_term, reason_term).encode(env)
                    }
                }
            });
        });

        atoms::ok().encode(env)
    })
}

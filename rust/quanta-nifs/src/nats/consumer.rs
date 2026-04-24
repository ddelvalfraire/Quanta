use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use rustler::{Binary, Encoder, Env, LocalPid, NewBinary, OwnedEnv, Resource, ResourceArc, Term};

use super::{atoms, encode_task_panic, NatsConnectionResource, NifError};
use crate::safety::nif_safe;

pub struct ConsumerResource {
    pub consumer:
        async_nats::jetstream::consumer::Consumer<async_nats::jetstream::consumer::pull::Config>,
    pub stream_name: String,
    pub consumer_name: String,
}

#[rustler::resource_impl]
impl Resource for ConsumerResource {}

#[rustler::nif]
fn consumer_create_async<'a>(
    env: Env<'a>,
    conn: ResourceArc<NatsConnectionResource>,
    caller_pid: LocalPid,
    caller_ref: Term<'a>,
    stream: String,
    subject_filter: String,
    start_seq: u64,
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
        let stream_name = stream.clone();

        inner.runtime.spawn(async move {
            let _permit = permit;

            let result = AssertUnwindSafe(async {
                let js_stream = jetstream
                    .get_stream(&stream)
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                let deliver_policy = if start_seq == 0 {
                    async_nats::jetstream::consumer::DeliverPolicy::All
                } else {
                    async_nats::jetstream::consumer::DeliverPolicy::ByStartSequence {
                        start_sequence: start_seq,
                    }
                };

                let consumer = js_stream
                    .create_consumer(async_nats::jetstream::consumer::pull::Config {
                        filter_subject: subject_filter,
                        deliver_policy,
                        ..Default::default()
                    })
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                let name = consumer.cached_info().name.clone();

                Ok::<_, NifError>((consumer, name))
            })
            .catch_unwind()
            .await;

            let _ = owned_env.send_and_clear(&caller_pid, |env| {
                let ref_term = saved_ref.load(env);
                match result {
                    Ok(Ok((consumer, name))) => {
                        let resource = ResourceArc::new(ConsumerResource {
                            consumer,
                            stream_name,
                            consumer_name: name,
                        });
                        (atoms::ok(), ref_term, resource).encode(env)
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
fn consumer_fetch_async<'a>(
    env: Env<'a>,
    conn: ResourceArc<NatsConnectionResource>,
    caller_pid: LocalPid,
    caller_ref: Term<'a>,
    consumer: ResourceArc<ConsumerResource>,
    batch_size: usize,
    timeout_ms: u64,
) -> Term<'a> {
    nif_safe!(env, {
        let inner = &conn.inner;

        let permit = match inner.semaphore.clone().try_acquire_owned() {
            Ok(p) => p,
            Err(_) => return (atoms::error(), atoms::nats_backpressure()).encode(env),
        };

        let mut owned_env = OwnedEnv::new();
        let saved_ref = owned_env.save(caller_ref);
        let consumer_clone = consumer.clone();

        inner.runtime.spawn(async move {
            let _permit = permit;

            let result = AssertUnwindSafe(async {
                use futures_util::StreamExt;

                let consumer_ref = &consumer_clone.consumer;
                let mut batch = consumer_ref
                    .fetch()
                    .max_messages(batch_size)
                    .expires(std::time::Duration::from_millis(timeout_ms))
                    .messages()
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                let mut msgs: Vec<(String, Vec<u8>, u64)> = Vec::new();
                while let Some(msg_result) = batch.next().await {
                    match msg_result {
                        Ok(msg) => {
                            let subject = msg.subject.to_string();
                            let payload = msg.payload.to_vec();
                            let info = msg.info().map_err(|e| NifError::Other(format!("{}", e)))?;
                            let seq = info.stream_sequence;
                            if let Err(e) = msg.ack().await {
                                return Err(NifError::Other(format!("ack_failed: {}", e)));
                            }
                            msgs.push((subject, payload, seq));
                        }
                        Err(e) => return Err(NifError::Other(format!("{}", e))),
                    }
                }

                Ok::<_, NifError>(msgs)
            })
            .catch_unwind()
            .await;

            let _ = owned_env.send_and_clear(&caller_pid, |env| {
                let ref_term = saved_ref.load(env);
                match result {
                    Ok(Ok(msgs)) => {
                        let msg_terms: Vec<Term> = msgs
                            .into_iter()
                            .map(|(subject, payload, seq)| {
                                let mut binary = NewBinary::new(env, payload.len());
                                binary.as_mut_slice().copy_from_slice(&payload);
                                rustler::Term::map_from_pairs(
                                    env,
                                    &[
                                        (atoms::subject().encode(env), subject.encode(env)),
                                        (
                                            atoms::payload().encode(env),
                                            Binary::from(binary).encode(env),
                                        ),
                                        (atoms::seq().encode(env), seq.encode(env)),
                                    ],
                                )
                                .expect("map_from_pairs with static keys cannot fail")
                            })
                            .collect();
                        (atoms::ok(), ref_term, msg_terms).encode(env)
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
fn consumer_delete_async<'a>(
    env: Env<'a>,
    conn: ResourceArc<NatsConnectionResource>,
    caller_pid: LocalPid,
    caller_ref: Term<'a>,
    consumer: ResourceArc<ConsumerResource>,
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
        let stream_name = consumer.stream_name.clone();
        let consumer_name = consumer.consumer_name.clone();

        inner.runtime.spawn(async move {
            let _permit = permit;

            let result = AssertUnwindSafe(async {
                let stream = jetstream
                    .get_stream(&stream_name)
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                stream
                    .delete_consumer(&consumer_name)
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                Ok::<_, NifError>(())
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

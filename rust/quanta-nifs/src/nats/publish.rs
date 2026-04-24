use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use rustler::{Binary, Encoder, Env, LocalPid, OwnedEnv, ResourceArc, Term};

use super::{atoms, encode_task_panic, NatsConnectionResource, NifError};
use crate::safety::nif_safe;

/// Publish a message to JetStream with a dual-return contract.
///
/// # Return contract
///
/// This NIF reports its result in one of **two** ways, and every caller must
/// handle **both** paths:
///
/// 1. **Synchronous backpressure error.** If the in-flight semaphore is
///    exhausted at call time, the NIF returns `{:error, :nats_backpressure}`
///    **immediately** as the synchronous return value. No message is sent to
///    `caller_pid` in this case — the error is the direct NIF return.
///
/// 2. **Asynchronous mailbox reply.** On the happy path (a permit was
///    acquired), the NIF returns `:ok` synchronously and spawns a tokio task
///    that eventually sends one of the following messages to `caller_pid`:
///
///    - `{:ok, ref, %{stream: stream_name, seq: sequence}}` on publish + ack success
///    - `{:error, ref, :wrong_last_sequence}` when JetStream rejects the expected-sequence header
///    - `{:error, ref, reason}` on any other JetStream failure, where `reason` is
///      a human-readable string formatted from the underlying async-nats error
///      (e.g. `"io error: connection reset by peer"`). The string is the raw
///      `Display` output of the error; do not try to pattern-match on its
///      contents beyond logging.
///    - `{:error, ref, reason}` if the spawned task panics, where `reason` is a
///      `"task_panic: ..."`-prefixed string from the panic payload.
///
///    The `ref` in the reply is the `caller_ref` passed in by the caller, used
///    to correlate replies when multiple publishes are in flight concurrently.
///
/// Treating the return value as "always async" silently drops backpressure
/// errors; treating it as "always sync" deadlocks on a reply that arrives via
/// mailbox. Callers MUST pattern-match on the synchronous return first, then
/// `receive` for the async reply only when the sync return was `:ok`.
///
/// # Elixir caller example
///
/// ```elixir
/// ref = make_ref()
///
/// case Quanta.Nats.js_publish_async(conn, self(), ref, subject, payload, nil) do
///   :ok ->
///     # Async mailbox reply is pending — receive it to complete the publish.
///     receive do
///       {:ok, ^ref, %{stream: stream, seq: seq}} ->
///         {:ok, {stream, seq}}
///
///       {:error, ^ref, :wrong_last_sequence} ->
///         {:error, :wrong_last_sequence}
///
///       {:error, ^ref, reason} when is_binary(reason) ->
///         # Catch-all for publish failures and task panics. `reason` is a
///         # human-readable string; a `"task_panic: "` prefix identifies a
///         # panic from the spawned tokio task.
///         {:error, reason}
///     after
///       5_000 -> {:error, :timeout}
///     end
///
///   {:error, :nats_backpressure} ->
///     # Synchronous return — the semaphore was exhausted, no message will
///     # be sent to the mailbox. Retry with backoff, shed load, or surface
///     # the error to the caller.
///     {:error, :nats_backpressure}
/// end
/// ```
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

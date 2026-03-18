use std::panic::AssertUnwindSafe;

use futures_util::FutureExt;
use rustler::{Encoder, Env, LocalPid, OwnedEnv, ResourceArc, Term};

use super::{atoms, encode_task_panic, NatsConnectionResource, NifError};
use crate::macros::nif_safe;

#[rustler::nif]
fn purge_subject_async<'a>(
    env: Env<'a>,
    conn: ResourceArc<NatsConnectionResource>,
    caller_pid: LocalPid,
    caller_ref: Term<'a>,
    stream: String,
    subject: String,
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
                let js_stream = jetstream
                    .get_stream(&stream)
                    .await
                    .map_err(|e| NifError::Other(format!("{}", e)))?;

                js_stream
                    .purge()
                    .filter(&subject)
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

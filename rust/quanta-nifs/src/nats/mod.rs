pub mod consumer;
pub mod kv;
pub mod publish;
pub mod stream;

use std::sync::Arc;

use rustler::{Encoder, Env, Resource, ResourceArc, Term};
use tokio::sync::Semaphore;

use crate::safety::nif_safe;

pub(crate) mod atoms {
    rustler::atoms! {
        ok,
        error,
        nats_backpressure,
        not_found,
        wrong_last_sequence,
        stream,
        seq,
        value,
        revision,
        subject,
        payload,
    }
}

const DEFAULT_MAX_IN_FLIGHT: usize = 10_000;
const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 5_000;

pub(crate) enum NifError {
    NotFound,
    WrongLastSequence,
    Other(String),
}

impl NifError {
    pub fn encode_term<'a>(&self, env: Env<'a>, ref_term: Term<'a>) -> Term<'a> {
        let reason = match self {
            NifError::NotFound => atoms::not_found().encode(env),
            NifError::WrongLastSequence => atoms::wrong_last_sequence().encode(env),
            NifError::Other(msg) => msg.encode(env),
        };
        (atoms::error(), ref_term, reason).encode(env)
    }
}

pub struct NatsConnectionResource {
    pub(crate) inner: NatsInner,
}

pub(crate) struct NatsInner {
    pub client: async_nats::Client,
    pub jetstream: async_nats::jetstream::Context,
    pub runtime: tokio::runtime::Runtime,
    pub semaphore: Arc<Semaphore>,
}

#[rustler::resource_impl]
impl Resource for NatsConnectionResource {}

pub(crate) fn encode_task_panic<'a>(
    env: Env<'a>,
    ref_term: Term<'a>,
    panic: Box<dyn std::any::Any + Send>,
) -> Term<'a> {
    let msg = if let Some(s) = panic.downcast_ref::<&str>() {
        format!("task_panic: {}", s)
    } else if let Some(s) = panic.downcast_ref::<String>() {
        format!("task_panic: {}", s)
    } else {
        "task_panic: unknown".to_string()
    };
    (atoms::error(), ref_term, msg.encode(env)).encode(env)
}

#[rustler::nif(schedule = "DirtyCpu")]
fn nats_connect<'a>(env: Env<'a>, urls: Vec<String>, opts: Term<'a>) -> Term<'a> {
    nif_safe!(env, {
        let max_in_flight = term_map_get::<usize>(&opts, "max_in_flight")
            .unwrap_or(DEFAULT_MAX_IN_FLIGHT);
        if max_in_flight == 0 {
            return (atoms::error(), "max_in_flight must be >= 1").encode(env);
        }
        let connect_timeout_ms = term_map_get::<u64>(&opts, "connect_timeout_ms")
            .unwrap_or(DEFAULT_CONNECT_TIMEOUT_MS);

        let runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => return (atoms::error(), format!("runtime_error: {}", e)).encode(env),
        };

        let server_addr = urls.join(",");

        let (client, jetstream) = match runtime.block_on(async {
            let client = async_nats::ConnectOptions::new()
                .connection_timeout(std::time::Duration::from_millis(connect_timeout_ms))
                .connect(&server_addr)
                .await?;
            let jetstream = async_nats::jetstream::new(client.clone());
            Ok::<_, async_nats::ConnectError>((client, jetstream))
        }) {
            Ok(pair) => pair,
            Err(e) => return (atoms::error(), format!("connect_error: {}", e)).encode(env),
        };

        let semaphore = Arc::new(Semaphore::new(max_in_flight));

        let resource = ResourceArc::new(NatsConnectionResource {
            inner: NatsInner {
                client,
                jetstream,
                runtime,
                semaphore,
            },
        });

        (atoms::ok(), resource).encode(env)
    })
}

fn term_map_get<T: TryFrom<i64>>(map_term: &Term, key: &str) -> Option<T> {
    let env = map_term.get_env();
    let atom = rustler::types::atom::Atom::from_str(env, key).ok()?;
    let val: i64 = map_term.map_get(atom.encode(env)).ok()?.decode().ok()?;
    T::try_from(val).ok()
}

pub fn load(env: Env, _: Term) -> bool {
    env.register::<NatsConnectionResource>().is_ok()
        && env.register::<consumer::ConsumerResource>().is_ok()
}

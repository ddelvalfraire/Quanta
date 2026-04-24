use rustler::{Binary, Encoder, Env, NewBinary, ResourceArc, Term};

use crate::resources::CompiledSchemaResource;
use quanta_core_rs::schema;
use quanta_core_rs::schema::evolution::{self, CompatibilityResult};

mod atoms {
    rustler::atoms! {
        ok,
        error,
        identical,
        compatible,
        incompatible,
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema_compile<'a>(
    env: Env<'a>,
    wit_source: String,
    type_name: String,
    prediction_enabled: bool,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let opts = schema::CompileOptions {
            prediction_enabled,
        };
        match schema::compile_schema(&wit_source, &type_name, &opts) {
            Ok((compiled, warnings)) => {
                let resource = ResourceArc::new(CompiledSchemaResource(compiled));
                let warning_terms: Vec<Term> =
                    warnings.iter().map(|w| w.to_string().encode(env)).collect();
                let warnings_list = warning_terms.encode(env);
                (atoms::ok(), resource, warnings_list).encode(env)
            }
            Err(e) => (atoms::error(), e.to_string()).encode(env),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema_export<'a>(
    env: Env<'a>,
    schema_arc: ResourceArc<CompiledSchemaResource>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let bytes = schema::export::export_schema(&schema_arc.0);
        let mut bin = NewBinary::new(env, bytes.len());
        bin.as_mut_slice().copy_from_slice(&bytes);
        let binary: Binary = bin.into();
        (atoms::ok(), binary).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema_import<'a>(env: Env<'a>, bytes: Binary) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        match evolution::import_schema(bytes.as_slice()) {
            Ok(compiled) => {
                let resource = ResourceArc::new(CompiledSchemaResource(compiled));
                (atoms::ok(), resource).encode(env)
            }
            Err(e) => (atoms::error(), e.to_string()).encode(env),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema_check_compatibility<'a>(
    env: Env<'a>,
    old: ResourceArc<CompiledSchemaResource>,
    new: ResourceArc<CompiledSchemaResource>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        match evolution::check_schema_compatibility(&old.0, &new.0) {
            CompatibilityResult::Identical => {
                (atoms::ok(), atoms::identical(), "".to_string()).encode(env)
            }
            CompatibilityResult::Compatible { details } => {
                (atoms::ok(), atoms::compatible(), details).encode(env)
            }
            CompatibilityResult::Incompatible { details } => {
                (atoms::ok(), atoms::incompatible(), details).encode(env)
            }
        }
    })
}

use rustler::{Binary, Encoder, Env, NewBinary, ResourceArc, Term};

use crate::resources::CompiledSchemaResource;
use quanta_core_rs::schema;

mod atoms {
    rustler::atoms! {
        ok,
        error,
    }
}

#[rustler::nif(schedule = "DirtyCpu")]
fn schema_compile<'a>(env: Env<'a>, wit_source: String, type_name: String) -> Term<'a> {
    crate::macros::nif_safe!(env, {
        match schema::compile_schema(&wit_source, &type_name) {
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
    crate::macros::nif_safe!(env, {
        let bytes = schema::export::export_schema(&schema_arc.0);
        let mut bin = NewBinary::new(env, bytes.len());
        bin.as_mut_slice().copy_from_slice(&bytes);
        let binary: Binary = bin.into();
        (atoms::ok(), binary).encode(env)
    })
}

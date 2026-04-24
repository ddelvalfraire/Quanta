use std::collections::HashMap;
use std::sync::Mutex;

use loro::{
    cursor::Side, ContainerTrait, ExpandType, ExportMode, LoroDoc, LoroListValue, LoroMapValue,
    LoroValue, StyleConfig, StyleConfigMap, TreeParentId,
};
use rustler::{Binary, Encoder, Env, NewBinary, ResourceArc, Term};

use crate::resources::{LoroDocInner, LoroDocResource};

mod atoms {
    rustler::atoms! {
        ok,
        error,
    }
}

const MAX_NESTING_DEPTH: usize = 128;

fn loro_value_to_term<'a>(env: Env<'a>, value: &LoroValue) -> Term<'a> {
    loro_value_to_term_depth(env, value, 0)
}

fn loro_value_to_term_depth<'a>(env: Env<'a>, value: &LoroValue, depth: usize) -> Term<'a> {
    if depth > MAX_NESTING_DEPTH {
        return "error: nesting too deep".encode(env);
    }
    match value {
        LoroValue::Null => rustler::types::atom::nil().encode(env),
        LoroValue::Bool(b) => b.encode(env),
        LoroValue::Double(f) => f.encode(env),
        LoroValue::I64(i) => i.encode(env),
        LoroValue::String(s) => {
            let s_ref: &str = s;
            s_ref.encode(env)
        }
        LoroValue::Binary(b) => {
            let bytes: &[u8] = b;
            let mut binary = NewBinary::new(env, bytes.len());
            binary.as_mut_slice().copy_from_slice(bytes);
            Binary::from(binary).encode(env)
        }
        LoroValue::List(l) => {
            let terms: Vec<Term<'a>> = l
                .iter()
                .map(|v| loro_value_to_term_depth(env, v, depth + 1))
                .collect();
            terms.encode(env)
        }
        LoroValue::Map(m) => {
            let pairs: Vec<(Term<'a>, Term<'a>)> = m
                .iter()
                .map(|(k, v)| {
                    (
                        k.as_str().encode(env),
                        loro_value_to_term_depth(env, v, depth + 1),
                    )
                })
                .collect();
            Term::map_from_pairs(env, &pairs).unwrap_or_else(|_| {
                let empty: &[(Term, Term)] = &[];
                Term::map_from_pairs(env, empty).unwrap()
            })
        }
        LoroValue::Container(c) => format!("{:?}", c).encode(env),
    }
}

fn term_to_loro_value<'a>(env: Env<'a>, term: Term<'a>) -> Result<LoroValue, String> {
    term_to_loro_value_depth(env, term, 0)
}

fn term_to_loro_value_depth<'a>(
    env: Env<'a>,
    term: Term<'a>,
    depth: usize,
) -> Result<LoroValue, String> {
    if depth > MAX_NESTING_DEPTH {
        return Err("nesting too deep (max 128 levels)".into());
    }

    if term == rustler::types::atom::nil().encode(env) {
        return Ok(LoroValue::Null);
    }

    if let Ok(b) = term.decode::<bool>() {
        return Ok(LoroValue::Bool(b));
    }

    if let Ok(i) = term.decode::<i64>() {
        return Ok(LoroValue::I64(i));
    }

    if let Ok(f) = term.decode::<f64>() {
        return Ok(LoroValue::Double(f));
    }

    if let Ok(s) = term.decode::<String>() {
        return Ok(LoroValue::from(s));
    }

    if let Ok(bin) = term.decode::<Binary>() {
        return Ok(LoroValue::from(bin.as_slice().to_vec()));
    }

    if term.is_list() {
        let list: Vec<Term> = term
            .decode()
            .map_err(|_| "failed to decode list".to_string())?;
        let values: Result<Vec<LoroValue>, String> = list
            .into_iter()
            .map(|t| term_to_loro_value_depth(env, t, depth + 1))
            .collect();
        return Ok(LoroValue::List(LoroListValue::from(values?)));
    }

    if let Some(iter) = rustler::types::map::MapIterator::new(term) {
        let mut map = HashMap::new();
        for (k, v) in iter {
            let key: String = k
                .decode()
                .map_err(|_| "map key must be a string".to_string())?;
            let value = term_to_loro_value_depth(env, v, depth + 1)?;
            map.insert(key, value);
        }
        return Ok(LoroValue::Map(LoroMapValue::from(map)));
    }

    Err("unsupported term type".into())
}

fn err_term<'a>(env: Env<'a>, msg: impl std::fmt::Display) -> Term<'a> {
    (atoms::error(), format!("{}", msg)).encode(env)
}

fn ok_binary<'a>(env: Env<'a>, data: &[u8]) -> Term<'a> {
    let mut bin = NewBinary::new(env, data.len());
    bin.as_mut_slice().copy_from_slice(data);
    (atoms::ok(), Binary::from(bin)).encode(env)
}

fn lock_doc(
    doc_arc: &ResourceArc<LoroDocResource>,
) -> Result<std::sync::MutexGuard<'_, LoroDocInner>, String> {
    doc_arc
        .0
        .lock()
        .map_err(|_| "loro doc mutex poisoned".to_string())
}

fn parse_expand(s: &str) -> Result<ExpandType, String> {
    match s {
        "after" => Ok(ExpandType::After),
        "before" => Ok(ExpandType::Before),
        "both" => Ok(ExpandType::Both),
        "none" => Ok(ExpandType::None),
        _ => Err(format!(
            "invalid expand type '{}', must be: after, before, both, none",
            s
        )),
    }
}

fn parse_side(side: i32) -> Result<Side, String> {
    match side {
        -1 => Ok(Side::Left),
        0 => Ok(Side::Middle),
        1 => Ok(Side::Right),
        _ => Err(format!(
            "invalid side {}, must be: -1 (left), 0 (middle), 1 (right)",
            side
        )),
    }
}

fn tree_id_to_string(id: &loro::TreeID) -> String {
    format!("{}:{}", id.peer, id.counter)
}

fn parse_tree_id(s: &str) -> Result<loro::TreeID, String> {
    let parts: Vec<&str> = s.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(format!("invalid tree_id '{}', expected 'peer:counter'", s));
    }
    let peer: u64 = parts[0]
        .parse()
        .map_err(|_| format!("invalid peer_id in tree_id '{}'", s))?;
    let counter: i32 = parts[1]
        .parse()
        .map_err(|_| format!("invalid counter in tree_id '{}'", s))?;
    Ok(loro::TreeID::new(peer, counter))
}

// Version vector wire format: [count:u32 BE][peer_id:u64 BE, counter:i32 BE] * count
// Empty binary = empty version vector (export everything).

fn encode_version_vector(vv: &loro::VersionVector) -> Vec<u8> {
    let count = vv.len() as u32;
    let mut buf = Vec::with_capacity(4 + (count as usize) * 12);
    buf.extend_from_slice(&count.to_be_bytes());
    for (&peer, &counter) in vv.iter() {
        buf.extend_from_slice(&peer.to_be_bytes());
        buf.extend_from_slice(&counter.to_be_bytes());
    }
    buf
}

fn decode_version_vector(bytes: &[u8]) -> Result<loro::VersionVector, String> {
    if bytes.is_empty() {
        return Ok(loro::VersionVector::new());
    }
    if bytes.len() < 4 {
        return Err("version vector too short".into());
    }
    let count = u32::from_be_bytes(
        bytes[0..4]
            .try_into()
            .map_err(|_| "version vector header corrupt")?,
    ) as usize;
    let expected = 4 + count * 12;
    if bytes.len() != expected {
        return Err(format!(
            "version vector size mismatch: expected {} bytes, got {}",
            expected,
            bytes.len()
        ));
    }
    let mut pairs = Vec::with_capacity(count);
    for i in 0..count {
        let offset = 4 + i * 12;
        let peer = u64::from_be_bytes(
            bytes[offset..offset + 8]
                .try_into()
                .map_err(|_| "corrupt peer_id")?,
        );
        let counter = i32::from_be_bytes(
            bytes[offset + 8..offset + 12]
                .try_into()
                .map_err(|_| "corrupt counter")?,
        );
        pairs.push((peer, counter));
    }
    Ok(pairs.into_iter().collect())
}

fn build_style_config_map(styles: &HashMap<String, StyleConfig>) -> StyleConfigMap {
    let mut scm = StyleConfigMap::new();
    for (key, config) in styles {
        scm.insert(key.as_str().into(), *config);
    }
    scm
}

fn value_or_container_to_value(voc: loro::ValueOrContainer) -> LoroValue {
    match voc {
        loro::ValueOrContainer::Value(v) => v,
        loro::ValueOrContainer::Container(c) => LoroValue::from(format!("{:?}", c.id())),
    }
}

#[rustler::nif]
fn loro_doc_new(env: Env) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = LoroDocInner {
            doc: LoroDoc::new(),
            text_styles: HashMap::new(),
        };
        let resource = ResourceArc::new(LoroDocResource(Mutex::new(inner)));
        (atoms::ok(), resource).encode(env)
    })
}

#[rustler::nif]
fn loro_doc_new_with_peer_id(env: Env, peer_id: u64) -> Term {
    crate::safety::nif_safe!(env, {
        let doc = LoroDoc::new();
        if let Err(e) = doc.set_peer_id(peer_id) {
            return err_term(env, e);
        }
        let inner = LoroDocInner {
            doc,
            text_styles: HashMap::new(),
        };
        let resource = ResourceArc::new(LoroDocResource(Mutex::new(inner)));
        (atoms::ok(), resource).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_doc_import<'a>(
    env: Env<'a>,
    doc_arc: ResourceArc<LoroDocResource>,
    bytes: Binary<'a>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        match inner.doc.import(bytes.as_slice()) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_doc_export_snapshot(env: Env, doc_arc: ResourceArc<LoroDocResource>) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        match inner.doc.export(ExportMode::Snapshot) {
            Ok(bytes) => ok_binary(env, &bytes),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_doc_export_shallow_snapshot(env: Env, doc_arc: ResourceArc<LoroDocResource>) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let frontiers = inner.doc.oplog_frontiers();
        match inner.doc.export(ExportMode::shallow_snapshot(&frontiers)) {
            Ok(bytes) => ok_binary(env, &bytes),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_doc_export_updates_from<'a>(
    env: Env<'a>,
    doc_arc: ResourceArc<LoroDocResource>,
    version_bytes: Binary<'a>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let vv = match decode_version_vector(version_bytes.as_slice()) {
            Ok(vv) => vv,
            Err(e) => return err_term(env, e),
        };
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        match inner.doc.export(ExportMode::updates(&vv)) {
            Ok(bytes) => ok_binary(env, &bytes),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_doc_get_value(env: Env, doc_arc: ResourceArc<LoroDocResource>) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let value = inner.doc.get_deep_value();
        (atoms::ok(), loro_value_to_term(env, &value)).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_doc_version(env: Env, doc_arc: ResourceArc<LoroDocResource>) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let vv = inner.doc.oplog_vv();
        let encoded = encode_version_vector(&vv);
        ok_binary(env, &encoded)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_doc_state_size(env: Env, doc_arc: ResourceArc<LoroDocResource>) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        match inner.doc.export(ExportMode::Snapshot) {
            Ok(bytes) => (atoms::ok(), bytes.len()).encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_text_insert(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    pos: usize,
    text: String,
) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let text_container = inner.doc.get_text(&*container_id);
        match text_container.insert(pos, &text) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_text_delete(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    pos: usize,
    len: usize,
) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let text_container = inner.doc.get_text(&*container_id);
        match text_container.delete(pos, len) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_doc_configure_text_style(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    key: String,
    expand: String,
) -> Term {
    crate::safety::nif_safe!(env, {
        let expand_type = match parse_expand(&expand) {
            Ok(e) => e,
            Err(msg) => return err_term(env, msg),
        };
        let mut inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        inner.text_styles.insert(
            key,
            StyleConfig {
                expand: expand_type,
            },
        );
        inner
            .doc
            .config_text_style(build_style_config_map(&inner.text_styles));
        atoms::ok().encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_text_mark<'a>(
    env: Env<'a>,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    from: usize,
    to: usize,
    key: String,
    value: Term<'a>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let loro_value = match term_to_loro_value(env, value) {
            Ok(v) => v,
            Err(msg) => return err_term(env, msg),
        };
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let text_container = inner.doc.get_text(&*container_id);
        match text_container.mark(from..to, &key, loro_value) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_text_to_string(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let text_container = inner.doc.get_text(&*container_id);
        let s = text_container.to_string();
        (atoms::ok(), s.as_str()).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_text_length(env: Env, doc_arc: ResourceArc<LoroDocResource>, container_id: String) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let text_container = inner.doc.get_text(&*container_id);
        (atoms::ok(), text_container.len_unicode()).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_map_set<'a>(
    env: Env<'a>,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    key: String,
    value: Term<'a>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let loro_value = match term_to_loro_value(env, value) {
            Ok(v) => v,
            Err(msg) => return err_term(env, msg),
        };
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let map_container = inner.doc.get_map(&*container_id);
        match map_container.insert(&key, loro_value) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_map_delete(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    key: String,
) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let map_container = inner.doc.get_map(&*container_id);
        match map_container.delete(&key) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_map_get(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    key: String,
) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let map_container = inner.doc.get_map(&*container_id);
        match map_container.get(&key) {
            Some(voc) => {
                let value = value_or_container_to_value(voc);
                (atoms::ok(), loro_value_to_term(env, &value)).encode(env)
            }
            None => err_term(env, "key not found"),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_list_insert<'a>(
    env: Env<'a>,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    index: usize,
    value: Term<'a>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let loro_value = match term_to_loro_value(env, value) {
            Ok(v) => v,
            Err(msg) => return err_term(env, msg),
        };
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let list_container = inner.doc.get_list(&*container_id);
        match list_container.insert(index, loro_value) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_list_delete(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    index: usize,
    len: usize,
) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let list_container = inner.doc.get_list(&*container_id);
        match list_container.delete(index, len) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_list_get(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    index: usize,
) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let list_container = inner.doc.get_list(&*container_id);
        match list_container.get(index) {
            Some(voc) => {
                let value = value_or_container_to_value(voc);
                (atoms::ok(), loro_value_to_term(env, &value)).encode(env)
            }
            None => err_term(env, "index out of bounds"),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_list_length(env: Env, doc_arc: ResourceArc<LoroDocResource>, container_id: String) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let list_container = inner.doc.get_list(&*container_id);
        (atoms::ok(), list_container.len()).encode(env)
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_tree_create_node(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
) -> Term {
    crate::safety::nif_safe!(env, {
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let tree = inner.doc.get_tree(&*container_id);
        match tree.create(TreeParentId::Root) {
            Ok(tree_id) => (atoms::ok(), tree_id_to_string(&tree_id)).encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_tree_move<'a>(
    env: Env<'a>,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    node_id: String,
    parent_id: Term<'a>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let tid = match parse_tree_id(&node_id) {
            Ok(id) => id,
            Err(e) => return err_term(env, e),
        };
        let parent: TreeParentId = if parent_id == rustler::types::atom::nil().encode(env) {
            TreeParentId::Root
        } else {
            let parent_str: String = match parent_id.decode() {
                Ok(s) => s,
                Err(_) => return err_term(env, "parent_id must be a string or nil"),
            };
            match parse_tree_id(&parent_str) {
                Ok(pid) => TreeParentId::Node(pid),
                Err(e) => return err_term(env, e),
            }
        };
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let tree = inner.doc.get_tree(&*container_id);
        match tree.mov(tid, parent) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_tree_delete(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    node_id: String,
) -> Term {
    crate::safety::nif_safe!(env, {
        let tid = match parse_tree_id(&node_id) {
            Ok(id) => id,
            Err(e) => return err_term(env, e),
        };
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let tree = inner.doc.get_tree(&*container_id);
        match tree.delete(tid) {
            Ok(_) => atoms::ok().encode(env),
            Err(e) => err_term(env, e),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_cursor_at(
    env: Env,
    doc_arc: ResourceArc<LoroDocResource>,
    container_id: String,
    pos: usize,
    side: i32,
) -> Term {
    crate::safety::nif_safe!(env, {
        let cursor_side = match parse_side(side) {
            Ok(s) => s,
            Err(e) => return err_term(env, e),
        };
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        let text_container = inner.doc.get_text(&*container_id);
        match text_container.get_cursor(pos, cursor_side) {
            Some(cursor) => {
                let encoded = cursor.encode();
                ok_binary(env, &encoded)
            }
            None => err_term(env, "cannot create cursor at given position"),
        }
    })
}

#[rustler::nif(schedule = "DirtyCpu")]
fn loro_cursor_pos<'a>(
    env: Env<'a>,
    doc_arc: ResourceArc<LoroDocResource>,
    cursor_bytes: Binary<'a>,
) -> Term<'a> {
    crate::safety::nif_safe!(env, {
        let cursor = match loro::cursor::Cursor::decode(cursor_bytes.as_slice()) {
            Ok(c) => c,
            Err(e) => return err_term(env, format!("failed to decode cursor: {}", e)),
        };
        let inner = match lock_doc(&doc_arc) {
            Ok(g) => g,
            Err(e) => return err_term(env, e),
        };
        match inner.doc.get_cursor_pos(&cursor) {
            Ok(result) => (atoms::ok(), result.current.pos).encode(env),
            Err(e) => err_term(env, format!("{:?}", e)),
        }
    })
}

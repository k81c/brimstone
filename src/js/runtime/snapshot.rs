//! Getter-free value snapshots for embedders (M0 embed API).
//!
//! `SnapshotValue` contains only plain Rust data. No `Handle`, `HeapPtr`, or `Value` crosses this
//! boundary. Conversion reads own data properties directly, so user-defined getters, `toJSON`,
//! `valueOf`, `toString`, Proxy traps, and iterator hooks are never invoked.

use crate::{
    common::wtf_8::Wtf8String,
    handle_scope_guard,
    runtime::{
        BsResult, Context, Handle, HeapItemKind, HeapPtr, PropertyKey, Value,
        abstract_operations::create_data_property_or_throw,
        array_object::array_create,
        error::BsError,
        eval_result::EvalError,
        gc::{Escapable, HandleScope},
        object_value::ObjectValue,
        ordinary_object::{
            ordinary_get_own_property, ordinary_object_create, ordinary_own_property_keys,
        },
        type_utilities::is_callable_object,
    },
};

/// Plain, engine-independent representation of a JS value.
#[derive(Clone, Debug, PartialEq)]
pub enum SnapshotValue {
    Null,
    Undefined,
    Bool(bool),
    Number(f64),
    String(String),
    BigInt(String),
    Array(Vec<SnapshotValue>),
    Object(Vec<(String, SnapshotValue)>),
    Function(String),
    Symbol(String),
    Circular,
    Truncated,
}

impl SnapshotValue {
    pub fn approx_bytes(&self) -> usize {
        match self {
            Self::Null | Self::Undefined => 4,
            Self::Bool(_) => 5,
            Self::Number(_) => 24,
            Self::String(s) | Self::BigInt(s) | Self::Symbol(s) | Self::Function(s) => 8 + s.len(),
            Self::Circular | Self::Truncated => 16,
            Self::Array(items) => 8 + items.iter().map(Self::approx_bytes).sum::<usize>(),
            Self::Object(entries) => {
                8 + entries
                    .iter()
                    .map(|(k, v)| 8 + k.len() + v.approx_bytes())
                    .sum::<usize>()
            }
        }
    }
}

/// Limits applied while snapshotting. Oversized values become `Truncated`.
#[derive(Clone, Copy, Debug)]
pub struct SnapshotLimits {
    pub max_depth: usize,
    pub max_items: usize,
    pub max_bytes: usize,
}

impl Default for SnapshotLimits {
    fn default() -> Self {
        Self { max_depth: 64, max_items: 10_000, max_bytes: 1024 * 1024 }
    }
}

/// Read a data property by name, walking the prototype chain for data entries only.
/// Getters, Proxy traps and user-defined `toString`/`valueOf` are never invoked.
pub fn data_string(
    mut cx: Context,
    value: Handle<Value>,
    name: &str,
    follow_prototype: bool,
) -> BsResult<Option<String>> {
    handle_scope_guard!(cx);
    if !value.is_object() {
        return Ok(None);
    }

    let mut object: Handle<ObjectValue> = value.as_object();
    loop {
        if object_ptr_kind(object) == HeapItemKind::ProxyObject {
            return Ok(None);
        }

        let key = PropertyKey::string_handle(cx, cx.alloc_string(name)?)?;
        if let Some(property) = ordinary_get_own_property(cx, object, key) {
            if property.is_data() && property.value().is_string() {
                return Ok(Some(wtf8_to_string(property.value().as_string().to_wtf8_string()?)));
            }
        }

        if !follow_prototype {
            return Ok(None);
        }
        match object.get_prototype_of(cx)? {
            Some(prototype) => object = prototype,
            None => return Ok(None),
        }
    }
}

fn object_ptr_kind(object: Handle<ObjectValue>) -> HeapItemKind {
    (*object).shape_ptr().kind()
}

/// Snapshot a JS value without invoking any user JS.
pub fn snapshot(
    cx: Context,
    value: Handle<Value>,
    limits: SnapshotLimits,
) -> BsResult<SnapshotValue> {
    handle_scope_guard!(cx);

    let mut state = SnapshotState {
        limits,
        remaining_bytes: limits.max_bytes,
        seen: Vec::new(),
        nodes: 0,
    };

    snapshot_inner(cx, value, 0, &mut state)
}

struct SnapshotState {
    limits: SnapshotLimits,
    remaining_bytes: usize,
    /// Object identities currently on the recursion path.
    seen: Vec<HeapPtr<ObjectValue>>,
    /// Number of visited nodes, used for cooperative stop/deadline checks.
    nodes: u64,
}

fn charge(state: &mut SnapshotState, bytes: usize) -> bool {
    if bytes > state.remaining_bytes {
        state.remaining_bytes = 0;
        return false;
    }
    state.remaining_bytes -= bytes;
    true
}

fn snapshot_inner(
    mut cx: Context,
    value: Handle<Value>,
    depth: usize,
    state: &mut SnapshotState,
) -> BsResult<SnapshotValue> {
    // F8: a huge result must be interruptible during conversion. Check stop/deadlines every 1024
    // visited nodes; an uncatchable Terminated propagates back to the worker.
    state.nodes += 1;
    if state.nodes.is_multiple_of(1024) && cx.vm().control().check(0).is_err() {
        return Err(BsError::Eval(EvalError::Terminated));
    }

    if depth > state.limits.max_depth || !charge(state, 8) {
        return Ok(SnapshotValue::Truncated);
    }

    let raw: Value = *value;

    if raw.is_undefined() {
        return Ok(SnapshotValue::Undefined);
    }
    if raw.is_null() {
        return Ok(SnapshotValue::Null);
    }
    if raw.is_bool() {
        return Ok(SnapshotValue::Bool(raw.as_bool()));
    }
    if raw.is_number() {
        return Ok(SnapshotValue::Number(raw.as_number()));
    }
    if raw.is_string() {
        let text = wtf8_to_string(value.as_string().to_wtf8_string()?);
        return Ok(if charge(state, text.len()) {
            SnapshotValue::String(text)
        } else {
            SnapshotValue::Truncated
        });
    }
    if raw.is_bigint() {
        let bigint = value.as_bigint().bigint().to_string();
        return Ok(if charge(state, bigint.len()) {
            SnapshotValue::BigInt(bigint)
        } else {
            SnapshotValue::Truncated
        });
    }
    if raw.is_symbol() {
        let description = match value.as_symbol().description() {
            Some(description) => wtf8_to_string(description.to_wtf8_string()),
            None => "Symbol()".to_string(),
        };
        return Ok(if charge(state, description.len()) {
            SnapshotValue::Symbol(description)
        } else {
            SnapshotValue::Truncated
        });
    }

    if !raw.is_object() {
        return Ok(SnapshotValue::Undefined);
    }

    let object: Handle<ObjectValue> = value.as_object();
    let object_ptr: HeapPtr<ObjectValue> = *object;

    if state.seen.iter().any(|ptr| ptr.ptr_eq(&object_ptr)) {
        return Ok(SnapshotValue::Circular);
    }

    let kind = object_ptr.shape_ptr().kind();

    // Proxies can run arbitrary JS in traps. Never inspect them.
    if kind == HeapItemKind::ProxyObject {
        return Ok(SnapshotValue::Function("Proxy".to_string()));
    }

    if is_callable_object(*object) {
        return Ok(SnapshotValue::Function("function".to_string()));
    }

    state.seen.push(object_ptr);

    let result = if kind == HeapItemKind::ArrayObject {
        let full_len = object.array_properties_length() as usize;
        let len = full_len.min(state.limits.max_items);
        let mut items = Vec::with_capacity(len.min(1024));
        for index in 0..len {
            match object.array_properties().get_property(cx, index as u32) {
                Some(property) if property.is_data() => {
                    items.push(snapshot_inner(cx, property.value(), depth + 1, state)?)
                }
                _ => items.push(SnapshotValue::Undefined),
            }
        }
        if full_len > len {
            items.push(SnapshotValue::Truncated);
        }
        SnapshotValue::Array(items)
    } else {
        let keys = ordinary_own_property_keys(cx, object)?;
        let mut entries = Vec::with_capacity(keys.len().min(1024));
        let mut truncated = false;
        for key_value in keys {
            if entries.len() >= state.limits.max_items {
                truncated = true;
                break;
            }

            let name = if key_value.is_string() {
                wtf8_to_string(key_value.as_string().to_wtf8_string()?)
            } else {
                match key_value.as_symbol().description() {
                    Some(description) => {
                        format!("Symbol({})", wtf8_to_string(description.to_wtf8_string()))
                    }
                    None => "Symbol()".to_string(),
                }
            };

            let key_handle = if key_value.is_string() {
                PropertyKey::string_handle(cx, key_value.as_string())?
            } else {
                PropertyKey::symbol(key_value.as_symbol())
            };

            // This is the critical path: ordinary, non-virtual, non-accessor own-property read.
            // It never invokes a getter or a Proxy trap.
            if let Some(property) = ordinary_get_own_property(cx, object, key_handle) {
                if property.is_data() {
                    let child = snapshot_inner(cx, property.value(), depth + 1, state)?;
                    if charge(state, name.len()) {
                        entries.push((name, child));
                    } else {
                        entries.push((name, SnapshotValue::Truncated));
                    }
                }
            }
        }
        if truncated {
            entries.push(("...".to_string(), SnapshotValue::Truncated));
        }
        SnapshotValue::Object(entries)
    };

    state.seen.pop();

    // Leaf values and keys were already charged during recursion. Charging `approx_bytes()`
    // again here would count the whole subtree twice and cut the effective byte budget in half.
    Ok(result)
}

fn wtf8_to_string(wtf8: Wtf8String) -> String {
    String::from_utf8_lossy(wtf8.as_bytes()).into_owned()
}

/// Build a JS value from a plain snapshot. Used to feed input documents into `__main__`.
///
/// The returned handle must escape into the caller's handle scope: a `handle_scope_guard!` would
/// pop this scope on return, leaving the handle pointing at a slot that the next allocation in
/// `call_main` reuses. That was the M1-F1 bug where a top-level string document arrived at
/// `__main__` as the `__main__` function itself.
pub fn build(cx: Context, value: &SnapshotValue) -> BsResult<Handle<Value>> {
    let scope = HandleScope::enter(cx);
    let result = build_inner(cx, value, 0);

    // Copy out any handle-bearing error before the scope is popped, then re-escape it into the
    // caller's scope (Escapable::escape must be called after the child scope is destroyed).
    let eval_error = match &result {
        Err(BsError::Eval(err)) => Some(*err),
        _ => None,
    };

    let built = match result {
        Ok(built) => built,
        Err(err) => {
            scope.exit();
            return Err(match eval_error {
                Some(err) => BsError::Eval(err.escape(cx)),
                None => err,
            });
        }
    };

    Ok(scope.escape(cx, built))
}

fn build_inner(mut cx: Context, value: &SnapshotValue, depth: usize) -> BsResult<Handle<Value>> {
    if depth > 64 {
        return Ok(cx.undefined());
    }

    match value {
        SnapshotValue::Undefined
        | SnapshotValue::Circular
        | SnapshotValue::Truncated
        | SnapshotValue::Function(_)
        | SnapshotValue::Symbol(_) => Ok(cx.undefined()),
        SnapshotValue::Null => Ok(cx.null()),
        SnapshotValue::Bool(value) => Ok(cx.bool(*value)),
        SnapshotValue::Number(value) => Ok(cx.number(*value)),
        SnapshotValue::String(value) | SnapshotValue::BigInt(value) => {
            Ok(cx.alloc_string(value)?.as_value())
        }
        SnapshotValue::Array(items) => {
            let array = array_create(cx, items.len() as u64, None)?;
            for (index, item) in items.iter().enumerate() {
                let key = PropertyKey::from_u64(cx, index as u64)?.to_handle(cx);
                let child = build_inner(cx, item, depth + 1)?;
                create_data_property_or_throw(cx, array.as_object(), key, child)?;
            }
            Ok(array.as_value())
        }
        SnapshotValue::Object(entries) => {
            let object = ordinary_object_create(cx)?;
            for (name, child) in entries {
                let key = PropertyKey::string_handle(cx, cx.alloc_string(name)?)?;
                let child = build_inner(cx, child, depth + 1)?;
                create_data_property_or_throw(cx, object, key, child)?;
            }
            Ok(object.as_value())
        }
    }
}

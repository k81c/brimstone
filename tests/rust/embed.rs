//! Tests for the embed API (`capture_main` / `call_main` / snapshots).
//!
//! These cover the M1-F1 regression: a top-level string document used to arrive at `__main__`
//! as the `__main__` function itself, because `snapshot::build` returned a handle from a scope
//! that had already been popped.

use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};

use brimstone_core::{
    common::{
        options::{ConsoleBuffer, OptionsBuilder},
        wtf_8::Wtf8String,
    },
    parser::source::Source,
    runtime::{
        Context, ContextBuilder,
        error::BsError,
        eval_result::EvalError,
        snapshot::{self, SnapshotLimits, SnapshotValue},
    },
};

fn run_script_and_call(
    script: &str,
    document: SnapshotValue,
    configure: impl FnOnce(
        brimstone_core::common::options::OptionsBuilder,
    ) -> brimstone_core::common::options::OptionsBuilder,
) -> SnapshotValue {
    let options = configure(OptionsBuilder::new().for_embedding())
        .build()
        .expect("options");
    let mut cx = ContextBuilder::new()
        .set_options(Rc::new(options))
        .build()
        .expect("context");

    let source =
        Source::new_for_string("<embed-test>", Wtf8String::from_str(script)).expect("source");
    cx.evaluate_script(Rc::new(source))
        .unwrap_or_else(|_| panic!("evaluate wrapper"));
    cx.capture_main().unwrap_or_else(|_| panic!("capture main"));

    let realm = cx.initial_realm_ptr();
    let called = cx.with_initial_realm_stack_frame(realm, |mut cx| {
        let argument = snapshot::build(cx, &document).unwrap_or_else(|_| panic!("build argument"));
        cx.call_main(argument)
    });
    let value = called.unwrap_or_else(|_| panic!("call main"));
    let result = snapshot::snapshot(cx, value, SnapshotLimits::default())
        .unwrap_or_else(|_| panic!("snapshot result"));

    cx.drop();
    result
}

fn run_wrapper(document: SnapshotValue) -> SnapshotValue {
    run_script_and_call(
        "function __main__(input) { return [typeof input, input]; }\n",
        document,
        |builder| builder,
    )
}

/// Evaluate a function body with no argument and snapshot its return value.
fn run_query(body: &str) -> SnapshotValue {
    let script = format!("function __main__(input) {{ {body} }}\n");
    run_script_and_call(&script, SnapshotValue::Undefined, |builder| builder)
}

fn typeof_and_value(result: SnapshotValue) -> (String, SnapshotValue) {
    match result {
        SnapshotValue::Array(mut items) => {
            let ty = match items.remove(0) {
                SnapshotValue::String(s) => s,
                other => panic!("expected typeof string, got {other:?}"),
            };
            (ty, items.remove(0))
        }
        other => panic!("expected array result, got {other:?}"),
    }
}

/// M1-F1: a top-level string document must be a string inside `__main__`.
#[test]
fn string_document_is_a_string() {
    let (ty, value) = typeof_and_value(run_wrapper(SnapshotValue::String("hello".into())));
    assert_eq!(ty, "string", "typeof input");
    assert_eq!(value, SnapshotValue::String("hello".into()));
}

/// The full document type matrix from the F1 plan.
#[test]
fn document_type_matrix() {
    let cases: Vec<(SnapshotValue, &str)> = vec![
        (SnapshotValue::String("s".into()), "string"),
        (SnapshotValue::Number(1.0), "number"),
        (SnapshotValue::Bool(true), "boolean"),
        (SnapshotValue::Null, "object"),
        (SnapshotValue::Array(vec![SnapshotValue::Number(1.0)]), "object"),
        (SnapshotValue::Object(vec![("a".into(), SnapshotValue::Number(1.0))]), "object"),
    ];

    for (document, expected_type) in cases {
        let (ty, _) = typeof_and_value(run_wrapper(document.clone()));
        assert_eq!(ty, expected_type, "typeof input for {document:?}");
    }
}

fn caught_name(result: SnapshotValue) -> String {
    match result {
        SnapshotValue::String(s) => s,
        other => panic!("expected a caught error name string, got {other:?}"),
    }
}

/// B2: 100k-deep JSON must be rejected as a catchable RangeError, not crash the process.
#[test]
fn deep_json_parse_is_catchable_range_error() {
    let body = "try { JSON.parse('{\"a\":'.repeat(100000) + '1' + '}'.repeat(100000)); return 'ok'; } catch (e) { return e.name; }";
    assert_eq!(caught_name(run_query(body)), "RangeError");
}

/// B3: a ReDoS pattern must exhaust the step budget as a catchable RangeError.
#[test]
fn redos_regex_is_catchable_range_error() {
    let body =
        "try { /^(a+)+$/.test('a'.repeat(30) + '!'); return 'ok'; } catch (e) { return e.name; }";
    assert_eq!(caught_name(run_query(body)), "RangeError");
}

/// B4: deep JS recursion is bounded by the stack config as a catchable RangeError.
#[test]
fn deep_recursion_is_catchable_range_error() {
    let body = "try { (function f(n) { return f(n + 1); })(0); return 'ok'; } catch (e) { return e.name; }";
    assert_eq!(caught_name(run_query(body)), "RangeError");
}

/// B5/B6: console output is captured in the installed buffer, and a thrown error has a
/// Brimstone-formatted stack (capped at 50 frames) without calling user JS.
#[test]
fn console_capture_and_error_stack() {
    let buffer = Arc::new(Mutex::new(ConsoleBuffer::new(1024)));
    let options = OptionsBuilder::new()
        .for_embedding()
        .console_buffer(Some(buffer.clone()))
        .build()
        .unwrap();
    let mut cx = ContextBuilder::new()
        .set_options(Rc::new(options))
        .build()
        .unwrap();

    let script = "function __main__(input) { console.log('hi', 1); throw new Error('boom'); }\n";
    let source = Source::new_for_string("<console-test>", Wtf8String::from_str(script)).unwrap();
    cx.evaluate_script(Rc::new(source))
        .unwrap_or_else(|_| panic!("evaluate"));
    cx.capture_main().unwrap_or_else(|_| panic!("capture main"));

    let result = cx.with_initial_realm_stack_frame(cx.initial_realm_ptr(), |mut cx| {
        let argument = cx.undefined();
        cx.call_main(argument)
    });

    let logs = cx.drain_console();
    assert_eq!(logs, vec!["hi 1"]);
    assert!(cx.drain_console().is_empty(), "drain must clear the buffer");

    match result {
        Err(EvalError::Value(handle)) => {
            let stack = cx
                .error_stack_string(handle)
                .unwrap_or_else(|_| panic!("stack"));
            assert!(stack.contains("at __main__"), "stack = {stack}");
            assert!(stack.lines().count() <= 50, "stack = {stack}");
        }
        _ => panic!("expected a thrown error"),
    }

    cx.drop();
}

/// B6: console output beyond the cap is truncated with a marker.
#[test]
fn console_capture_is_capped() {
    let buffer = Arc::new(Mutex::new(ConsoleBuffer::new(16)));
    let options = OptionsBuilder::new()
        .for_embedding()
        .console_buffer(Some(buffer.clone()))
        .build()
        .unwrap();
    let mut cx = ContextBuilder::new()
        .set_options(Rc::new(options))
        .build()
        .unwrap();
    let script = "function __main__(input) { console.log('abcdefghijklmnopqrstuvwxyz'); }";
    let source = Source::new_for_string("<console-cap>", Wtf8String::from_str(script)).unwrap();
    cx.evaluate_script(Rc::new(source))
        .unwrap_or_else(|_| panic!("evaluate"));
    cx.capture_main().unwrap_or_else(|_| panic!("capture main"));
    let result = cx.with_initial_realm_stack_frame(cx.initial_realm_ptr(), |mut cx| {
        let argument = cx.undefined();
        cx.call_main(argument)
    });
    assert!(result.is_ok());

    let logs = cx.drain_console();
    assert!(logs.iter().any(|line| line.contains("truncated")), "logs = {logs:?}");
    cx.drop();
}

/// B8: dynamically installed host functions are callable through `__host_call__`.
#[test]
fn host_function_is_callable() {
    let options = OptionsBuilder::new().for_embedding().build().unwrap();
    let mut cx = ContextBuilder::new()
        .set_options(Rc::new(options))
        .build()
        .unwrap();

    cx.install_host_function(
        "echo",
        Box::new(|mut cx: Context, args: &[_]| {
            let wtf8 = args[0].as_string().to_wtf8_string()?;
            let text = String::from_utf8_lossy(wtf8.as_bytes());
            Ok(cx.alloc_string(&text)?.as_value())
        }),
    );

    let script = "function __main__(input) { return __host_call__('echo', 'hi'); }";
    let source = Source::new_for_string("<host-fn>", Wtf8String::from_str(script)).unwrap();
    cx.evaluate_script(Rc::new(source))
        .unwrap_or_else(|_| panic!("evaluate"));
    cx.capture_main().unwrap_or_else(|_| panic!("capture main"));

    let value = cx
        .with_initial_realm_stack_frame(cx.initial_realm_ptr(), |mut cx| {
            let argument = cx.undefined();
            cx.call_main(argument)
        })
        .unwrap_or_else(|_| panic!("call main"));
    let result = snapshot::snapshot(cx, value, SnapshotLimits::default())
        .unwrap_or_else(|_| panic!("snapshot"));
    assert_eq!(result, SnapshotValue::String("hi".into()));
    cx.drop();
}

/// B17: result conversion observes cooperative stop requests every 1024 nodes.
#[test]
fn snapshot_conversion_honors_stop() {
    let options = OptionsBuilder::new().for_embedding().build().unwrap();
    let mut cx = ContextBuilder::new()
        .set_options(Rc::new(options))
        .build()
        .unwrap();

    let script = "function __main__(input) { return new Array(5000).fill(1); }";
    let source = Source::new_for_string("<snapshot-stop>", Wtf8String::from_str(script)).unwrap();
    cx.evaluate_script(Rc::new(source))
        .unwrap_or_else(|_| panic!("evaluate"));
    cx.capture_main().unwrap_or_else(|_| panic!("capture main"));

    let value = cx
        .with_initial_realm_stack_frame(cx.initial_realm_ptr(), |mut cx| {
            let argument = cx.undefined();
            cx.call_main(argument)
        })
        .unwrap_or_else(|_| panic!("call main"));

    // Stop after the call has completed so the conversion itself must observe it.
    cx.vm().control().stop(0);
    let result = snapshot::snapshot(cx, value, SnapshotLimits::default());
    assert!(matches!(result, Err(BsError::Eval(EvalError::Terminated))));
    cx.drop();
}

/// The default limits leave ordinary regexes and JSON untouched.
#[test]
fn ordinary_json_and_regex_still_work() {
    assert_eq!(run_query("return JSON.parse('{\"a\":1}').a;"), SnapshotValue::Number(1.0));
    assert_eq!(run_query("return /a+/.test('baaad');"), SnapshotValue::Bool(true));
}

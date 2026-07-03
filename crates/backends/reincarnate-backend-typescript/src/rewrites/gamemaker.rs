//! GameMaker-specific JsExpr → JsExpr rewrite pass.
//!
//! Resolves GameMaker SystemCall nodes into native JavaScript constructs.
//! Much simpler than Flash — only 8 SystemCall patterns to handle.

use std::collections::{BTreeMap, HashMap};

use reincarnate_core::ir::inst::CastKind;
use reincarnate_core::ir::value::Constant;
use reincarnate_core::ir::{CmpKind, ExternalImport, Type, TypeId, ValueId};
use reincarnate_core::project::ExternalMethodSig;

use crate::emit::{ClassRegistry, RefSets};
use crate::js_ast::{JsExpr, JsFunction, JsStmt};
use crate::rewrites::take_arg;

/// Event method names that have explicit typed declarations on GMLObject.
///
/// GML separates event handlers and instance variables, but TypeScript classes
/// share a single namespace. When a game uses a variable name matching one of
/// these (e.g. `self.create = 10`), the typed method declaration shadows the
/// `[key: string]: any` index signature, causing spurious type errors.
fn is_event_method_name(name: &str) -> bool {
    matches!(
        name,
        "create"
            | "destroy"
            | "draw"
            | "step"
            | "beginstep"
            | "endstep"
            | "drawgui"
            | "mouseenter"
            | "mouseleave"
            | "roomstart"
            | "roomend"
    )
}

/// Strip coerce wrappers from an instance-field target expression.
///
/// GML bytecode emits `Conv.v.i32` before cross-instance field operands,
/// which the backend lowers to `JsExpr::Cast { ty: Int(32), kind: Coerce }`.
/// GML also sometimes widens integer constants to `dyn` before passing them
/// to instance-field syscalls (`coerce i64_const, dyn`), which is a no-op at
/// the printed level but prevents `resolve_instance_target` from seeing the
/// underlying literal.
///
/// Strips `Int(32)` and `Unknown` coerce wrappers; both are printed as the
/// inner expression unchanged and both may hide a constant integer class index.
fn strip_int_coerce(expr: JsExpr) -> JsExpr {
    use reincarnate_core::ir::{CastKind, Type};
    if let JsExpr::Cast {
        expr: inner,
        ty,
        kind: CastKind::Coerce,
    } = expr
    {
        if matches!(ty, Type::Int(32) | Type::Value) {
            return *inner;
        }
        JsExpr::Cast {
            expr: inner,
            ty,
            kind: CastKind::Coerce,
        }
    } else {
        expr
    }
}

/// After stripping an `int()` coerce, resolve a constant integer class index
/// to the class constructor (`JsExpr::Var(obj_name)`). GMLObject instance
/// references and already-resolved names pass through unchanged.
fn resolve_instance_target(expr: JsExpr, object_names: &[String]) -> JsExpr {
    if let JsExpr::Literal(reincarnate_core::ir::value::Constant::Int(idx)) = expr {
        if idx >= 0 {
            if let Some(obj_name) = object_names.get(idx as usize) {
                return JsExpr::Var(obj_name.clone());
            }
        }
    }
    expr
}

/// Try to build `ObjName.instances[0]!` for a constant object reference.
///
/// Returns `Some(instances_0_expr)` when `expr` is:
/// - A string literal naming an object class, OR
/// - A constant integer class index resolvable via `object_names`.
///
/// Returns `None` for dynamic references (variables, non-integer constants,
/// out-of-range indices) — these must fall back to `getInstanceField`.
fn get_instances_0(
    expr: &JsExpr,
    object_names: &[String],
    name_map: &HashMap<String, String>,
) -> Option<JsExpr> {
    match expr {
        JsExpr::Literal(Constant::String(s)) => {
            let ts_name = name_map
                .get(s.as_str())
                .cloned()
                .unwrap_or_else(|| s.clone());
            Some(instances_0(ts_name))
        }
        JsExpr::Literal(Constant::Int(idx)) if *idx >= 0 => object_names
            .get(*idx as usize)
            .map(|name| instances_0(name.clone())),
        _ => None,
    }
}

fn instances_0(obj_name: String) -> JsExpr {
    JsExpr::NonNull(Box::new(JsExpr::Index {
        collection: Box::new(JsExpr::Field {
            object: Box::new(JsExpr::Var(obj_name)),
            field: "instances".into(),
        }),
        index: Box::new(JsExpr::Literal(Constant::Int(0))),
    }))
}

/// Returns the stateful runtime names that a direct `Op::Call` rewrite will
/// introduce, if any.  Used by import generation to ensure these names are
/// destructured from `this._rt` before the rewrite pass runs.
///
/// `@@Global@@` and `@@Other@@` now rewrite directly to `_rt.global` /
/// `_rt.other` (or `this._rt.global` / `this._rt.other` in class context) —
/// qualified member expressions, not bare identifiers.  No destructured names
/// are introduced.
pub fn rewrite_introduced_direct_calls(_func_name: &str) -> &'static [&'static str] {
    &[]
}

/// Returns the bare function names that a SystemCall rewrite will introduce,
/// if any.  Used by import generation to emit the correct imports before
/// the rewrite pass runs.  Kept for Flash/Twine callers that have the
/// (system, method) pair from Op::SystemCall.
pub fn rewrite_introduced_calls(_system: &str, _method: &str) -> &'static [&'static str] {
    // GameMaker.Instance/Global syscalls rewrite to rt-qualified member calls
    // (`_rt.getInstanceField(...)`, `this._rt.setOtherField(...)`, etc.) — these
    // are stateful `GameRuntime` closures (Law 5), reached through the runtime
    // handle, not bare free-function identifiers. They introduce no free-function
    // imports.
    &[]
}

/// Returns the bare function names that a GML plain-call rewrite will introduce,
/// given the full dotted call name (e.g. `"GameMaker.Instance.getField"`).
///
/// Used by import generation for `Op::Call` nodes that are GML syscalls registered
/// as plain runtime functions (Phase 2+).  Arg 0 of these calls is always `_rt`.
pub fn rewrite_introduced_calls_by_name(_name: &str) -> &'static [&'static str] {
    // See `rewrite_introduced_calls`: these syscalls rewrite to rt-qualified
    // member calls, not bare free-function identifiers, so they introduce no
    // free-function imports.
    &[]
}

/// Extract the full dotted call name from a nested `JsExpr::Field` callee.
///
/// `JsExpr::Call { callee: Field { object: Field { object: Var("GameMaker"), field: "Instance" },
/// field: "getField" }, args }` → `"GameMaker.Instance.getField"`.
///
/// Returns `None` if the callee is not a pure dotted-field-on-Var chain.
fn extract_dotted_callee_name(callee: &JsExpr) -> Option<String> {
    fn collect(expr: &JsExpr, parts: &mut Vec<String>) -> bool {
        match expr {
            JsExpr::Var(name) => {
                parts.push(name.clone());
                true
            }
            JsExpr::Field { object, field } => {
                if collect(object, parts) {
                    parts.push(field.clone());
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
    let mut parts = Vec::new();
    if collect(callee, &mut parts) && parts.len() >= 2 {
        Some(parts.join("."))
    } else {
        None
    }
}

/// Returns true if `callee` is a dotted-field path for the given full name.
fn callee_is(callee: &JsExpr, name: &str) -> bool {
    extract_dotted_callee_name(callee).as_deref() == Some(name)
}

/// Recognize the stateful-lowered GML runtime-call shape and split its dotted
/// FuncId name into `(system, method)`.
///
/// `lower::lower_call` lowers a call to a registered runtime function (whose IR
/// signature begins with the `_rt` handle) into
/// `Call { callee: Field { object: <rt>, field: "GameMaker.Instance.getField" }, args }`
/// — the runtime handle becomes the JS receiver and the dotted FuncId name
/// becomes a single field. Because the field is the full dotted name rather than
/// a `GameMaker.Instance.getField` Var-chain, `try_rewrite_system_call` (which
/// matches `JsExpr::SystemCall`) never reaches it, leaving the call as a
/// string-keyed property access (`_rt["GameMaker.Instance.getField"](...)`).
///
/// Returns `Some((system, method))` when `callee` is `Field { field: "A.b.c" }`
/// whose field is a dotted name (the receiver — `_rt` / `this._rt` — is
/// irrelevant; the native rewrite re-derives its own receiver). `(system,
/// method)` is the name split at the last `.`.
fn stateful_dotted_system_method(callee: &JsExpr) -> Option<(&str, &str)> {
    let JsExpr::Field { field, .. } = callee else {
        return None;
    };
    let dot = field.rfind('.')?;
    Some((&field[..dot], &field[dot + 1..]))
}

/// Rewrite a function's body, resolving GameMaker SystemCalls.
///
/// `event_name` is the method name of the enclosing event handler (e.g.
/// `"create"`, `"step"`, `"draw"`), used to rewrite `event_inherited()` to
/// `super.eventName()`.  Pass `None` for free functions.
pub fn rewrite_gamemaker_function(
    mut func: JsFunction,
    sprite_names: &[String],
    object_names: &[String],
    closure_bodies: &HashMap<String, JsFunction>,
    event_name: Option<&str>,
    name_map: &HashMap<String, String>,
    global_state_type_id: Option<TypeId>,
) -> JsFunction {
    rewrite_stmts(
        &mut func.body,
        sprite_names,
        object_names,
        closure_bodies,
        event_name,
        name_map,
        global_state_type_id,
    );
    func
}

fn rewrite_stmts(
    stmts: &mut Vec<JsStmt>,
    sprite_names: &[String],
    object_names: &[String],
    closure_bodies: &HashMap<String, JsFunction>,
    event_name: Option<&str>,
    name_map: &HashMap<String, String>,
    global_state_type_id: Option<TypeId>,
) {
    for stmt in stmts.iter_mut() {
        rewrite_stmt(
            stmt,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        );
    }
    // Remove no-op expression statements produced by GML internal function rewrites:
    //   @@Global@@() → global; (standalone call is a no-op)
    //   @@Other@@()  → other;  (standalone call is a no-op)
    stmts.retain(|s| !is_gml_noop_stmt(s));
}

/// Returns true for expression-statement no-ops introduced by rewriting GML
/// internal built-in calls (`@@Global@@`, `@@Other@@`).
///
/// Both rewrite to `_rt.global` / `_rt.other` (or `this._rt.global` /
/// `this._rt.other`), so we match on Field expressions with field `"global"` or
/// `"other"` whose object is `_rt` or `this._rt`.
fn is_gml_noop_stmt(stmt: &JsStmt) -> bool {
    let JsStmt::Expr(expr) = stmt else {
        return false;
    };
    let JsExpr::Field { object, field } = expr else {
        return false;
    };
    if field != "global" && field != "other" {
        return false;
    }
    // _rt.global / _rt.other
    if matches!(object.as_ref(), JsExpr::Var(v) if v == "_rt") {
        return true;
    }
    // this._rt.global / this._rt.other
    if let JsExpr::Field {
        object: inner_obj,
        field: inner_field,
    } = object.as_ref()
    {
        if inner_field == "_rt" && matches!(inner_obj.as_ref(), JsExpr::This) {
            return true;
        }
    }
    false
}

fn rewrite_stmt(
    stmt: &mut JsStmt,
    sprite_names: &[String],
    object_names: &[String],
    closure_bodies: &HashMap<String, JsFunction>,
    event_name: Option<&str>,
    name_map: &HashMap<String, String>,
    global_state_type_id: Option<TypeId>,
) {
    match stmt {
        JsStmt::VarDecl { init, .. } => {
            if let Some(expr) = init {
                rewrite_expr(
                    expr,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsStmt::Assign { target, value } => {
            rewrite_expr(
                target,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                value,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            try_resolve_sprite_assign(target, value, sprite_names);
            // GML auto-creates arrays on first indexed write; TypeScript requires
            // explicit initialization.  When the target is `this.field[i]`, wrap
            // the collection as `(this.field ??= [])` so the field is initialized
            // to an empty array if it is undefined at the time of the write.
            try_null_coalesce_self_array_collection(target);
        }
        JsStmt::CompoundAssign { target, value, .. } => {
            rewrite_expr(
                target,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                value,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            try_resolve_sprite_assign(target, value, sprite_names);
            try_null_coalesce_self_array_collection(target);
        }
        JsStmt::Expr(e) => {
            // Intercept global set with constant key → global.field = val
            // After Phase 2: GML syscalls are plain JsExpr::Call with _rt as args[0].
            if let JsExpr::Call { callee, args } = e {
                if callee_is(callee, "GameMaker.Global.set")
                    && args.len() == 3
                    && matches!(&args[1], JsExpr::Literal(Constant::String(_)))
                {
                    let mut args = std::mem::take(args);
                    let mut val = take_arg(&mut args, "GameMaker.Global.set");
                    let name_expr = take_arg(&mut args, "GameMaker.Global.set");
                    // _rt is args[0], already popped name and val — ignore _rt.
                    let JsExpr::Literal(Constant::String(field_name)) = name_expr else {
                        unreachable!()
                    };
                    rewrite_expr(
                        &mut val,
                        sprite_names,
                        object_names,
                        closure_bodies,
                        event_name,
                        name_map,
                        global_state_type_id,
                    );
                    // Wrap `_rt.global` in a `GameGlobalState` cast so that
                    // `_rt.global.fieldName = val` becomes
                    // `(_rt.global as GameGlobalState).fieldName = val`,
                    // resolving TS2339 for game-specific global variables.
                    let global_expr = cast_global_for_field(global_state_type_id, event_name);
                    *stmt = JsStmt::Assign {
                        target: JsExpr::Field {
                            object: Box::new(global_expr),
                            field: field_name,
                        },
                        value: val,
                    };
                    return;
                }
            }
            // Intercept setOn with named object at statement level → direct assignment.
            // After Phase 2: args are [_rt, obj, field, ...], so len is +1 vs old.
            if let JsExpr::Call { callee, args } = e {
                if callee_is(callee, "GameMaker.Instance.setOn")
                    && matches!(&args[1], JsExpr::Literal(Constant::String(_)))
                    && matches!(&args[2], JsExpr::Literal(Constant::String(_)))
                {
                    match args.len() {
                        // setOn(_rt, obj, field, value) → Obj.instances[0].field = value
                        4 => {
                            let mut args = std::mem::take(args);
                            let val = take_arg(&mut args, "GameMaker.Instance.setOn");
                            let field = take_arg(&mut args, "GameMaker.Instance.setOn");
                            let obj_name_expr = take_arg(&mut args, "GameMaker.Instance.setOn");
                            // _rt is args[0], already popped obj, field, val — ignore _rt.
                            let JsExpr::Literal(Constant::String(obj_name)) = obj_name_expr else {
                                unreachable!()
                            };
                            let JsExpr::Literal(Constant::String(field_name)) = field else {
                                unreachable!()
                            };
                            let ts_obj_name =
                                name_map.get(obj_name.as_str()).cloned().unwrap_or(obj_name);
                            let target = JsExpr::Field {
                                object: Box::new(instances_0(ts_obj_name)),
                                field: field_name,
                            };
                            let mut val = val;
                            rewrite_expr(
                                &mut val,
                                sprite_names,
                                object_names,
                                closure_bodies,
                                event_name,
                                name_map,
                                global_state_type_id,
                            );
                            *stmt = JsStmt::Assign { target, value: val };
                            return;
                        }
                        // setOn(_rt, obj, field, index, value) → Obj.instances[0]!.field[index] = value
                        5 => {
                            let mut args = std::mem::take(args);
                            let val = take_arg(&mut args, "GameMaker.Instance.setOn");
                            let mut index_expr = take_arg(&mut args, "GameMaker.Instance.setOn");
                            let field = take_arg(&mut args, "GameMaker.Instance.setOn");
                            let obj_name_expr = take_arg(&mut args, "GameMaker.Instance.setOn");
                            // _rt is args[0], already popped obj, field, index, val — ignore _rt.
                            let JsExpr::Literal(Constant::String(obj_name)) = obj_name_expr else {
                                unreachable!()
                            };
                            let JsExpr::Literal(Constant::String(field_name)) = field else {
                                unreachable!()
                            };
                            let ts_obj_name =
                                name_map.get(obj_name.as_str()).cloned().unwrap_or(obj_name);
                            rewrite_expr(
                                &mut index_expr,
                                sprite_names,
                                object_names,
                                closure_bodies,
                                event_name,
                                name_map,
                                global_state_type_id,
                            );
                            let target = JsExpr::Index {
                                collection: Box::new(JsExpr::Field {
                                    object: Box::new(instances_0(ts_obj_name)),
                                    field: field_name,
                                }),
                                index: Box::new(index_expr),
                            };
                            let mut val = val;
                            rewrite_expr(
                                &mut val,
                                sprite_names,
                                object_names,
                                closure_bodies,
                                event_name,
                                name_map,
                                global_state_type_id,
                            );
                            *stmt = JsStmt::Assign { target, value: val };
                            return;
                        }
                        _ => {}
                    }
                }
            }
            rewrite_expr(
                e,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsStmt::Return(Some(e)) => rewrite_expr(
            e,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsStmt::Return(None) => {}
        JsStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            rewrite_expr(
                cond,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_stmts(
                then_body,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_stmts(
                else_body,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsStmt::While { cond, body } => {
            rewrite_expr(
                cond,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_stmts(
                body,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsStmt::For {
            init,
            cond,
            update,
            body,
        } => {
            rewrite_stmts(
                init,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                cond,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_stmts(
                update,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_stmts(
                body,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsStmt::Loop { body } => {
            rewrite_stmts(
                body,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsStmt::ForOf { iterable, body, .. } => {
            rewrite_expr(
                iterable,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_stmts(
                body,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsStmt::Throw(e) => rewrite_expr(
            e,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsStmt::Dispatch { blocks, .. } => {
            for (_, stmts) in blocks {
                rewrite_stmts(
                    stmts,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsStmt::Switch {
            value,
            cases,
            default_body,
        } => {
            rewrite_expr(
                value,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            for (_, stmts) in cases {
                rewrite_stmts(
                    stmts,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
            rewrite_stmts(
                default_body,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsStmt::Break | JsStmt::Continue | JsStmt::LabeledBreak { .. } => {}
    }
}

/// If `target` is a `Field { field: "sprite_index", .. }` and `value` is a
/// `Literal(Int(idx))` within range, replace the value with `Sprites.Name`.
fn try_resolve_sprite_assign(target: &JsExpr, value: &mut JsExpr, sprite_names: &[String]) {
    if sprite_names.is_empty() {
        return;
    }
    let is_sprite_field = matches!(target, JsExpr::Field { field, .. } if field == "sprite_index");
    if !is_sprite_field {
        return;
    }
    if let JsExpr::Literal(Constant::Int(idx)) = value {
        let idx = *idx as usize;
        if let Some(name) = sprite_names.get(idx) {
            let sprites = Box::new(JsExpr::Var("Sprites".into()));
            *value = if crate::ast_printer::is_valid_js_ident(name) {
                JsExpr::Field {
                    object: sprites,
                    field: name.clone(),
                }
            } else {
                JsExpr::Index {
                    collection: sprites,
                    index: Box::new(JsExpr::Literal(Constant::String(name.clone()))),
                }
            };
        }
    }
}

/// If `target` is `this.field[i]`, wrap `collection` as `(this.field ??= [])`.
///
/// GML auto-creates arrays on first indexed write: `self.arr[0] = v` works
/// even when `arr` is undefined.  TypeScript requires the field to already be
/// an array; reading `undefined[0]` throws at runtime.  We rewrite the index
/// target so that the field is lazily initialized to `[]` on the first write.
///
/// The rewrite turns `this.field[i] = v` into `(this.field ??= [])[i] = v`,
/// matching GML semantics without requiring a separate initializer statement.
///
/// Only applies when:
/// - The target is an `Index` expression.
/// - The collection of that index is a `Field` on `This` (not an arbitrary expr).
/// - The collection has not already been wrapped (idempotent guard).
fn try_null_coalesce_self_array_collection(target: &mut JsExpr) {
    let JsExpr::Index { collection, .. } = target else {
        return;
    };
    // Already wrapped — idempotent.
    if matches!(collection.as_ref(), JsExpr::NullCoalesceAssign { .. }) {
        return;
    }
    // Only rewrite direct `this.field` access, not arbitrary sub-expressions.
    if !matches!(collection.as_ref(), JsExpr::Field { object, .. } if matches!(object.as_ref(), JsExpr::This))
    {
        return;
    }
    let field_expr = std::mem::replace(
        collection,
        Box::new(JsExpr::Literal(reincarnate_core::ir::value::Constant::Int(
            0,
        ))),
    );
    *collection = Box::new(JsExpr::NullCoalesceAssign {
        target: field_expr,
        value: Box::new(JsExpr::ArrayInit(vec![])),
    });
}

/// Build `_rt.field` (free-function context) or `this._rt.field` (class-method context).
fn rt_field(field: &str, in_class: bool) -> JsExpr {
    let rt = if in_class {
        JsExpr::Field {
            object: Box::new(JsExpr::This),
            field: "_rt".into(),
        }
    } else {
        JsExpr::Var("_rt".into())
    };
    JsExpr::Field {
        object: Box::new(rt),
        field: field.into(),
    }
}

/// Build a call to a `GameRuntime` instance method: `_rt.method(args)` in
/// free-function context (`_init.ts` scripts receive `_rt` as a parameter) or
/// `this._rt.method(args)` in class-method context (event handlers).
///
/// These runtime methods are stateful closures on the runtime instance (Law 5),
/// not free functions — emitting a bare `method(args)` identifier produces an
/// unresolved name (TS2304). `in_class` is `event_name.is_some()`, matching the
/// receiver-selection used by `rt_field` / `cast_global_for_field`.
fn rt_call(method: &str, args: Vec<JsExpr>, in_class: bool) -> JsExpr {
    JsExpr::Call {
        callee: Box::new(rt_field(method, in_class)),
        args,
    }
}

fn rewrite_expr(
    expr: &mut JsExpr,
    sprite_names: &[String],
    object_names: &[String],
    closure_bodies: &HashMap<String, JsFunction>,
    event_name: Option<&str>,
    name_map: &HashMap<String, String>,
    global_state_type_id: Option<TypeId>,
) {
    // Rename raw GML object names to their disambiguated TypeScript identifiers.
    if let JsExpr::Var(name) = expr {
        if let Some(ts_name) = name_map.get(name.as_str()) {
            if ts_name != name {
                *name = ts_name.clone();
                return;
            }
        }
    }

    // Early-exit: rewrite GML internal built-in calls before child recursion.
    // These replace the whole expression so we handle arg recursion manually.
    //
    // Note: JsExpr::Var stores the original GML name with `@@` delimiters.
    // `sanitize_ident` is only applied at print time (ast_printer.rs), so we
    // match against the raw form here.
    if let JsExpr::Call { callee, args } = expr {
        if let JsExpr::Var(name) = callee.as_ref() {
            match name.as_str() {
                // @@NewGMLArray@@(v0, v1, ...) → [v0, v1, ...]
                "@@NewGMLArray@@" => {
                    for arg in args.iter_mut() {
                        rewrite_expr(
                            arg,
                            sprite_names,
                            object_names,
                            closure_bodies,
                            event_name,
                            name_map,
                            global_state_type_id,
                        );
                    }
                    *expr = JsExpr::ArrayInit(std::mem::take(args));
                    return;
                }
                // @@NewGMLObject@@() → new GMLObject()
                // Anonymous GML structs (GMS2.3+) are GMLObject instances — not room
                // instances, but the same base type. `new GMLObject()` gives the correct
                // TypeScript type and satisfies GMLObject | typeof GMLObject parameters.
                "@@NewGMLObject@@" => {
                    *expr = JsExpr::New {
                        callee: Box::new(JsExpr::Var("GMLObject".into())),
                        args: vec![],
                    };
                    return;
                }
                // @@Global@@() → _rt.global  (or this._rt.global in class context)
                "@@Global@@" if args.is_empty() => {
                    *expr = rt_field("global", event_name.is_some());
                    return;
                }
                // @@Other@@() → _rt.other  (or this._rt.other in class context)
                "@@Other@@" if args.is_empty() => {
                    *expr = rt_field("other", event_name.is_some());
                    return;
                }
                // event_inherited() → super.eventName(args...)
                // Collision events have an `other` param that must be forwarded.
                "event_inherited" => {
                    if let Some(method) = event_name {
                        let super_args = if method.starts_with("collision") {
                            vec![JsExpr::Var("other".into())]
                        } else {
                            vec![]
                        };
                        *expr = JsExpr::SuperMethodCall {
                            method: method.to_string(),
                            args: super_args,
                        };
                        return;
                    }
                }
                // array_length(arr) → arr.length
                "array_length" if args.len() == 1 => {
                    let mut arr = args.remove(0);
                    rewrite_expr(
                        &mut arr,
                        sprite_names,
                        object_names,
                        closure_bodies,
                        event_name,
                        name_map,
                        global_state_type_id,
                    );
                    *expr = JsExpr::Field {
                        object: Box::new(arr),
                        field: "length".into(),
                    };
                    return;
                }
                // array_push(arr, v0, v1, ...) → arr.push(v0, v1, ...)
                // When arr is an ArrayInit literal (e.g. from inlined @@NewGMLArray@@),
                // fold the items in directly: array_push([], x) → [x].
                // This avoids `[].push(x)` where TypeScript infers [] as never[].
                "array_push" if !args.is_empty() => {
                    for arg in args.iter_mut() {
                        rewrite_expr(
                            arg,
                            sprite_names,
                            object_names,
                            closure_bodies,
                            event_name,
                            name_map,
                            global_state_type_id,
                        );
                    }
                    let mut args = std::mem::take(args);
                    let arr = args.remove(0);
                    if let JsExpr::ArrayInit(mut existing) = arr {
                        existing.extend(args);
                        *expr = JsExpr::ArrayInit(existing);
                    } else {
                        *expr = JsExpr::Call {
                            callee: Box::new(JsExpr::Field {
                                object: Box::new(arr),
                                field: "push".into(),
                            }),
                            args,
                        };
                    }
                    return;
                }
                // show_debug_message(msg) → console.log(msg)
                "show_debug_message" => {
                    for arg in args.iter_mut() {
                        rewrite_expr(
                            arg,
                            sprite_names,
                            object_names,
                            closure_bodies,
                            event_name,
                            name_map,
                            global_state_type_id,
                        );
                    }
                    *expr = JsExpr::Call {
                        callee: Box::new(JsExpr::Field {
                            object: Box::new(JsExpr::Var("console".into())),
                            field: "log".into(),
                        }),
                        args: std::mem::take(args),
                    };
                    return;
                }
                // is_struct(val) → (typeof (val) === "object" && (val) !== null)
                "is_struct" if args.len() == 1 => {
                    let mut val = args.remove(0);
                    rewrite_expr(
                        &mut val,
                        sprite_names,
                        object_names,
                        closure_bodies,
                        event_name,
                        name_map,
                        global_state_type_id,
                    );
                    let val_box = Box::new(val);
                    *expr = JsExpr::LogicalAnd {
                        lhs: Box::new(JsExpr::Cmp {
                            kind: CmpKind::Eq,
                            lhs: Box::new(JsExpr::TypeOf(val_box.clone())),
                            rhs: Box::new(JsExpr::Literal(Constant::String("object".into()))),
                        }),
                        rhs: Box::new(JsExpr::Cmp {
                            kind: CmpKind::Ne,
                            lhs: val_box,
                            rhs: Box::new(JsExpr::Literal(Constant::Null)),
                        }),
                    };
                    return;
                }
                _ => {}
            }
        }
    }
    // Rewrite bare variable references for GML built-ins used as values
    // (not in function-call position, e.g. `this.altDestinationObject = @@Global@@`).
    if let JsExpr::Var(name) = expr {
        match name.as_str() {
            "@@Global@@" => {
                *expr = rt_field("global", event_name.is_some());
                return;
            }
            "@@Other@@" => {
                *expr = rt_field("other", event_name.is_some());
                return;
            }
            _ => {}
        }
    }

    // Normal path: recurse into children first, then attempt SystemCall resolution.
    rewrite_expr_children(
        expr,
        sprite_names,
        object_names,
        closure_bodies,
        event_name,
        name_map,
        global_state_type_id,
    );

    // Then, attempt to resolve SystemCall patterns (Flash/Twine) or GML plain-Call
    // patterns (Phase 2+: GML intrinsics are plain Op::Call with dotted names).
    let replacement = match expr {
        JsExpr::SystemCall {
            system,
            method,
            args,
        } => try_rewrite_system_call(
            system,
            method,
            args,
            object_names,
            closure_bodies,
            name_map,
            global_state_type_id,
            event_name,
        ),
        // Stateful-lowered GML runtime call:
        //   Call { callee: Field { object: _rt, field: "GameMaker.Instance.getField" }, args }
        // Split the dotted FuncId name and dispatch to the same native rewrite as
        // the SystemCall path. The `_rt` receiver is discarded — the native forms
        // (`global.name`, `ObjName.instances[0]!.field`, `getInstanceField(...)`,
        // etc.) re-derive their own receiver. This removes the string-keyed
        // `_rt["GameMaker.Instance.getField"](...)` dispatch (TS7053) and emits the
        // direct method/field access a hand port would write.
        JsExpr::Call { callee, args } => match stateful_dotted_system_method(callee) {
            Some((system, method)) => {
                let (system, method) = (system.to_string(), method.to_string());
                try_rewrite_system_call(
                    &system,
                    &method,
                    args,
                    object_names,
                    closure_bodies,
                    name_map,
                    global_state_type_id,
                    event_name,
                )
            }
            None => None,
        },
        _ => None,
    };

    if let Some(new_expr) = replacement {
        *expr = new_expr;
        // Re-recurse into the replacement's children so that SystemCalls
        // inside an inlined closure body (ArrowFunction) are also rewritten.
        rewrite_expr_children(
            expr,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        );
    }
}

fn rewrite_expr_children(
    expr: &mut JsExpr,
    sprite_names: &[String],
    object_names: &[String],
    closure_bodies: &HashMap<String, JsFunction>,
    event_name: Option<&str>,
    name_map: &HashMap<String, String>,
    global_state_type_id: Option<TypeId>,
) {
    match expr {
        JsExpr::Binary { lhs, rhs, .. }
        | JsExpr::Cmp { lhs, rhs, .. }
        | JsExpr::LooseEq { lhs, rhs }
        | JsExpr::LooseNe { lhs, rhs } => {
            rewrite_expr(
                lhs,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                rhs,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsExpr::LogicalOr { lhs, rhs } | JsExpr::LogicalAnd { lhs, rhs } => {
            rewrite_expr(
                lhs,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                rhs,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsExpr::Unary { expr: inner, .. } => rewrite_expr(
            inner,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsExpr::Not(inner) | JsExpr::PostIncrement(inner) | JsExpr::Spread(inner) => rewrite_expr(
            inner,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsExpr::Field { object, field } => {
            // GML separates event handlers and instance variables into different
            // namespaces, but TypeScript doesn't. When a game uses a variable name
            // that matches an event method (e.g. `self.create = 10`), the typed
            // method declaration on GMLObject shadows the `[key: string]: any`
            // index signature, causing type errors. Wrap such accesses with
            // `(this as any).field` so the index signature applies.
            if matches!(object.as_ref(), JsExpr::This) && is_event_method_name(field) {
                *object = Box::new(JsExpr::Cast {
                    expr: Box::new(JsExpr::This),
                    ty: Type::Value,
                    kind: CastKind::NullableCoerce,
                });
            } else {
                rewrite_expr(
                    object,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsExpr::Index { collection, index } => {
            rewrite_expr(
                collection,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                index,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsExpr::Call { callee, args } => {
            rewrite_expr(
                callee,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            for arg in args {
                rewrite_expr(
                    arg,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsExpr::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            rewrite_expr(
                cond,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                then_val,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                else_val,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsExpr::ArrayInit(items) | JsExpr::TupleInit(items) => {
            for item in items {
                rewrite_expr(
                    item,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsExpr::ObjectInit(fields) => {
            for (_, val) in fields {
                rewrite_expr(
                    val,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsExpr::New { callee, args } => {
            rewrite_expr(
                callee,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            for arg in args {
                rewrite_expr(
                    arg,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsExpr::TypeOf(inner) => rewrite_expr(
            inner,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsExpr::In { key, object } => {
            rewrite_expr(
                key,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                object,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsExpr::Delete { object, key } => {
            rewrite_expr(
                object,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                key,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsExpr::Cast { expr: inner, .. } => rewrite_expr(
            inner,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsExpr::TypeCheck {
            expr: inner,
            ty,
            use_instanceof,
        } => {
            // GML: all objects are class instances — use `instanceof` for Instance checks.
            if matches!(ty, reincarnate_core::ir::ty::Type::Instance(_)) {
                *use_instanceof = true;
            }
            rewrite_expr(
                inner,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            )
        }
        // Arrow functions are closures: `event_inherited` inside a closure is
        // not meaningful (closures don't have a "current event"), so pass None.
        JsExpr::ArrowFunction { body, .. } => rewrite_stmts(
            body,
            sprite_names,
            object_names,
            closure_bodies,
            None,
            name_map,
            global_state_type_id,
        ),
        JsExpr::SuperCall(args) | JsExpr::SuperMethodCall { args, .. } => {
            for arg in args {
                rewrite_expr(
                    arg,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsExpr::SuperGet(_) => {}
        JsExpr::SuperSet { value, .. } => rewrite_expr(
            value,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsExpr::GeneratorCreate { args, .. } => {
            for arg in args {
                rewrite_expr(
                    arg,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsExpr::GeneratorResume(inner) => rewrite_expr(
            inner,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsExpr::Yield(inner) => {
            if let Some(e) = inner {
                rewrite_expr(
                    e,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        JsExpr::NonNull(inner) => rewrite_expr(
            inner,
            sprite_names,
            object_names,
            closure_bodies,
            event_name,
            name_map,
            global_state_type_id,
        ),
        JsExpr::NullCoalesceAssign { target, value } => {
            rewrite_expr(
                target,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                value,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsExpr::Assign { lhs, rhs } => {
            rewrite_expr(
                lhs,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
            rewrite_expr(
                rhs,
                sprite_names,
                object_names,
                closure_bodies,
                event_name,
                name_map,
                global_state_type_id,
            );
        }
        JsExpr::Activation => {}
        JsExpr::SystemCall { args, .. } => {
            for arg in args {
                rewrite_expr(
                    arg,
                    sprite_names,
                    object_names,
                    closure_bodies,
                    event_name,
                    name_map,
                    global_state_type_id,
                );
            }
        }
        // Leaf nodes — nothing to recurse into.
        JsExpr::Literal(_) | JsExpr::Var(_) | JsExpr::Regex(_) | JsExpr::This => {}
    }
}

/// Build the expression used as the object in `global.fieldName` accesses.
///
/// Returns `_rt.global` (free-function context) or `this._rt.global` (class-method
/// context).  When `global_state_type_id` is `Some`, wraps the expression in a
/// `Coerce` cast to `GameGlobalState` so that constant-key accesses emit
/// `(_rt.global as GameGlobalState).fieldName`, resolving TS2339 errors for
/// game-specific global variables.
fn cast_global_for_field(global_state_type_id: Option<TypeId>, event_name: Option<&str>) -> JsExpr {
    let global_expr = rt_field("global", event_name.is_some());
    if let Some(id) = global_state_type_id {
        JsExpr::Cast {
            expr: Box::new(global_expr),
            ty: Type::Instance(id),
            kind: CastKind::Coerce,
        }
    } else {
        global_expr
    }
}

/// Try to rewrite a GameMaker SystemCall into a native JS expression.
#[allow(clippy::too_many_arguments)]
fn try_rewrite_system_call(
    system: &str,
    method: &str,
    args: &mut Vec<JsExpr>,
    object_names: &[String],
    closure_bodies: &HashMap<String, JsFunction>,
    name_map: &HashMap<String, String>,
    global_state_type_id: Option<TypeId>,
    event_name: Option<&str>,
) -> Option<JsExpr> {
    match (system, method) {
        // SugarCube.Engine.closure(name[, cap0, cap1, ...]) → arrow function or IIFE
        //
        // This is the IR encoding emitted by lower.rs for Op::MakeClosure.
        // For GML with-bodies: no-capture case produces a plain arrow function;
        // the capture case produces ((cap0, ...) => (_self) => body)(cap_val0, ...).
        ("SugarCube.Engine", "closure") if !args.is_empty() => {
            let JsExpr::Literal(Constant::String(ref name)) = args[0] else {
                return None;
            };
            let short_name = name.rsplit("::").next().unwrap_or(name.as_str());
            let closure_func = closure_bodies.get(short_name).cloned()?;
            let n_cap = closure_func.num_capture_params;
            let n_total = closure_func.params.len();
            let n_reg = n_total.saturating_sub(n_cap);
            if n_cap == 0 || args.len() == 1 {
                // No captures: plain arrow function.
                return Some(JsExpr::ArrowFunction {
                    params: closure_func.params,
                    return_ty: closure_func.return_ty,
                    body: closure_func.body,
                    has_rest_param: closure_func.has_rest_param,
                    cast_as: None,
                    infer_param_types: false,
                });
            }

            // Check if all captures are ByRef (future: GML ByRef capture
            // support).  Currently all GML with-body captures are ByValue,
            // so this branch is not taken.
            let modes = &closure_func.capture_modes;
            let all_byref = n_cap > 0
                && modes.len() == n_cap
                && modes
                    .iter()
                    .all(|m| *m == reincarnate_core::ir::CaptureMode::ByRef);

            if all_byref {
                // All captures are ByRef: plain arrow function.
                // The capture params are stripped — the closure body references
                // outer `let` variables directly via JS closure semantics, so
                // mutations inside the closure propagate back to the outer scope.
                let mut all_params = closure_func.params;
                all_params.truncate(n_reg);
                args.drain(1..);
                return Some(JsExpr::ArrowFunction {
                    params: all_params,
                    return_ty: closure_func.return_ty,
                    body: closure_func.body,
                    has_rest_param: closure_func.has_rest_param,
                    cast_as: None,
                    infer_param_types: false,
                });
            }

            // IIFE for ByValue captures: ((cap0, ...) => (_self) => body)(cap_val0, ...)
            let mut all_params = closure_func.params;
            // Preserve the capture param types — they are set by the frontend
            // (e.g., `_other: GMLObject`) and stripping them to `unknown` would
            // cause TS18046 errors in the closure body.
            let cap_params: Vec<(String, Type)> = all_params.split_off(n_reg);
            let reg_params = all_params;
            let cap_vals: Vec<JsExpr> = args.drain(1..).collect();
            // The outer IIFE returns the inner closure. Compute the correct return
            // type so TypeScript knows the IIFE produces the right callback type
            // (e.g. `(p0: GMLObject) => void` for withInstances callbacks).
            let iife_return_ty = Type::Function(Box::new(reincarnate_core::ir::ty::FunctionSig {
                params: reg_params.iter().map(|(_, t)| t.clone()).collect(),
                return_ty: closure_func.return_ty.clone(),
                defaults: Vec::new(),
                has_rest_param: closure_func.has_rest_param,
                param_lower_bounds: vec![],
            }));
            Some(JsExpr::Call {
                callee: Box::new(JsExpr::ArrowFunction {
                    params: cap_params,
                    return_ty: iife_return_ty,
                    body: vec![crate::js_ast::JsStmt::Return(Some(JsExpr::ArrowFunction {
                        params: reg_params,
                        return_ty: closure_func.return_ty,
                        body: closure_func.body,
                        has_rest_param: closure_func.has_rest_param,
                        cast_as: None,
                        infer_param_types: false,
                    }))],
                    has_rest_param: false,
                    cast_as: None,
                    infer_param_types: false,
                }),
                args: cap_vals,
            })
        }
        // GameMaker.Instance.getInstances() → _rt.getInstances()
        ("GameMaker.Instance", "getInstances") if args.is_empty() => {
            Some(rt_call("getInstances", vec![], event_name.is_some()))
        }
        // GameMaker.Instance.withInstances(target, closure) → _rt.withInstances(target, closure)
        //
        // At this point the closure arg has already been rewritten (children-first)
        // from SugarCube.Engine.closure(...) → ArrowFunction.
        ("GameMaker.Instance", "withInstances") if args.len() == 2 => {
            let callback = take_arg(args, "GameMaker.Instance.withInstances");
            let target = take_arg(args, "GameMaker.Instance.withInstances");
            Some(rt_call(
                "withInstances",
                vec![target, callback],
                event_name.is_some(),
            ))
        }
        // GameMaker.Global.set(name, val) → _rt.variable_global_set(name, val)
        // Constant-key sets are intercepted at statement level (→ global.name = val).
        // This handles the expression-position fallback and dynamic keys.
        ("GameMaker.Global", "set") if args.len() == 2 => {
            let val = take_arg(args, "GameMaker.Global.set");
            let name = take_arg(args, "GameMaker.Global.set");
            Some(rt_call(
                "variable_global_set",
                vec![name, val],
                event_name.is_some(),
            ))
        }
        // GameMaker.Global.get(name) → global.name (constant key)
        // or variable_global_get(name) (dynamic key fallback)
        ("GameMaker.Global", "get") if args.len() == 1 => {
            let name = take_arg(args, "GameMaker.Global.get");
            if let JsExpr::Literal(Constant::String(field_name)) = name {
                // Wrap `_rt.global` in a `GameGlobalState` cast so that this
                // becomes `(_rt.global as GameGlobalState).fieldName` (or
                // `(this._rt.global as GameGlobalState).fieldName` in class
                // context), resolving TS2339 errors for game-specific global
                // variables.
                let global_expr = cast_global_for_field(global_state_type_id, event_name);
                Some(JsExpr::Field {
                    object: Box::new(global_expr),
                    field: field_name,
                })
            } else {
                Some(rt_call(
                    "variable_global_get",
                    vec![name],
                    event_name.is_some(),
                ))
            }
        }
        // GameMaker.Instance.getOn(objName, field) → ObjName.instances[0]!.field
        // GameMaker.Instance.getOn(objIdx,  field) → ObjName.instances[0]!.field  (resolved)
        // GameMaker.Instance.getOn(objId,   field) → getInstanceField(objId, field)
        ("GameMaker.Instance", "getOn") if args.len() == 2 => {
            let field = take_arg(args, "GameMaker.Instance.getOn");
            let obj_id = strip_int_coerce(take_arg(args, "GameMaker.Instance.getOn"));
            // Resolve the object: string literal name or constant integer class index.
            let instances0 = get_instances_0(&obj_id, object_names, name_map);
            if let Some(base) = instances0 {
                if let JsExpr::Literal(Constant::String(ref field_name)) = field {
                    Some(JsExpr::Field {
                        object: Box::new(base),
                        field: field_name.clone(),
                    })
                } else {
                    Some(JsExpr::Index {
                        collection: Box::new(base),
                        index: Box::new(field),
                    })
                }
            } else {
                let obj_id = resolve_instance_target(obj_id, object_names);
                Some(rt_call(
                    "getInstanceField",
                    vec![obj_id, field],
                    event_name.is_some(),
                ))
            }
        }
        // GameMaker.Instance.getOn(objName, field, index) → ObjName.instances[0]!.field[index]
        // GameMaker.Instance.getOn(objIdx,  field, index) → ObjName.instances[0]!.field[index] (resolved)
        // GameMaker.Instance.getOn(objId,   field, index) → getInstanceField(objId, field)[index]
        ("GameMaker.Instance", "getOn") if args.len() == 3 => {
            let index = take_arg(args, "GameMaker.Instance.getOn");
            let field = take_arg(args, "GameMaker.Instance.getOn");
            let obj_id = strip_int_coerce(take_arg(args, "GameMaker.Instance.getOn"));
            let instances0 = get_instances_0(&obj_id, object_names, name_map);
            let base = if let Some(base) = instances0 {
                if let JsExpr::Literal(Constant::String(ref field_name)) = field {
                    JsExpr::Field {
                        object: Box::new(base),
                        field: field_name.clone(),
                    }
                } else {
                    JsExpr::Index {
                        collection: Box::new(base),
                        index: Box::new(field),
                    }
                }
            } else {
                let obj_id = resolve_instance_target(obj_id, object_names);
                rt_call(
                    "getInstanceField",
                    vec![obj_id, field],
                    event_name.is_some(),
                )
            };
            Some(JsExpr::Index {
                collection: Box::new(base),
                index: Box::new(index),
            })
        }
        // GameMaker.Instance.setOn(objId, field, val) → setInstanceField(objId, field, val)
        // Named object+field case is handled at statement level (→ assignment).
        ("GameMaker.Instance", "setOn") if args.len() == 3 => {
            let val = take_arg(args, "GameMaker.Instance.setOn");
            let field = take_arg(args, "GameMaker.Instance.setOn");
            let obj_id = resolve_instance_target(
                strip_int_coerce(take_arg(args, "GameMaker.Instance.setOn")),
                object_names,
            );
            Some(rt_call(
                "setInstanceField",
                vec![obj_id, field, val],
                event_name.is_some(),
            ))
        }
        // GameMaker.Instance.setOn(objId, field, index, val) → setInstanceFieldIndex(objId, field, index, val)
        // Named object+field case is handled at statement level (→ indexed assignment).
        ("GameMaker.Instance", "setOn") if args.len() == 4 => {
            let val = take_arg(args, "GameMaker.Instance.setOn");
            let index = take_arg(args, "GameMaker.Instance.setOn");
            let field = take_arg(args, "GameMaker.Instance.setOn");
            let obj_id = resolve_instance_target(
                strip_int_coerce(take_arg(args, "GameMaker.Instance.setOn")),
                object_names,
            );
            Some(rt_call(
                "setInstanceFieldIndex",
                vec![obj_id, field, index, val],
                event_name.is_some(),
            ))
        }
        // GameMaker.Instance.getOther(field) → _rt.other[field]
        //
        // `other` is a stateful field on the runtime (set by withInstances), not
        // a local param — it must be reached through the runtime handle.
        ("GameMaker.Instance", "getOther") if args.len() == 1 => {
            let field = take_arg(args, "GameMaker.Instance.getOther");
            Some(JsExpr::Index {
                collection: Box::new(rt_field("other", event_name.is_some())),
                index: Box::new(field),
            })
        }
        // GameMaker.Instance.setOther(field, val) → _rt.setOtherField(_rt.other, field, val)
        ("GameMaker.Instance", "setOther") if args.len() == 2 => {
            let val = take_arg(args, "GameMaker.Instance.setOther");
            let field = take_arg(args, "GameMaker.Instance.setOther");
            Some(rt_call(
                "setOtherField",
                vec![rt_field("other", event_name.is_some()), field, val],
                event_name.is_some(),
            ))
        }
        // GameMaker.Instance.getAll(field) → _rt.getAllField(field)
        ("GameMaker.Instance", "getAll") if args.len() == 1 => {
            let field = take_arg(args, "GameMaker.Instance.getAll");
            Some(rt_call("getAllField", vec![field], event_name.is_some()))
        }
        // GameMaker.Instance.setAll(field, val) → _rt.setAllField(field, val)
        ("GameMaker.Instance", "setAll") if args.len() == 2 => {
            let val = take_arg(args, "GameMaker.Instance.setAll");
            let field = take_arg(args, "GameMaker.Instance.setAll");
            Some(rt_call(
                "setAllField",
                vec![field, val],
                event_name.is_some(),
            ))
        }
        // GameMaker.Instance.getField(target, field)
        //   If target is a const int (object index), resolve to ObjName.instances[0]!.field
        //   Otherwise → target[field]
        ("GameMaker.Instance", "getField") if args.len() == 2 => {
            let field = take_arg(args, "GameMaker.Instance.getField");
            let target = strip_int_coerce(take_arg(args, "GameMaker.Instance.getField"));
            if let JsExpr::Literal(Constant::Int(idx)) = &target {
                let idx = *idx;
                if idx >= 0 {
                    if let Some(obj_name) = object_names.get(idx as usize) {
                        let base = instances_0(obj_name.clone());
                        if let JsExpr::Literal(Constant::String(ref field_name)) = field {
                            Some(JsExpr::Field {
                                object: Box::new(base),
                                field: field_name.clone(),
                            })
                        } else {
                            Some(JsExpr::Index {
                                collection: Box::new(base),
                                index: Box::new(field),
                            })
                        }
                    } else {
                        Some(rt_call(
                            "getInstanceField",
                            vec![target, field],
                            event_name.is_some(),
                        ))
                    }
                } else {
                    // Negative object index (e.g. -4 for noone) — game author bug
                    // but faithfully translate via runtime lookup.
                    Some(rt_call(
                        "getInstanceField",
                        vec![target, field],
                        event_name.is_some(),
                    ))
                }
            } else {
                // Non-constant target (dynamic instance reference) — use runtime lookup.
                Some(rt_call(
                    "getInstanceField",
                    vec![target, field],
                    event_name.is_some(),
                ))
            }
        }
        // GameMaker.Instance.setField(target, field, val) → _rt.setInstanceField(target, field, val)
        ("GameMaker.Instance", "setField") if args.len() == 3 => {
            let val = take_arg(args, "GameMaker.Instance.setField");
            let field = take_arg(args, "GameMaker.Instance.setField");
            let target = resolve_instance_target(
                strip_int_coerce(take_arg(args, "GameMaker.Instance.setField")),
                object_names,
            );
            Some(rt_call(
                "setInstanceField",
                vec![target, field, val],
                event_name.is_some(),
            ))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Import extraction for GameMaker instance SystemCalls
// ---------------------------------------------------------------------------

/// Collect import references from GameMaker.Instance getOn/setOn calls.
///
/// When the first argument is a const string naming an object class, the rewrite
/// pass produces `ObjName.instances[0].field` — so the object class needs a
/// value import.
pub(crate) fn collect_gamemaker_instance_refs(
    args: &[ValueId],
    const_strings: &HashMap<ValueId, &str>,
    self_name: &str,
    self_ts_name: &str,
    registry: &ClassRegistry,
    external_imports: &BTreeMap<String, ExternalImport>,
    refs: &mut RefSets,
) {
    if let Some(&obj_name) = args.first().and_then(|v| const_strings.get(v)) {
        if let Some(entry) = registry.lookup(obj_name) {
            if entry.short_name != self_ts_name {
                refs.value_refs.insert(entry.short_name.clone());
            }
        } else if obj_name != self_name && external_imports.contains_key(obj_name) {
            refs.ext_value_refs.insert(obj_name.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// Boolean argument coercion for GML → TypeScript
// ---------------------------------------------------------------------------

/// Wrap numeric arguments with `!!` at positions where the function signature
/// expects `boolean`.  GML has no boolean type — 0/1 are used everywhere —
/// but the TypeScript runtime declarations use `boolean` params, causing
/// TS2345 coercion for GML's boolean/number interchangeability.
///
/// Direction 1 — number → boolean: inserts `!!arg` when a numeric expression is
/// passed to a parameter typed `"boolean"` in the runtime signature.
///
/// Direction 2 — boolean → number: inserts `Number(arg)` when a boolean expression
/// is passed to a parameter typed `"number"` or `"int"`.
pub fn coerce_bool_args(func: &mut JsFunction, sigs: &BTreeMap<String, ExternalMethodSig>) {
    if sigs.is_empty() {
        return;
    }
    coerce_bool_stmts(&mut func.body, sigs);
}

fn coerce_bool_stmts(stmts: &mut [JsStmt], sigs: &BTreeMap<String, ExternalMethodSig>) {
    for stmt in stmts.iter_mut() {
        coerce_bool_stmt(stmt, sigs);
    }
}

fn coerce_bool_stmt(stmt: &mut JsStmt, sigs: &BTreeMap<String, ExternalMethodSig>) {
    match stmt {
        JsStmt::VarDecl { init, .. } => {
            if let Some(e) = init {
                coerce_bool_expr(e, sigs);
            }
        }
        JsStmt::Assign { target, value } | JsStmt::CompoundAssign { target, value, .. } => {
            coerce_bool_expr(target, sigs);
            coerce_bool_expr(value, sigs);
        }
        JsStmt::Expr(e) | JsStmt::Throw(e) => coerce_bool_expr(e, sigs),
        JsStmt::Return(Some(e)) => coerce_bool_expr(e, sigs),
        JsStmt::Return(None) | JsStmt::Break | JsStmt::Continue | JsStmt::LabeledBreak { .. } => {}
        JsStmt::If {
            cond,
            then_body,
            else_body,
        } => {
            coerce_bool_expr(cond, sigs);
            coerce_bool_stmts(then_body, sigs);
            coerce_bool_stmts(else_body, sigs);
        }
        JsStmt::While { cond, body } => {
            coerce_bool_expr(cond, sigs);
            coerce_bool_stmts(body, sigs);
        }
        JsStmt::For {
            init,
            cond,
            update,
            body,
        } => {
            coerce_bool_stmts(init, sigs);
            coerce_bool_expr(cond, sigs);
            coerce_bool_stmts(update, sigs);
            coerce_bool_stmts(body, sigs);
        }
        JsStmt::Loop { body } => coerce_bool_stmts(body, sigs),
        JsStmt::ForOf { iterable, body, .. } => {
            coerce_bool_expr(iterable, sigs);
            coerce_bool_stmts(body, sigs);
        }
        JsStmt::Dispatch { blocks, .. } => {
            for (_, stmts) in blocks {
                coerce_bool_stmts(stmts, sigs);
            }
        }
        JsStmt::Switch {
            value,
            cases,
            default_body,
        } => {
            coerce_bool_expr(value, sigs);
            for (_, stmts) in cases {
                coerce_bool_stmts(stmts, sigs);
            }
            coerce_bool_stmts(default_body, sigs);
        }
    }
}

fn coerce_bool_expr(expr: &mut JsExpr, sigs: &BTreeMap<String, ExternalMethodSig>) {
    // Recurse into children first.
    match expr {
        JsExpr::Call { callee, args } => {
            coerce_bool_expr(callee, sigs);
            // Resolve callee name: direct `name(...)` or `_rt.name(...)`.
            let callee_name: Option<&str> = match callee.as_ref() {
                JsExpr::Var(name) => Some(name.as_str()),
                JsExpr::Field { field, .. } => Some(field.as_str()),
                _ => None,
            };
            let sig = callee_name.and_then(|n| sigs.get(n));
            if let Some(sig) = sig {
                for (i, arg) in args.iter_mut().enumerate() {
                    coerce_bool_expr(arg, sigs);
                    if let Some(param_ty) = sig.params.get(i) {
                        if param_ty == "boolean" && !is_already_boolean(arg) {
                            coerce_to_bool(arg);
                        } else if is_numeric_param(param_ty) && may_produce_boolean(arg) {
                            coerce_to_number(arg);
                        }
                    }
                }
                return;
            }
            for arg in args.iter_mut() {
                coerce_bool_expr(arg, sigs);
            }
        }
        JsExpr::Binary { lhs, rhs, .. }
        | JsExpr::Cmp { lhs, rhs, .. }
        | JsExpr::LooseEq { lhs, rhs }
        | JsExpr::LooseNe { lhs, rhs } => {
            coerce_bool_expr(lhs, sigs);
            coerce_bool_expr(rhs, sigs);
        }
        JsExpr::LogicalOr { lhs, rhs } | JsExpr::LogicalAnd { lhs, rhs } => {
            coerce_bool_expr(lhs, sigs);
            coerce_bool_expr(rhs, sigs);
        }
        JsExpr::Unary { expr: inner, .. }
        | JsExpr::Not(inner)
        | JsExpr::PostIncrement(inner)
        | JsExpr::Spread(inner)
        | JsExpr::NonNull(inner)
        | JsExpr::TypeOf(inner)
        | JsExpr::GeneratorResume(inner) => coerce_bool_expr(inner, sigs),
        JsExpr::Field { object, .. } => coerce_bool_expr(object, sigs),
        JsExpr::Index { collection, index } => {
            coerce_bool_expr(collection, sigs);
            coerce_bool_expr(index, sigs);
        }
        JsExpr::Ternary {
            cond,
            then_val,
            else_val,
        } => {
            coerce_bool_expr(cond, sigs);
            coerce_bool_expr(then_val, sigs);
            coerce_bool_expr(else_val, sigs);
        }
        JsExpr::ArrayInit(items) | JsExpr::TupleInit(items) => {
            for item in items {
                coerce_bool_expr(item, sigs);
            }
        }
        JsExpr::ObjectInit(fields) => {
            for (_, val) in fields {
                coerce_bool_expr(val, sigs);
            }
        }
        JsExpr::New { callee, args } => {
            coerce_bool_expr(callee, sigs);
            for arg in args.iter_mut() {
                coerce_bool_expr(arg, sigs);
            }
        }
        JsExpr::SuperMethodCall { args, .. } | JsExpr::GeneratorCreate { args, .. } => {
            for arg in args.iter_mut() {
                coerce_bool_expr(arg, sigs);
            }
        }
        JsExpr::In { key, object } | JsExpr::Delete { object, key } => {
            coerce_bool_expr(key, sigs);
            coerce_bool_expr(object, sigs);
        }
        JsExpr::Cast { expr: inner, .. } | JsExpr::TypeCheck { expr: inner, .. } => {
            coerce_bool_expr(inner, sigs);
        }
        JsExpr::ArrowFunction { body, .. } => coerce_bool_stmts(body, sigs),
        JsExpr::SuperCall(args) => {
            for arg in args {
                coerce_bool_expr(arg, sigs);
            }
        }
        JsExpr::SuperSet { value, .. } => coerce_bool_expr(value, sigs),
        JsExpr::Yield(inner) => {
            if let Some(e) = inner {
                coerce_bool_expr(e, sigs);
            }
        }
        JsExpr::SystemCall { args, .. } => {
            for arg in args {
                coerce_bool_expr(arg, sigs);
            }
        }
        JsExpr::NullCoalesceAssign { target, value } => {
            coerce_bool_expr(target, sigs);
            coerce_bool_expr(value, sigs);
        }
        JsExpr::Assign { lhs, rhs } => {
            coerce_bool_expr(lhs, sigs);
            coerce_bool_expr(rhs, sigs);
        }
        JsExpr::Literal(_)
        | JsExpr::Var(_)
        | JsExpr::Regex(_)
        | JsExpr::This
        | JsExpr::Activation
        | JsExpr::SuperGet(_) => {}
    }
}

/// Coerces an expression to boolean in-place.
/// Constant literals are replaced with their truthiness value directly.
/// All other non-boolean expressions are wrapped with `!!`.
fn coerce_to_bool(arg: &mut JsExpr) {
    // Extract constant from nested Coerce casts (e.g. Cast(Int(16777215), Unknown, Coerce)).
    if let Some(b) = try_const_truthiness(arg) {
        *arg = JsExpr::Literal(Constant::Bool(b));
        return;
    }
    let inner = std::mem::replace(arg, JsExpr::Literal(Constant::Null));
    *arg = JsExpr::Not(Box::new(JsExpr::Not(Box::new(inner))));
}

/// Try to evaluate the truthiness of a constant expression, unwrapping Coerce casts.
fn try_const_truthiness(expr: &JsExpr) -> Option<bool> {
    match expr {
        JsExpr::Literal(c) => Some(match c {
            Constant::Bool(b) => *b,
            Constant::Int(n) => *n != 0,
            Constant::UInt(n) => *n != 0,
            Constant::Float(f) => *f != 0.0,
            Constant::String(s) => !s.is_empty(),
            Constant::Null => false,
        }),
        JsExpr::Cast {
            expr,
            kind: CastKind::Coerce,
            ..
        } => try_const_truthiness(expr),
        _ => None,
    }
}

/// Returns true if the expression is already boolean-typed and doesn't need `!!`.
/// Looks through `Cast(expr, Unknown, Coerce)` wrappers that are no-ops at runtime.
fn is_already_boolean(expr: &JsExpr) -> bool {
    match expr {
        JsExpr::Literal(Constant::Bool(_))
        | JsExpr::Not(_)
        | JsExpr::Cmp { .. }
        | JsExpr::TypeCheck { .. }
        | JsExpr::LogicalOr { .. }
        | JsExpr::LogicalAnd { .. } => true,
        JsExpr::Cast {
            expr: inner,
            kind: CastKind::Coerce,
            ..
        } => is_already_boolean(inner),
        _ => false,
    }
}

/// Returns true if the expression may produce a boolean value, even if it also
/// produces other types (e.g. `cond ? true : 0` → `boolean | number`).
/// Used for boolean→number coercion at numeric param sites.
fn may_produce_boolean(expr: &JsExpr) -> bool {
    if is_already_boolean(expr) {
        return true;
    }
    match expr {
        JsExpr::Ternary {
            then_val, else_val, ..
        } => may_produce_boolean(then_val) || may_produce_boolean(else_val),
        JsExpr::Cast {
            expr: inner,
            kind: CastKind::Coerce,
            ..
        } => may_produce_boolean(inner),
        _ => false,
    }
}

/// Returns true if the parameter type string indicates a numeric type.
fn is_numeric_param(param_ty: &str) -> bool {
    param_ty == "number" || param_ty == "int"
}

/// Coerces a boolean expression to number in-place.
/// Wraps with `Number(expr)` — emitted as `JsExpr::Cast { ty: Float(64), kind: Coerce }`.
/// Unwraps redundant `Cast(_, Unknown, Coerce)` wrappers first (no-ops at runtime).
fn coerce_to_number(arg: &mut JsExpr) {
    // `false` → `0`, `true` → `1` (constant fold).
    if let JsExpr::Literal(Constant::Bool(b)) = arg {
        *arg = JsExpr::Literal(Constant::Float(if *b { 1.0 } else { 0.0 }));
        return;
    }
    // Unwrap `Cast(inner, Unknown, Coerce)` — replacing with `Number(inner)` directly.
    let inner = match arg {
        JsExpr::Cast {
            kind: CastKind::Coerce,
            ..
        } => {
            // Take the Cast node, then extract the inner expression.
            let taken = std::mem::replace(arg, JsExpr::Literal(Constant::Null));
            if let JsExpr::Cast { expr, .. } = taken {
                *expr
            } else {
                unreachable!()
            }
        }
        _ => std::mem::replace(arg, JsExpr::Literal(Constant::Null)),
    };
    *arg = JsExpr::Cast {
        expr: Box::new(inner),
        ty: Type::Float(64),
        kind: CastKind::Coerce,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use reincarnate_core::ir::{MethodKind, Visibility};

    #[test]
    fn coerce_to_bool_int_literal_becomes_bool_literal() {
        let mut expr = JsExpr::Literal(Constant::Int(1));
        coerce_to_bool(&mut expr);
        assert!(
            matches!(expr, JsExpr::Literal(Constant::Bool(true))),
            "Int(1) should become Bool(true)"
        );

        let mut expr = JsExpr::Literal(Constant::Int(0));
        coerce_to_bool(&mut expr);
        assert!(
            matches!(expr, JsExpr::Literal(Constant::Bool(false))),
            "Int(0) should become Bool(false)"
        );
    }

    #[test]
    fn coerce_to_bool_float_literal_becomes_bool_literal() {
        let mut expr = JsExpr::Literal(Constant::Float(2.0));
        coerce_to_bool(&mut expr);
        assert!(
            matches!(expr, JsExpr::Literal(Constant::Bool(true))),
            "Float(2.0) should become Bool(true)"
        );

        let mut expr = JsExpr::Literal(Constant::Float(0.0));
        coerce_to_bool(&mut expr);
        assert!(
            matches!(expr, JsExpr::Literal(Constant::Bool(false))),
            "Float(0.0) should become Bool(false)"
        );
    }

    #[test]
    fn coerce_to_bool_non_literal_gets_double_not() {
        // Non-literal expressions should be wrapped with !!
        let mut expr = JsExpr::Var("x".to_string());
        coerce_to_bool(&mut expr);
        assert!(
            matches!(&expr, JsExpr::Not(inner) if matches!(inner.as_ref(), JsExpr::Not(_))),
            "expected !!<expr>"
        );
    }

    #[test]
    fn coerce_to_number_bool_literal_becomes_float_literal() {
        let mut expr = JsExpr::Literal(Constant::Bool(true));
        coerce_to_number(&mut expr);
        assert!(
            matches!(expr, JsExpr::Literal(Constant::Float(f)) if f == 1.0),
            "Bool(true) should become Float(1.0), got {expr:?}"
        );

        let mut expr = JsExpr::Literal(Constant::Bool(false));
        coerce_to_number(&mut expr);
        assert!(
            matches!(expr, JsExpr::Literal(Constant::Float(f)) if f == 0.0),
            "Bool(false) should become Float(0.0), got {expr:?}"
        );
    }

    #[test]
    fn coerce_to_number_cmp_gets_number_cast() {
        let mut expr = JsExpr::Cmp {
            kind: CmpKind::Ge,
            lhs: Box::new(JsExpr::Var("x".to_string())),
            rhs: Box::new(JsExpr::Literal(Constant::Int(3))),
        };
        coerce_to_number(&mut expr);
        assert!(
            matches!(
                &expr,
                JsExpr::Cast {
                    ty: Type::Float(64),
                    kind: CastKind::Coerce,
                    ..
                }
            ),
            "Cmp should be wrapped with Cast(Float(64), Coerce), got {expr:?}"
        );
    }

    #[test]
    fn coerce_to_number_unwraps_dynamic_coerce_wrapper() {
        // Cast(Cmp → Unknown, Coerce) should become Cast(Cmp → Float(64), Coerce)
        let cmp = JsExpr::Cmp {
            kind: CmpKind::Eq,
            lhs: Box::new(JsExpr::Var("a".to_string())),
            rhs: Box::new(JsExpr::Literal(Constant::Int(0))),
        };
        let mut expr = JsExpr::Cast {
            expr: Box::new(cmp),
            ty: Type::Value,
            kind: CastKind::Coerce,
        };
        coerce_to_number(&mut expr);
        match &expr {
            JsExpr::Cast {
                expr: inner,
                ty: Type::Float(64),
                kind: CastKind::Coerce,
            } => {
                assert!(
                    matches!(inner.as_ref(), JsExpr::Cmp { .. }),
                    "inner should be Cmp, got {inner:?}"
                );
            }
            _ => panic!("expected Cast(Cmp, Float(64), Coerce), got {expr:?}"),
        }
    }

    #[test]
    fn coerce_bool_args_coerces_bool_to_number_at_call_site() {
        let mut sigs = BTreeMap::new();
        sigs.insert(
            "lerp".to_string(),
            ExternalMethodSig {
                params: vec![
                    "number".to_string(),
                    "number".to_string(),
                    "number".to_string(),
                ],
                returns: "number".to_string(),
            },
        );
        // lerp(x, y >= 3, 0.5) — 2nd arg is Cmp (boolean), should get Number() wrap
        let mut func = JsFunction {
            name: "test".to_string(),
            params: vec![],
            body: vec![JsStmt::Expr(JsExpr::Call {
                callee: Box::new(JsExpr::Var("lerp".to_string())),
                args: vec![
                    JsExpr::Var("x".to_string()),
                    JsExpr::Cmp {
                        kind: CmpKind::Ge,
                        lhs: Box::new(JsExpr::Var("y".to_string())),
                        rhs: Box::new(JsExpr::Literal(Constant::Int(3))),
                    },
                    JsExpr::Literal(Constant::Float(0.5)),
                ],
            })],
            param_defaults: vec![],
            return_ty: Type::Value,
            is_generator: false,
            visibility: Visibility::Public,
            method_kind: MethodKind::Free,
            has_rest_param: false,
            num_capture_params: 0,
            capture_modes: vec![],
            overloads: vec![],
        };
        coerce_bool_args(&mut func, &sigs);
        // The 2nd arg should now be Cast(Cmp, Float(64), Coerce)
        if let JsStmt::Expr(JsExpr::Call { args, .. }) = &func.body[0] {
            assert!(
                matches!(
                    &args[1],
                    JsExpr::Cast {
                        ty: Type::Float(64),
                        kind: CastKind::Coerce,
                        ..
                    }
                ),
                "2nd arg should be Number()-wrapped, got {:?}",
                args[1]
            );
        } else {
            panic!("expected Call statement");
        }
    }

    #[test]
    fn coerce_bool_args_ternary_with_boolean_branch_gets_number_wrap() {
        let mut sigs = BTreeMap::new();
        sigs.insert(
            "lerp".to_string(),
            ExternalMethodSig {
                params: vec![
                    "number".to_string(),
                    "number".to_string(),
                    "number".to_string(),
                ],
                returns: "number".to_string(),
            },
        );
        // lerp(x, cond ? !y : 0, 0.5) — ternary with boolean branch
        let mut func = JsFunction {
            name: "test".to_string(),
            params: vec![],
            body: vec![JsStmt::Expr(JsExpr::Call {
                callee: Box::new(JsExpr::Var("lerp".to_string())),
                args: vec![
                    JsExpr::Var("x".to_string()),
                    JsExpr::Ternary {
                        cond: Box::new(JsExpr::Var("cond".to_string())),
                        then_val: Box::new(JsExpr::Not(Box::new(JsExpr::Var("y".to_string())))),
                        else_val: Box::new(JsExpr::Literal(Constant::Int(0))),
                    },
                    JsExpr::Literal(Constant::Float(0.5)),
                ],
            })],
            param_defaults: vec![],
            return_ty: Type::Value,
            is_generator: false,
            visibility: Visibility::Public,
            method_kind: MethodKind::Free,
            has_rest_param: false,
            num_capture_params: 0,
            capture_modes: vec![],
            overloads: vec![],
        };
        coerce_bool_args(&mut func, &sigs);
        // The 2nd arg (ternary) should now be wrapped with Number()
        if let JsStmt::Expr(JsExpr::Call { args, .. }) = &func.body[0] {
            assert!(
                matches!(
                    &args[1],
                    JsExpr::Cast {
                        ty: Type::Float(64),
                        kind: CastKind::Coerce,
                        ..
                    }
                ),
                "ternary with boolean branch should be Number()-wrapped, got {:?}",
                args[1]
            );
        } else {
            panic!("expected Call statement");
        }
    }

    #[test]
    fn rewrite_introduced_calls_never_introduces_bare_names() {
        // GameMaker.Instance/Global syscalls now rewrite to rt-qualified member
        // calls (`_rt.withInstances(...)`), so no bare free-function identifiers
        // are introduced and nothing needs importing.
        assert_eq!(
            rewrite_introduced_calls("GameMaker.Instance", "withInstances"),
            &[] as &[&str]
        );
        assert_eq!(
            rewrite_introduced_calls("GameMaker.Instance", "getOn"),
            &[] as &[&str]
        );
        assert_eq!(
            rewrite_introduced_calls("GameMaker.Instance", "withEnd"),
            &[] as &[&str]
        );
        assert_eq!(
            rewrite_introduced_calls("Unknown.System", "whatever"),
            &[] as &[&str]
        );
    }
}

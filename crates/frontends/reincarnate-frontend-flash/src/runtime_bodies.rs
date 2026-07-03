//! Flash/AVM2 runtime builtin bodies.
//!
//! AVM2's plain `add` opcode is the ECMAScript `+` operator: string
//! concatenation when either operand is a String, numeric addition (ToNumber
//! on both operands) otherwise.  The operand types are not carried on the
//! opcode, so the translator emits a call to the polymorphic `add_any`
//! builtin registered here.  After HM inference resolves the operand types,
//! the core `BuiltinOverloadSelect` pass rewrites call sites with concrete
//! argument types to the typed variants (`add_f64`, `concat_str`) via the
//! `specializations` table; only genuinely-dynamic call sites keep the
//! runtime dispatch below.
//!
//! This registration is AVM2-specific and therefore lives in the Flash
//! frontend, not core: GML's `add_any` (in the GameMaker frontend) requires
//! *both* operands to share a type, while ECMAScript concatenates when
//! *either* operand is a String.

use std::collections::HashMap;

use reincarnate_core::ir::func::FuncId;
use reincarnate_core::ir::module::Module;
use reincarnate_core::ir::ty::{FunctionSig, Type};

/// Register Flash-specific polymorphic builtins and attach their IR bodies.
///
/// Must be called before method translation so that `add_any` is resolvable
/// by `FunctionBuilder::call_named` (the translator receives a clone of the
/// runtime registry taken after this call).
pub fn register_runtime_bodies(module: &mut Module) {
    register_add_any(module);
}

/// Register `add_any(Value, Value) -> Value` with the ECMAScript `+` dispatch
/// body and its specialization table.
///
/// The dispatch checks each operand for String-ness at runtime; the numeric
/// arm coerces both operands with ToNumber semantics (the same
/// `Cast(_, Float(64), Coerce)` the translator emits for AVM2 `convert_d`).
///
/// Known gap (tracked in TODO.md): for *object* operands ECMAScript applies
/// ToPrimitive first and branches on the primitive's type, so an object whose
/// ToPrimitive yields a String concatenates.  Expressing that ordering in IR
/// requires a ToPrimitive backend primitive; until then the branch decision
/// sees the object itself (not String → numeric arm).  All primitive operands
/// (Number, int, uint, String, Boolean, null, undefined) behave exactly per
/// spec.
fn register_add_any(module: &mut Module) {
    let sig = FunctionSig {
        params: vec![Type::Value, Type::Value],
        return_ty: Type::Value,
        ..Default::default()
    };
    let any_id = module.register_runtime("add_any", sig);

    module.attach_runtime_body("add_any", &[Type::Value, Type::Value], Type::Value, |fb| {
        let a = fb.param(0);
        let b = fb.param(1);

        // Every branch target has exactly one predecessor (the concat arm
        // is duplicated rather than shared), matching the dispatch shape
        // established by the GML `_any` builtins: the structurizer only
        // handles chains whose true-targets are unique and whose false
        // edges fall through (see TODO.md, "structurizer mis-emits shared
        // forward true-targets").
        let concat_a_block = fb.create_block();
        let check_b_block = fb.create_block();
        let concat_b_block = fb.create_block();
        let numeric_block = fb.create_block();

        // entry: if a is a String -> concat, else check b.
        let a_is_str = fb.type_check(a, Type::String);
        fb.br_if(a_is_str, concat_a_block, &[], check_b_block, &[]);

        // a is a String: ToString both operands, concatenate.
        fb.switch_to_block(concat_a_block);
        let sa = fb.coerce(a, Type::String);
        let sb = fb.coerce(b, Type::String);
        let r = fb.call_named("concat_str", &[sa, sb], Type::String);
        fb.ret(Some(r));

        // check_b: if b is a String -> concat, else numeric.
        fb.switch_to_block(check_b_block);
        let b_is_str = fb.type_check(b, Type::String);
        fb.br_if(b_is_str, concat_b_block, &[], numeric_block, &[]);

        // b is a String: ToString both operands, concatenate.
        fb.switch_to_block(concat_b_block);
        let sa = fb.coerce(a, Type::String);
        let sb = fb.coerce(b, Type::String);
        let r = fb.call_named("concat_str", &[sa, sb], Type::String);
        fb.ret(Some(r));

        // numeric: ToNumber both operands, add.
        fb.switch_to_block(numeric_block);
        let na = fb.coerce(a, Type::Float(64));
        let nb = fb.coerce(b, Type::Float(64));
        let r = fb.call_named("add_f64", &[na, nb], Type::Float(64));
        fb.ret(Some(r));
    });

    // Specializations for BuiltinOverloadSelect: exact concrete argument
    // types -> typed variant.  Int/UInt pairs are deliberately absent: AVM2
    // `add` on two ints produces a Number (no i32 wrapping), so `add_i32`
    // would be semantically wrong, and the pass cannot insert the required
    // Int -> Float coercions for `add_f64` (tracked in TODO.md).
    let mut specs: HashMap<Vec<Type>, FuncId> = HashMap::new();
    specs.insert(
        vec![Type::Float(64), Type::Float(64)],
        module.runtime_registry["add_f64"],
    );
    specs.insert(
        vec![Type::String, Type::String],
        module.runtime_registry["concat_str"],
    );
    module.functions[any_id].specializations = specs;
}

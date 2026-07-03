use std::f64::consts::PI;

use reincarnate_core::ir::builder::FunctionBuilder;
use reincarnate_core::ir::func::Visibility;
use reincarnate_core::ir::inst::CmpKind;
use reincarnate_core::ir::module::{FieldDef, Module, TypeDecl};
use reincarnate_core::ir::ty::{FunctionSig, Type, TypeId};

/// Attach IR bodies to GML runtime stubs that have closed-form math definitions.
///
/// Called after `register_runtime` has already created empty stubs for these
/// functions.  We build a full `Function` with `FunctionBuilder`, then replace
/// the stub's `blocks`, `insts`, `value_types`, and `entry` with the built
/// function — the stub's `name`, `sig`, `visibility`, and registry entry are
/// left untouched.
///
/// The bodies use only `*_f64` builtin calls and `Const(Float)` values, so they
/// are legal IR for any pipeline stage that runs after registration.
pub fn register_runtime_bodies(module: &mut Module) {
    // Register XorGen type: { x: Array<i32>, i: i32 }
    let xorgen_type_id = module.types.push(TypeDecl::Object {
        name: Some("XorGen".to_string()),
        namespace: Vec::new(),
        visibility: Visibility::Public,
        parent: None,
        fields: vec![
            FieldDef {
                name: "x".to_string(),
                ty: Type::Array(Box::new(Type::Int(32))),
                default: None,
            },
            FieldDef {
                name: "i".to_string(),
                ty: Type::Int(32),
                default: None,
            },
        ],
        methods: vec![],
        class_ref: None,
        inferred: false,
    });
    module
        .name_table
        .type_names
        .push(Some("XorGen".to_string()));
    module
        .type_names
        .insert("XorGen".to_string(), xorgen_type_id);

    let xorgen_ty = Type::Instance(xorgen_type_id);

    // Register MathState type: { prng: XorGen }
    let math_state_type_id = module.types.push(TypeDecl::Object {
        name: Some("MathState".to_string()),
        namespace: Vec::new(),
        visibility: Visibility::Public,
        parent: None,
        fields: vec![FieldDef {
            name: "prng".to_string(),
            ty: xorgen_ty.clone(),
            default: None,
        }],
        methods: vec![],
        class_ref: None,
        inferred: false,
    });
    module
        .name_table
        .type_names
        .push(Some("MathState".to_string()));
    module
        .type_names
        .insert("MathState".to_string(), math_state_type_id);

    // Add _math field to GameRuntime
    if let Some(rt_id) = module.runtime_type_id {
        if let TypeDecl::Object { fields, .. } = &mut module.types[rt_id] {
            fields.push(FieldDef {
                name: "_math".to_string(),
                ty: Type::Instance(math_state_type_id),
                default: None,
            });
        }
    }

    // Register xorgen_next stub: (XorGen) -> i32
    let xorgen_next_sig = FunctionSig {
        params: vec![xorgen_ty.clone()],
        return_ty: Type::Int(32),
        defaults: vec![],
        has_rest_param: false,
        param_lower_bounds: vec![],
    };
    module.register_runtime("xorgen_next", xorgen_next_sig);

    attach_body_xorgen_next(module, xorgen_type_id);
    attach_body_random(module, xorgen_type_id, math_state_type_id);
    attach_body_random_range(module);

    attach_body_point_in_rectangle(module);
    attach_body_point_in_circle(module);
    attach_body_lengthdir_x(module);
    attach_body_lengthdir_y(module);
    attach_body_point_distance(module);
    attach_body_point_distance_3d(module);
    attach_body_degtorad(module);
    attach_body_radtodeg(module);
    attach_body_dsin(module);
    attach_body_dcos(module);
    attach_body_dtan(module);
    attach_body_darcsin(module);
    attach_body_darccos(module);
    attach_body_darctan(module);
    attach_body_darctan2(module);
    attach_body_arctan2(module);
    attach_body_point_direction(module);
    attach_body_sqr(module);
    attach_body_power(module);
    attach_body_logn(module);
    attach_body_log2(module);
    attach_body_log10(module);
    attach_body_exp(module);
    attach_body_clamp(module);
    attach_body_lerp(module);
    attach_body_abs(module);
    attach_body_floor(module);
    attach_body_ceil(module);
    attach_body_round(module);
    attach_body_sign(module);
    attach_body_sqrt(module);
    attach_body_arctan(module);
    attach_body_frac(module);
    attach_body_dot_product(module);
    attach_body_dot_product_3d(module);
    attach_body_color_get_red(module);
    module.register_alias(
        "colour_get_red",
        module.lookup_runtime("color_get_red").unwrap(),
    );
    attach_body_color_get_green(module);
    module.register_alias(
        "colour_get_green",
        module.lookup_runtime("color_get_green").unwrap(),
    );
    attach_body_color_get_blue(module);
    module.register_alias(
        "colour_get_blue",
        module.lookup_runtime("color_get_blue").unwrap(),
    );
    attach_body_make_color_rgb(module);
    module.register_alias(
        "make_colour_rgb",
        module.lookup_runtime("make_color_rgb").unwrap(),
    );
    attach_body_merge_color(module);
    module.register_alias(
        "merge_colour",
        module.lookup_runtime("merge_color").unwrap(),
    );
    attach_body_color_get_value(module);
    module.register_alias(
        "colour_get_value",
        module.lookup_runtime("color_get_value").unwrap(),
    );
    attach_body_color_get_saturation(module);
    module.register_alias(
        "colour_get_saturation",
        module.lookup_runtime("color_get_saturation").unwrap(),
    );
    attach_body_color_get_hue(module);
    module.register_alias(
        "colour_get_hue",
        module.lookup_runtime("color_get_hue").unwrap(),
    );
    attach_body_make_color_hsv(module);
    module.register_alias(
        "make_colour_hsv",
        module.lookup_runtime("make_color_hsv").unwrap(),
    );
    attach_body_string_length(module);
    attach_body_string_upper(module);
    attach_body_string_lower(module);
    attach_body_string_char_at(module);
    attach_body_string_copy(module);
    attach_body_string_pos(module);
    attach_body_string_delete(module);
    attach_body_string_insert(module);
    attach_body_string_replace_all(module);
    attach_body_string_count(module);
    attach_body_string_ord_at(module);
    attach_body_string_repeat(module);
    attach_body_string_replace(module);
    attach_body_string_hash_to_newline(module);
    attach_body_string_trim(module);
    attach_body_array_length(module);
    attach_body_array_length_1d(module);
    attach_body_array_contains(module);
    attach_body_sin(module);
    attach_body_cos(module);
    attach_body_tan(module);
    attach_body_arcsin(module);
    attach_body_arccos(module);
    attach_body_ord(module);
    attach_body_string_byte_at(module);
    attach_body_string_digits(module);
    attach_body_string_letters(module);
    attach_body_string_format(module);
    attach_body_chr(module);
    attach_body_ln(module);
    attach_body_math_get_epsilon(module);
    attach_body_is_nan(module);
    attach_body_is_infinity(module);
    attach_body_is_bool(module);
    attach_body_is_real(module);
    attach_body_is_string(module);
    attach_body_is_undefined(module);
    attach_body_is_array(module);
    attach_body_is_method(module);
    attach_body_is_struct(module);
    attach_body_is_numeric(module);
    attach_body_typeof(module);
    attach_body_real(module);
    attach_body_pass(module);
    attach_body_try_hook(module);
    attach_body_try_unhook(module);
    attach_body_approach(module);
    attach_body_angle_difference(module);
    attach_body_rectangle_in_rectangle(module);
    attach_body_matrix_build(module);
    attach_body_array_copy(module);
    attach_body_array_equals(module);
    attach_body_array_get(module);
    attach_body_array_set(module);
    attach_body_array_height_2d(module);
    attach_body_array_pop(module);
    attach_body_array_delete(module);
    attach_body_array_insert(module);
    attach_body_array_resize(module);
    attach_body_array_get_index(module);
    attach_body_point_in_triangle(module);
    attach_body_variable_struct_exists(module);
    attach_body_variable_struct_get(module);
    attach_body_variable_struct_names_count(module);
    attach_body_variable_struct_get_names(module);
    attach_body_variable_struct_set(module);
    attach_body_array_sort(module);
    attach_body_array_unique(module);
}

// ---------------------------------------------------------------------------
// Helper: build a FunctionBuilder pre-loaded with the module's runtime registry
// so that arithmetic helpers (add, mul, etc.) and call_named work correctly.
// ---------------------------------------------------------------------------

fn make_builder(module: &Module, name: &str, sig: FunctionSig) -> FunctionBuilder {
    let registry = module.runtime_registry.clone();
    let mut b = FunctionBuilder::new(name, sig, Visibility::Public);
    b.set_registry(registry);
    b
}

// ---------------------------------------------------------------------------
// point_in_rectangle(px, py, x1, y1, x2, y2: f64) -> Bool
//   =  px >= x1 && px <= x2 && py >= y1 && py <= y2
// ---------------------------------------------------------------------------

fn attach_body_point_in_rectangle(module: &mut Module) {
    module.attach_runtime_body(
        "point_in_rectangle",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Bool,
        |b| {
            let px = b.param(0);
            let py = b.param(1);
            let x1 = b.param(2);
            let y1 = b.param(3);
            let x2 = b.param(4);
            let y2 = b.param(5);

            let px_ge_x1 = b.cmp(CmpKind::Ge, px, x1);
            let px_le_x2 = b.cmp(CmpKind::Le, px, x2);
            let py_ge_y1 = b.cmp(CmpKind::Ge, py, y1);
            let py_le_y2 = b.cmp(CmpKind::Le, py, y2);
            let in_x = b.call_named("and_bool", &[px_ge_x1, px_le_x2], Type::Bool);
            let in_y = b.call_named("and_bool", &[py_ge_y1, py_le_y2], Type::Bool);
            let result = b.call_named("and_bool", &[in_x, in_y], Type::Bool);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// point_in_circle(px, py, cx, cy, radius: f64) -> Bool
//   =  (px - cx)^2 + (py - cy)^2 <= radius^2
// ---------------------------------------------------------------------------

fn attach_body_point_in_circle(module: &mut Module) {
    module.attach_runtime_body(
        "point_in_circle",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Bool,
        |b| {
            let px = b.param(0);
            let py = b.param(1);
            let cx = b.param(2);
            let cy = b.param(3);
            let radius = b.param(4);

            let dx = b.sub(px, cx);
            let dy = b.sub(py, cy);
            let dx2 = b.mul(dx, dx);
            let dy2 = b.mul(dy, dy);
            let dist2 = b.add(dx2, dy2);
            let r2 = b.mul(radius, radius);
            let result = b.cmp(CmpKind::Le, dist2, r2);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// lengthdir_x(len: f64, dir: f64) -> f64  =  len * cos(dir * π/180)
// ---------------------------------------------------------------------------

fn attach_body_lengthdir_x(module: &mut Module) {
    module.attach_runtime_body(
        "lengthdir_x",
        &[Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let len = b.param(0);
            let dir = b.param(1);

            let pi_over_180 = b.const_float(PI / 180.0);
            let dir_rad = b.mul(dir, pi_over_180);
            let cos_val = b.call_named("cos_f64", &[dir_rad], Type::Float(64));
            let result = b.mul(len, cos_val);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// lengthdir_y(len: f64, dir: f64) -> f64  =  len * -sin(dir * π/180)
//
// GML uses a y-down coordinate system where angle 0 points right and
// increases counter-clockwise.  The GameMaker manual defines
// `lengthdir_y` as `len * -sin(dir * π/180)` because increasing y goes
// down, which flips the vertical component.
// ---------------------------------------------------------------------------

fn attach_body_lengthdir_y(module: &mut Module) {
    module.attach_runtime_body(
        "lengthdir_y",
        &[Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let len = b.param(0);
            let dir = b.param(1);

            let pi_over_180 = b.const_float(PI / 180.0);
            let dir_rad = b.mul(dir, pi_over_180);
            let sin_val = b.call_named("sin_f64", &[dir_rad], Type::Float(64));
            let neg_sin = b.neg(sin_val);
            let result = b.mul(len, neg_sin);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// point_distance(x1, y1, x2, y2: f64) -> f64  =  hypot(x2-x1, y2-y1)
// ---------------------------------------------------------------------------

fn attach_body_point_distance(module: &mut Module) {
    module.attach_runtime_body(
        "point_distance",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Float(64),
        |b| {
            let x1 = b.param(0);
            let y1 = b.param(1);
            let x2 = b.param(2);
            let y2 = b.param(3);

            let dx = b.sub(x2, x1);
            let dy = b.sub(y2, y1);
            let result = b.call_named("hypot_f64", &[dx, dy], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// point_distance_3d(x1, y1, z1, x2, y2, z2: f64) -> f64
//   =  sqrt((x2-x1)^2 + (y2-y1)^2 + (z2-z1)^2)
// ---------------------------------------------------------------------------

fn attach_body_point_distance_3d(module: &mut Module) {
    module.attach_runtime_body(
        "point_distance_3d",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Float(64),
        |b| {
            let x1 = b.param(0);
            let y1 = b.param(1);
            let z1 = b.param(2);
            let x2 = b.param(3);
            let y2 = b.param(4);
            let z2 = b.param(5);

            let dx = b.sub(x2, x1);
            let dy = b.sub(y2, y1);
            let dz = b.sub(z2, z1);
            let dx2 = b.mul(dx, dx);
            let dy2 = b.mul(dy, dy);
            let dz2 = b.mul(dz, dz);
            let xy = b.add(dx2, dy2);
            let sum = b.add(xy, dz2);
            let result = b.call_named("sqrt_f64", &[sum], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// degtorad(x: f64) -> f64  =  x * π/180
// ---------------------------------------------------------------------------

fn attach_body_degtorad(module: &mut Module) {
    module.attach_runtime_body("degtorad", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let factor = b.const_float(PI / 180.0);
        let result = b.mul(x, factor);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// radtodeg(x: f64) -> f64  =  x * 180/π
// ---------------------------------------------------------------------------

fn attach_body_radtodeg(module: &mut Module) {
    module.attach_runtime_body("radtodeg", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let factor = b.const_float(180.0 / PI);
        let result = b.mul(x, factor);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// dsin(x: f64) -> f64  =  sin(x * π/180)
// ---------------------------------------------------------------------------

fn attach_body_dsin(module: &mut Module) {
    module.attach_runtime_body("dsin", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let factor = b.const_float(PI / 180.0);
        let rad = b.mul(x, factor);
        let result = b.call_named("sin_f64", &[rad], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// dcos(x: f64) -> f64  =  cos(x * π/180)
// ---------------------------------------------------------------------------

fn attach_body_dcos(module: &mut Module) {
    module.attach_runtime_body("dcos", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let factor = b.const_float(PI / 180.0);
        let rad = b.mul(x, factor);
        let result = b.call_named("cos_f64", &[rad], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// dtan(x: f64) -> f64  =  tan(x * π/180)
// ---------------------------------------------------------------------------

fn attach_body_dtan(module: &mut Module) {
    module.attach_runtime_body("dtan", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let factor = b.const_float(PI / 180.0);
        let rad = b.mul(x, factor);
        let result = b.call_named("tan_f64", &[rad], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// darcsin(x: f64) -> f64  =  asin(x) * 180/π
// ---------------------------------------------------------------------------

fn attach_body_darcsin(module: &mut Module) {
    module.attach_runtime_body("darcsin", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let asin_val = b.call_named("asin_f64", &[x], Type::Float(64));
        let factor = b.const_float(180.0 / PI);
        let result = b.mul(asin_val, factor);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// darccos(x: f64) -> f64  =  acos(x) * 180/π
// ---------------------------------------------------------------------------

fn attach_body_darccos(module: &mut Module) {
    module.attach_runtime_body("darccos", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let acos_val = b.call_named("acos_f64", &[x], Type::Float(64));
        let factor = b.const_float(180.0 / PI);
        let result = b.mul(acos_val, factor);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// darctan(x: f64) -> f64  =  atan(x) * 180/π
// ---------------------------------------------------------------------------

fn attach_body_darctan(module: &mut Module) {
    module.attach_runtime_body("darctan", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let atan_val = b.call_named("atan_f64", &[x], Type::Float(64));
        let factor = b.const_float(180.0 / PI);
        let result = b.mul(atan_val, factor);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// darctan2(y: f64, x: f64) -> f64  =  atan2(y, x) * 180/π
// ---------------------------------------------------------------------------

fn attach_body_darctan2(module: &mut Module) {
    module.attach_runtime_body(
        "darctan2",
        &[Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let y = b.param(0);
            let x = b.param(1);
            let atan2_val = b.call_named("atan2_f64", &[y, x], Type::Float(64));
            let factor = b.const_float(180.0 / PI);
            let result = b.mul(atan2_val, factor);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// arctan2(y: f64, x: f64) -> f64  =  atan2(y, x)  [result in radians]
// ---------------------------------------------------------------------------

fn attach_body_arctan2(module: &mut Module) {
    module.attach_runtime_body(
        "arctan2",
        &[Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let y = b.param(0);
            let x = b.param(1);
            let result = b.call_named("atan2_f64", &[y, x], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// point_direction(x1, y1, x2, y2: f64) -> f64
//   = atan2(y1 - y2, x2 - x1) * 180/π
//
// GML uses a y-down coordinate system, so the angle from (x1,y1) toward
// (x2,y2) is computed as atan2(y1-y2, x2-x1) — the y delta is inverted
// relative to standard math convention.
// ---------------------------------------------------------------------------

fn attach_body_point_direction(module: &mut Module) {
    module.attach_runtime_body(
        "point_direction",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Float(64),
        |b| {
            let x1 = b.param(0);
            let y1 = b.param(1);
            let x2 = b.param(2);
            let y2 = b.param(3);

            let dy = b.sub(y1, y2);
            let dx = b.sub(x2, x1);
            let atan2_val = b.call_named("atan2_f64", &[dy, dx], Type::Float(64));
            let factor = b.const_float(180.0 / PI);
            let result = b.mul(atan2_val, factor);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// sqr(x: f64) -> f64  =  x * x
// ---------------------------------------------------------------------------

fn attach_body_sqr(module: &mut Module) {
    module.attach_runtime_body("sqr", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.mul(x, x);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// power(base: f64, exp: f64) -> f64  =  pow(base, exp)
// ---------------------------------------------------------------------------

fn attach_body_power(module: &mut Module) {
    module.attach_runtime_body(
        "power",
        &[Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let base = b.param(0);
            let exp = b.param(1);
            let result = b.call_named("pow_f64", &[base, exp], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// logn(n: f64, val: f64) -> f64  =  ln(val) / ln(n)
// ---------------------------------------------------------------------------

fn attach_body_logn(module: &mut Module) {
    module.attach_runtime_body(
        "logn",
        &[Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let n = b.param(0);
            let val = b.param(1);
            let ln_val = b.call_named("ln_f64", &[val], Type::Float(64));
            let ln_n = b.call_named("ln_f64", &[n], Type::Float(64));
            let result = b.div(ln_val, ln_n);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// log2(x: f64) -> f64  =  log2(x)
// ---------------------------------------------------------------------------

fn attach_body_log2(module: &mut Module) {
    module.attach_runtime_body("log2", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("log2_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// log10(x: f64) -> f64  =  log10(x)
// ---------------------------------------------------------------------------

fn attach_body_log10(module: &mut Module) {
    module.attach_runtime_body("log10", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("log10_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// exp(x: f64) -> f64  =  e^x
// ---------------------------------------------------------------------------

fn attach_body_exp(module: &mut Module) {
    module.attach_runtime_body("exp", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("exp_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// clamp(val: f64, min: f64, max: f64) -> f64  =  min_f64(max_f64(val, min), max)
// ---------------------------------------------------------------------------

fn attach_body_clamp(module: &mut Module) {
    module.attach_runtime_body(
        "clamp",
        &[Type::Float(64), Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let val = b.param(0);
            let lo = b.param(1);
            let hi = b.param(2);
            let clamped_lo = b.call_named("max_f64", &[val, lo], Type::Float(64));
            let result = b.call_named("min_f64", &[clamped_lo, hi], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// lerp(a: f64, b: f64, amt: f64) -> f64  =  a * (1 - amt) + b * amt
// ---------------------------------------------------------------------------

fn attach_body_lerp(module: &mut Module) {
    module.attach_runtime_body(
        "lerp",
        &[Type::Float(64), Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let a = b.param(0);
            let bv = b.param(1);
            let amt = b.param(2);

            // a * (1 - amt) + b * amt
            let one = b.const_float(1.0);
            let one_minus_amt = b.sub(one, amt);
            let a_part = b.mul(a, one_minus_amt);
            let b_part = b.mul(bv, amt);
            let result = b.add(a_part, b_part);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// abs(x: f64) -> f64  =  abs(x)
// ---------------------------------------------------------------------------

fn attach_body_abs(module: &mut Module) {
    module.attach_runtime_body("abs", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("abs_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// floor(x: f64) -> f64  =  floor(x)
// ---------------------------------------------------------------------------

fn attach_body_floor(module: &mut Module) {
    module.attach_runtime_body("floor", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("floor_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// ceil(x: f64) -> f64  =  ceil(x)
// ---------------------------------------------------------------------------

fn attach_body_ceil(module: &mut Module) {
    module.attach_runtime_body("ceil", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("ceil_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// round(x: f64) -> f64  =  round(x)
//
// GML round uses round-half-away-from-zero, same as Math.round for positive
// values and mirrored for negative.  round_f64 maps to Math.round.
// ---------------------------------------------------------------------------

fn attach_body_round(module: &mut Module) {
    module.attach_runtime_body("round", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("round_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// sign(x: f64) -> f64  =  sign(x)  [returns -1, 0, or 1]
// ---------------------------------------------------------------------------

fn attach_body_sign(module: &mut Module) {
    module.attach_runtime_body("sign", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("sign_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// sqrt(x: f64) -> f64  =  sqrt(x)
// ---------------------------------------------------------------------------

fn attach_body_sqrt(module: &mut Module) {
    module.attach_runtime_body("sqrt", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("sqrt_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// arctan(x: f64) -> f64  =  atan(x)  [result in radians]
// ---------------------------------------------------------------------------

fn attach_body_arctan(module: &mut Module) {
    module.attach_runtime_body("arctan", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("atan_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// frac(x: f64) -> f64  =  x - trunc(x)
//
// Returns the fractional part of x (the digits after the decimal point).
// For negative values, e.g. frac(-3.7) = -3.7 - (-3.0) = -0.7.
// ---------------------------------------------------------------------------

fn attach_body_frac(module: &mut Module) {
    module.attach_runtime_body("frac", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let trunc_val = b.call_named("trunc_f64", &[x], Type::Float(64));
        let result = b.sub(x, trunc_val);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// dot_product(x1, y1, x2, y2: f64) -> f64  =  x1*x2 + y1*y2
// ---------------------------------------------------------------------------

fn attach_body_dot_product(module: &mut Module) {
    module.attach_runtime_body(
        "dot_product",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Float(64),
        |b| {
            let x1 = b.param(0);
            let y1 = b.param(1);
            let x2 = b.param(2);
            let y2 = b.param(3);

            let x_term = b.mul(x1, x2);
            let y_term = b.mul(y1, y2);
            let result = b.add(x_term, y_term);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// dot_product_3d(x1, y1, z1, x2, y2, z2: f64) -> f64  =  x1*x2 + y1*y2 + z1*z2
// ---------------------------------------------------------------------------

fn attach_body_dot_product_3d(module: &mut Module) {
    module.attach_runtime_body(
        "dot_product_3d",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Float(64),
        |b| {
            let x1 = b.param(0);
            let y1 = b.param(1);
            let z1 = b.param(2);
            let x2 = b.param(3);
            let y2 = b.param(4);
            let z2 = b.param(5);

            let x_term = b.mul(x1, x2);
            let y_term = b.mul(y1, y2);
            let z_term = b.mul(z1, z2);
            let xy = b.add(x_term, y_term);
            let result = b.add(xy, z_term);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// color_get_red(color: f64) -> f64  =  color & 0xFF
//
// GML colors use BGR byte order; red is in the low byte.
// ---------------------------------------------------------------------------

fn attach_body_color_get_red(module: &mut Module) {
    module.attach_runtime_body("color_get_red", &[Type::Float(64)], Type::Float(64), |b| {
        let color = b.param(0);
        let mask = b.const_float(255.0);
        let result = b.bit_and(color, mask);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// color_get_green(color: f64) -> f64  =  (color >> 8) & 0xFF
//
// Green occupies the middle byte.
// ---------------------------------------------------------------------------

fn attach_body_color_get_green(module: &mut Module) {
    module.attach_runtime_body(
        "color_get_green",
        &[Type::Float(64)],
        Type::Float(64),
        |b| {
            let color = b.param(0);
            let shift = b.const_float(8.0);
            let shifted = b.shr(color, shift);
            let mask = b.const_float(255.0);
            let result = b.bit_and(shifted, mask);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// color_get_blue(color: f64) -> f64  =  color >> 16
//
// Blue occupies the high byte; no mask needed after shifting.
// ---------------------------------------------------------------------------

fn attach_body_color_get_blue(module: &mut Module) {
    module.attach_runtime_body("color_get_blue", &[Type::Float(64)], Type::Float(64), |b| {
        let color = b.param(0);
        let shift = b.const_float(16.0);
        let result = b.shr(color, shift);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// make_color_rgb(r: f64, g: f64, b: f64) -> f64  =  (b << 16) | (g << 8) | r
//
// Packs RGB components into a GML BGR color value.
// ---------------------------------------------------------------------------

fn attach_body_make_color_rgb(module: &mut Module) {
    module.attach_runtime_body(
        "make_color_rgb",
        &[Type::Float(64), Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let r = b.param(0);
            let g = b.param(1);
            let bv = b.param(2);

            let shift16 = b.const_float(16.0);
            let shift8 = b.const_float(8.0);
            let b_shifted = b.shl(bv, shift16);
            let g_shifted = b.shl(g, shift8);
            let bg = b.bit_or(b_shifted, g_shifted);
            let result = b.bit_or(bg, r);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// merge_color(col1: f64, col2: f64, amount: f64) -> f64
//
// Blend two BGR colors by linearly interpolating each channel:
//   make_color_rgb(
//     round(red(col1)*(1-amount) + red(col2)*amount),
//     round(green(col1)*(1-amount) + green(col2)*amount),
//     round(blue(col1)*(1-amount) + blue(col2)*amount),
//   )
// ---------------------------------------------------------------------------

fn attach_body_merge_color(module: &mut Module) {
    module.attach_runtime_body(
        "merge_color",
        &[Type::Float(64), Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let col1 = b.param(0);
            let col2 = b.param(1);
            let amt = b.param(2);

            let one = b.const_float(1.0);
            let one_minus_amt = b.sub(one, amt);

            // Red channel
            let r1 = b.call_named("color_get_red", &[col1], Type::Float(64));
            let r2 = b.call_named("color_get_red", &[col2], Type::Float(64));
            let r1_part = b.mul(r1, one_minus_amt);
            let r2_part = b.mul(r2, amt);
            let r_blend = b.add(r1_part, r2_part);
            let r_out = b.call_named("round_f64", &[r_blend], Type::Float(64));

            // Green channel
            let g1 = b.call_named("color_get_green", &[col1], Type::Float(64));
            let g2 = b.call_named("color_get_green", &[col2], Type::Float(64));
            let g1_part = b.mul(g1, one_minus_amt);
            let g2_part = b.mul(g2, amt);
            let g_blend = b.add(g1_part, g2_part);
            let g_out = b.call_named("round_f64", &[g_blend], Type::Float(64));

            // Blue channel
            let b1 = b.call_named("color_get_blue", &[col1], Type::Float(64));
            let b2 = b.call_named("color_get_blue", &[col2], Type::Float(64));
            let b1_part = b.mul(b1, one_minus_amt);
            let b2_part = b.mul(b2, amt);
            let b_blend = b.add(b1_part, b2_part);
            let bv_out = b.call_named("round_f64", &[b_blend], Type::Float(64));

            let result = b.call_named("make_color_rgb", &[r_out, g_out, bv_out], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// color_get_value(color: f64) -> f64
//
// Returns the HSV "value" (brightness) of a GML BGR color in the range 0–255:
//   r = (color & 0xFF) / 255
//   g = ((color >> 8) & 0xFF) / 255
//   b = (color >> 16) / 255
//   round(max(r, g, b) * 255)
// ---------------------------------------------------------------------------

fn attach_body_color_get_value(module: &mut Module) {
    module.attach_runtime_body(
        "color_get_value",
        &[Type::Float(64)],
        Type::Float(64),
        |b| {
            let color = b.param(0);

            let c255 = b.const_float(255.0);

            let r_raw = b.call_named("color_get_red", &[color], Type::Float(64));
            let g_raw = b.call_named("color_get_green", &[color], Type::Float(64));
            let bv_raw = b.call_named("color_get_blue", &[color], Type::Float(64));
            let r = b.div(r_raw, c255);
            let g = b.div(g_raw, c255);
            let bv = b.div(bv_raw, c255);

            let max_rg = b.call_named("max_f64", &[r, g], Type::Float(64));
            let max_rgb = b.call_named("max_f64", &[max_rg, bv], Type::Float(64));

            let scaled = b.mul(max_rgb, c255);
            let result = b.call_named("round_f64", &[scaled], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// color_get_saturation(color: f64) -> f64
//
// Returns the HSV saturation in 0–255:
//   r = (color & 0xFF) / 255
//   g = ((color >> 8) & 0xFF) / 255
//   b = (color >> 16) / 255
//   max = max(r, g, b);  min = min(r, g, b)
//   if max <= 0 { return 0 }   [max >= 0 always, so this equals max === 0]
//   round(((max - min) / max) * 255)
// ---------------------------------------------------------------------------

fn attach_body_color_get_saturation(module: &mut Module) {
    module.attach_runtime_body(
        "color_get_saturation",
        &[Type::Float(64)],
        Type::Float(64),
        |b| {
            let color = b.param(0);

            let c255 = b.const_float(255.0);
            let zero = b.const_float(0.0);

            let r_raw = b.call_named("color_get_red", &[color], Type::Float(64));
            let g_raw = b.call_named("color_get_green", &[color], Type::Float(64));
            let bv_raw = b.call_named("color_get_blue", &[color], Type::Float(64));
            let r = b.div(r_raw, c255);
            let g = b.div(g_raw, c255);
            let bv = b.div(bv_raw, c255);

            let max_rg = b.call_named("max_f64", &[r, g], Type::Float(64));
            let max_rgb = b.call_named("max_f64", &[max_rg, bv], Type::Float(64));
            let min_rg = b.call_named("min_f64", &[r, g], Type::Float(64));
            let min_rgb = b.call_named("min_f64", &[min_rg, bv], Type::Float(64));

            // if max <= 0 { return 0 }  (max >= 0 always, so this equals max === 0)
            let max_le_zero = b.cmp(CmpKind::Le, max_rgb, zero);
            let ret_zero_block = b.create_block();
            let cont_block = b.create_block();
            b.br_if(max_le_zero, ret_zero_block, &[], cont_block, &[]);

            b.switch_to_block(ret_zero_block);
            b.ret(Some(zero));

            b.switch_to_block(cont_block);
            let d = b.sub(max_rgb, min_rgb);
            let sat = b.div(d, max_rgb);
            let scaled = b.mul(sat, c255);
            let result = b.call_named("round_f64", &[scaled], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// color_get_hue(color: f64) -> f64
//
// Returns the HSV hue in 0–255 (GML maps 0–360° to 0–255).
//
//   r = (color & 0xFF) / 255
//   g = ((color >> 8) & 0xFF) / 255
//   b = (color >> 16) / 255
//   max = max(r, g, b);  min = min(r, g, b);  d = max - min
//   if d <= 0 { return 0 }
//   // 3-way select (avoid CmpKind::Eq on floats derived from bitwise ops):
//   if r >= g && r >= b: h = ((g - b) / d) % 6
//   else if g >= b:      h = (b - r) / d + 2
//   else:                h = (r - g) / d + 4
//   h = round(h * 255 / 6)
//   if h < 0 { h += 255 }
//   return h
// ---------------------------------------------------------------------------

fn attach_body_color_get_hue(module: &mut Module) {
    module.attach_runtime_body("color_get_hue", &[Type::Float(64)], Type::Float(64), |b| {
        let color = b.param(0);

        let c255 = b.const_float(255.0);
        let zero = b.const_float(0.0);

        // Extract r, g, b as fractions in [0, 1].
        let r_raw = b.call_named("color_get_red", &[color], Type::Float(64));
        let g_raw = b.call_named("color_get_green", &[color], Type::Float(64));
        let bv_raw = b.call_named("color_get_blue", &[color], Type::Float(64));
        let r = b.div(r_raw, c255);
        let g = b.div(g_raw, c255);
        let bv = b.div(bv_raw, c255);

        let max_rg = b.call_named("max_f64", &[r, g], Type::Float(64));
        let max_rgb = b.call_named("max_f64", &[max_rg, bv], Type::Float(64));
        let min_rg = b.call_named("min_f64", &[r, g], Type::Float(64));
        let min_rgb = b.call_named("min_f64", &[min_rg, bv], Type::Float(64));
        let d = b.sub(max_rgb, min_rgb);

        // if d <= 0 { return 0 }  (d >= 0 always, so this equals d === 0)
        let d_le_zero = b.cmp(CmpKind::Le, d, zero);
        let ret_zero_block = b.create_block();
        let branch_r_check = b.create_block();
        b.br_if(d_le_zero, ret_zero_block, &[], branch_r_check, &[]);

        b.switch_to_block(ret_zero_block);
        b.ret(Some(zero));

        // 3-way branch: is r the maximum channel?
        b.switch_to_block(branch_r_check);
        let r_ge_g = b.cmp(CmpKind::Ge, r, g);
        let r_ge_b = b.cmp(CmpKind::Ge, r, bv);
        let r_is_max = b.call_named("and_bool", &[r_ge_g, r_ge_b], Type::Bool);

        let (merge_block, h_params) = b.create_block_with_params(&[Type::Float(64)]);
        let block_r = b.create_block();
        let block_not_r = b.create_block();
        b.br_if(r_is_max, block_r, &[], block_not_r, &[]);

        // Branch: r is max → h = ((g - b) / d) % 6
        b.switch_to_block(block_r);
        let c6 = b.const_float(6.0);
        let g_minus_b = b.sub(g, bv);
        let h_r_raw = b.div(g_minus_b, d);
        let h_r = b.rem(h_r_raw, c6);
        b.br(merge_block, &[h_r]);

        // Branch: r is not max — is g the max? (check g >= b)
        b.switch_to_block(block_not_r);
        let g_ge_b = b.cmp(CmpKind::Ge, g, bv);
        let block_g = b.create_block();
        let block_bv = b.create_block();
        b.br_if(g_ge_b, block_g, &[], block_bv, &[]);

        // Branch: g is max → h = (b - r) / d + 2
        b.switch_to_block(block_g);
        let two = b.const_float(2.0);
        let bv_minus_r = b.sub(bv, r);
        let h_g_div = b.div(bv_minus_r, d);
        let h_g = b.add(h_g_div, two);
        b.br(merge_block, &[h_g]);

        // Branch: b is max → h = (r - g) / d + 4
        b.switch_to_block(block_bv);
        let four = b.const_float(4.0);
        let r_minus_g = b.sub(r, g);
        let h_bv_div = b.div(r_minus_g, d);
        let h_bv = b.add(h_bv_div, four);
        b.br(merge_block, &[h_bv]);

        // Merge: h_raw is the block param from whichever branch.
        b.switch_to_block(merge_block);
        let h_raw = h_params[0];

        // h = round(h_raw * 255 / 6)
        let c255_over_6 = b.const_float(255.0 / 6.0);
        let h_scaled = b.mul(h_raw, c255_over_6);
        let h_rounded = b.call_named("round_f64", &[h_scaled], Type::Float(64));

        // if h < 0 { h += 255 }
        let h_lt_zero = b.cmp(CmpKind::Lt, h_rounded, zero);
        let (final_block, final_params) = b.create_block_with_params(&[Type::Float(64)]);
        let add_255_block = b.create_block();
        b.br_if(h_lt_zero, add_255_block, &[], final_block, &[h_rounded]);

        b.switch_to_block(add_255_block);
        let h_plus_255 = b.add(h_rounded, c255);
        b.br(final_block, &[h_plus_255]);

        b.switch_to_block(final_block);
        let result = final_params[0];
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// make_color_hsv(h: f64, s: f64, v: f64) -> f64
//
// Converts HSV (0–255 each) to a GML BGR color.
//   hf = (h / 255) * 6    — hue in [0, 6)
//   sf = s / 255
//   vf = v / 255
//   c  = vf * sf           — chroma
//   x  = c * (1 - |hf % 2 - 1|)
//   m  = vf - c
//   6-way branch on hf:
//     hf < 1 → (r=c, g=x, b=0)
//     hf < 2 → (r=x, g=c, b=0)
//     hf < 3 → (r=0, g=c, b=x)
//     hf < 4 → (r=0, g=x, b=c)
//     hf < 5 → (r=x, g=0, b=c)
//     else   → (r=c, g=0, b=x)
//   return make_color_rgb(round((r+m)*255), round((g+m)*255), round((b+m)*255))
// ---------------------------------------------------------------------------

fn attach_body_make_color_hsv(module: &mut Module) {
    module.attach_runtime_body(
        "make_color_hsv",
        &[Type::Float(64), Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let h = b.param(0);
            let s = b.param(1);
            let v = b.param(2);

            let c255 = b.const_float(255.0);
            let c6 = b.const_float(6.0);

            // Normalize inputs.
            let h_div = b.div(h, c255);
            let hf = b.mul(h_div, c6);
            let sf = b.div(s, c255);
            let vf = b.div(v, c255);

            // c = vf * sf;  x = c * (1 - |hf % 2 - 1|);  m = vf - c
            let cv = b.mul(vf, sf);
            let c2_hsv = b.const_float(2.0);
            let hf_mod2 = b.rem(hf, c2_hsv);
            let c1_hsv = b.const_float(1.0);
            let hf_mod2_m1 = b.sub(hf_mod2, c1_hsv);
            let abs_val = b.call_named("abs_f64", &[hf_mod2_m1], Type::Float(64));
            let c1_hsv2 = b.const_float(1.0);
            let one_minus_abs = b.sub(c1_hsv2, abs_val);
            let x = b.mul(cv, one_minus_abs);
            let m = b.sub(vf, cv);

            // Merge block collects (r, g, b) from the 6 branches.
            let (merge_block, rgb_params) =
                b.create_block_with_params(&[Type::Float(64), Type::Float(64), Type::Float(64)]);

            let c1 = b.const_float(1.0);
            let c2 = b.const_float(2.0);
            let c3 = b.const_float(3.0);
            let c4 = b.const_float(4.0);
            let c5 = b.const_float(5.0);
            let zero = b.const_float(0.0);

            // hf < 1?
            let hf_lt1 = b.cmp(CmpKind::Lt, hf, c1);
            let blk0 = b.create_block();
            let blk_ge1 = b.create_block();
            b.br_if(hf_lt1, blk0, &[], blk_ge1, &[]);

            // Branch 0: hf < 1 → r=c, g=x, b=0
            b.switch_to_block(blk0);
            b.br(merge_block, &[cv, x, zero]);

            // hf >= 1: hf < 2?
            b.switch_to_block(blk_ge1);
            let hf_lt2 = b.cmp(CmpKind::Lt, hf, c2);
            let blk1 = b.create_block();
            let blk_ge2 = b.create_block();
            b.br_if(hf_lt2, blk1, &[], blk_ge2, &[]);

            // Branch 1: 1 <= hf < 2 → r=x, g=c, b=0
            b.switch_to_block(blk1);
            b.br(merge_block, &[x, cv, zero]);

            // hf >= 2: hf < 3?
            b.switch_to_block(blk_ge2);
            let hf_lt3 = b.cmp(CmpKind::Lt, hf, c3);
            let blk2 = b.create_block();
            let blk_ge3 = b.create_block();
            b.br_if(hf_lt3, blk2, &[], blk_ge3, &[]);

            // Branch 2: 2 <= hf < 3 → r=0, g=c, b=x
            b.switch_to_block(blk2);
            b.br(merge_block, &[zero, cv, x]);

            // hf >= 3: hf < 4?
            b.switch_to_block(blk_ge3);
            let hf_lt4 = b.cmp(CmpKind::Lt, hf, c4);
            let blk3 = b.create_block();
            let blk_ge4 = b.create_block();
            b.br_if(hf_lt4, blk3, &[], blk_ge4, &[]);

            // Branch 3: 3 <= hf < 4 → r=0, g=x, b=c
            b.switch_to_block(blk3);
            b.br(merge_block, &[zero, x, cv]);

            // hf >= 4: hf < 5?
            b.switch_to_block(blk_ge4);
            let hf_lt5 = b.cmp(CmpKind::Lt, hf, c5);
            let blk4 = b.create_block();
            let blk5 = b.create_block();
            b.br_if(hf_lt5, blk4, &[], blk5, &[]);

            // Branch 4: 4 <= hf < 5 → r=x, g=0, b=c
            b.switch_to_block(blk4);
            b.br(merge_block, &[x, zero, cv]);

            // Branch 5: hf >= 5 → r=c, g=0, b=x
            b.switch_to_block(blk5);
            b.br(merge_block, &[cv, zero, x]);

            // Merge: apply m offset and pack into BGR.
            b.switch_to_block(merge_block);
            let r_out = rgb_params[0];
            let g_out = rgb_params[1];
            let bv_out = rgb_params[2];

            let r_plus_m = b.add(r_out, m);
            let r_scaled = b.mul(r_plus_m, c255);
            let r_final = b.call_named("round_f64", &[r_scaled], Type::Float(64));
            let g_plus_m = b.add(g_out, m);
            let g_scaled = b.mul(g_plus_m, c255);
            let g_final = b.call_named("round_f64", &[g_scaled], Type::Float(64));
            let bv_plus_m = b.add(bv_out, m);
            let bv_scaled = b.mul(bv_plus_m, c255);
            let b_final = b.call_named("round_f64", &[bv_scaled], Type::Float(64));

            let result = b.call_named(
                "make_color_rgb",
                &[r_final, g_final, b_final],
                Type::Float(64),
            );
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_length(s: String) -> Float(64)  =  s.length
// ---------------------------------------------------------------------------

fn attach_body_string_length(module: &mut Module) {
    module.attach_runtime_body("string_length", &[Type::String], Type::Float(64), |b| {
        let s = b.param(0);
        let result = b.call_named("string_length_str", &[s], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// string_upper(s: String) -> String  =  s.toUpperCase()
// ---------------------------------------------------------------------------

fn attach_body_string_upper(module: &mut Module) {
    module.attach_runtime_body("string_upper", &[Type::String], Type::String, |b| {
        let s = b.param(0);
        let result = b.call_named("string_upper_str", &[s], Type::String);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// string_lower(s: String) -> String  =  s.toLowerCase()
// ---------------------------------------------------------------------------

fn attach_body_string_lower(module: &mut Module) {
    module.attach_runtime_body("string_lower", &[Type::String], Type::String, |b| {
        let s = b.param(0);
        let result = b.call_named("string_lower_str", &[s], Type::String);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// string_char_at(s: String, index: Float(64)) -> String
//   GML is 1-based: s.charAt(index - 1)
// ---------------------------------------------------------------------------

fn attach_body_string_char_at(module: &mut Module) {
    module.attach_runtime_body(
        "string_char_at",
        &[Type::String, Type::Float(64)],
        Type::String,
        |b| {
            let s = b.param(0);
            let index = b.param(1);

            let one = b.const_float(1.0);
            let idx_zero = b.sub(index, one);
            let result = b.call_named("string_char_at_str", &[s, idx_zero], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_copy(s: String, index: Float(64), count: Float(64)) -> String
//   GML is 1-based: s.slice(index-1, index-1+count)
// ---------------------------------------------------------------------------

fn attach_body_string_copy(module: &mut Module) {
    module.attach_runtime_body(
        "string_copy",
        &[Type::String, Type::Float(64), Type::Float(64)],
        Type::String,
        |b| {
            let s = b.param(0);
            let index = b.param(1);
            let count = b.param(2);

            let one = b.const_float(1.0);
            let start = b.sub(index, one); // index - 1  (0-based start)
            let end = b.add(start, count); // start + count  (0-based exclusive end)
            let result = b.call_named("string_slice_str", &[s, start, end], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_pos(substr: String, s: String) -> Float(64)
//   Returns 1-based position, 0 if not found.
//   JS indexOf returns 0-based (-1 if not found); adding 1 gives GML semantics:
//   -1 + 1 = 0 (not found), n + 1 = 1-based position.
// ---------------------------------------------------------------------------

fn attach_body_string_pos(module: &mut Module) {
    module.attach_runtime_body(
        "string_pos",
        &[Type::String, Type::String],
        Type::Float(64),
        |b| {
            let substr = b.param(0);
            let s = b.param(1);

            // string_index_of_str(needle, haystack) -> 0-based index or -1
            let idx = b.call_named("string_index_of_str", &[substr, s], Type::Float(64));
            let one = b.const_float(1.0);
            let result = b.add(idx, one);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_delete(s: String, index: Float(64), count: Float(64)) -> String
//   s.slice(0, index-1) + s.slice(index-1+count, length)
// ---------------------------------------------------------------------------

fn attach_body_string_delete(module: &mut Module) {
    module.attach_runtime_body(
        "string_delete",
        &[Type::String, Type::Float(64), Type::Float(64)],
        Type::String,
        |b| {
            let s = b.param(0);
            let index = b.param(1);
            let count = b.param(2);

            let zero = b.const_float(0.0);
            let one = b.const_float(1.0);
            let idx_minus1 = b.sub(index, one); // index - 1  (0-based)
            let tail_start = b.add(idx_minus1, count); // index - 1 + count
            let len = b.call_named("string_length_str", &[s], Type::Float(64));

            let head = b.call_named("string_slice_str", &[s, zero, idx_minus1], Type::String);
            let tail = b.call_named("string_slice_str", &[s, tail_start, len], Type::String);
            let result = b.call_named("concat_str", &[head, tail], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_insert(substr: String, s: String, index: Float(64)) -> String
//   s.slice(0, index-1) + substr + s.slice(index-1)
// ---------------------------------------------------------------------------

fn attach_body_string_insert(module: &mut Module) {
    module.attach_runtime_body(
        "string_insert",
        &[Type::String, Type::String, Type::Float(64)],
        Type::String,
        |b| {
            let substr = b.param(0);
            let s = b.param(1);
            let index = b.param(2);

            let zero = b.const_float(0.0);
            let one = b.const_float(1.0);
            let idx_minus1 = b.sub(index, one); // index - 1  (0-based)
            let len = b.call_named("string_length_str", &[s], Type::Float(64));

            let head = b.call_named("string_slice_str", &[s, zero, idx_minus1], Type::String);
            let tail = b.call_named("string_slice_str", &[s, idx_minus1, len], Type::String);
            // head + substr + tail  (two string concatenations)
            let head_sub = b.call_named("concat_str", &[head, substr], Type::String);
            let result = b.call_named("concat_str", &[head_sub, tail], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_replace_all(str: String, substr: String, newstr: String) -> String
//   str.split(substr).join(newstr)
// ---------------------------------------------------------------------------

fn attach_body_string_replace_all(module: &mut Module) {
    module.attach_runtime_body(
        "string_replace_all",
        &[Type::String, Type::String, Type::String],
        Type::String,
        |b| {
            let content = b.param(0);
            let find = b.param(1);
            let replace = b.param(2);

            let arr_ty = Type::Array(Box::new(Type::String));
            let parts = b.call_named("string_split_str", &[content, find], arr_ty);
            let result = b.call_named("string_join_arr", &[parts, replace], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_count(substr: String, s: String) -> Float(64)
//   s.split(substr).length - 1
// ---------------------------------------------------------------------------

fn attach_body_string_count(module: &mut Module) {
    module.attach_runtime_body(
        "string_count",
        &[Type::String, Type::String],
        Type::Float(64),
        |b| {
            let substr = b.param(0);
            let s = b.param(1);

            let arr_ty = Type::Array(Box::new(Type::String));
            let parts = b.call_named("string_split_str", &[s, substr], arr_ty);
            let len = b.get_field(parts, "length", Type::Float(64));
            let one = b.const_float(1.0);
            let result = b.sub(len, one);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_ord_at(str: String, index: Float(64)) -> Float(64)
//   GML: char code at 1-based index
//   JS equivalent: str.charCodeAt(index - 1)
// ---------------------------------------------------------------------------

fn attach_body_string_ord_at(module: &mut Module) {
    module.attach_runtime_body(
        "string_ord_at",
        &[Type::String, Type::Float(64)],
        Type::Float(64),
        |b| {
            let s = b.param(0);
            let idx = b.param(1);

            let one = b.const_float(1.0);
            let idx0 = b.sub(idx, one); // convert 1-based GML index to 0-based JS
            let result = b.call_named("string_char_code_at_str", &[s, idx0], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_repeat(str: String, count: Float(64)) -> String
//   JS: str.repeat(count)
// ---------------------------------------------------------------------------

fn attach_body_string_repeat(module: &mut Module) {
    module.attach_runtime_body(
        "string_repeat",
        &[Type::String, Type::Float(64)],
        Type::String,
        |b| {
            let s = b.param(0);
            let n = b.param(1);
            let result = b.call_named("string_repeat_str", &[s, n], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_replace(str: String, substr: String, newstr: String) -> String
//   GML: replaces FIRST occurrence only
//   JS: str.replace(substr, newstr)
// ---------------------------------------------------------------------------

fn attach_body_string_replace(module: &mut Module) {
    module.attach_runtime_body(
        "string_replace",
        &[Type::String, Type::String, Type::String],
        Type::String,
        |b| {
            let s = b.param(0);
            let sub = b.param(1);
            let new = b.param(2);
            let result = b.call_named("string_replace_first_str", &[s, sub, new], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_hash_to_newline(string: String) -> String
//   GML: replaces '#' with newline
//   JS: str.split('#').join('\n')
// ---------------------------------------------------------------------------

fn attach_body_string_hash_to_newline(module: &mut Module) {
    module.attach_runtime_body(
        "string_hash_to_newline",
        &[Type::String],
        Type::String,
        |b| {
            let s = b.param(0);
            let hash = b.const_string("#");
            let newline = b.const_string("\n");
            let arr_ty = Type::Array(Box::new(Type::String));
            let parts = b.call_named("string_split_str", &[s, hash], arr_ty);
            let result = b.call_named("string_join_arr", &[parts, newline], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_trim(str: String, substr: Unknown) -> String
//   GML: trims whitespace (optional 2nd param ignored for whitespace-trim form)
//   JS: str.trim()
// ---------------------------------------------------------------------------

fn attach_body_string_trim(module: &mut Module) {
    module.attach_runtime_body(
        "string_trim",
        &[Type::String, Type::Value],
        Type::String,
        |b| {
            let s = b.param(0);
            b.param(1); // substr — unused in whitespace-trim form
            let result = b.call_named("string_trim_str", &[s], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// array_length(array: Array(Unknown)) -> Float(64)
//   JS: arr.length
// ---------------------------------------------------------------------------

fn attach_body_array_length(module: &mut Module) {
    module.attach_runtime_body(
        "array_length",
        &[Type::Array(Box::new(Type::Value))],
        Type::Float(64),
        |b| {
            let arr = b.param(0);
            let result = b.call_named("array_length_arr", &[arr], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// array_length_1d(array: Array(Unknown)) -> Float(64)
//   Alias for array_length; identical behaviour.
//   JS: arr.length
// ---------------------------------------------------------------------------

fn attach_body_array_length_1d(module: &mut Module) {
    module.attach_runtime_body(
        "array_length_1d",
        &[Type::Array(Box::new(Type::Value))],
        Type::Float(64),
        |b| {
            let arr = b.param(0);
            let result = b.call_named("array_length_arr", &[arr], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// array_contains(array: Array(Unknown), value: Unknown) -> Bool
//   GML: checks if value is in array
//   JS: arr.includes(value)
// ---------------------------------------------------------------------------

fn attach_body_array_contains(module: &mut Module) {
    module.attach_runtime_body(
        "array_contains",
        &[Type::Array(Box::new(Type::Value)), Type::Value],
        Type::Bool,
        |b| {
            let arr = b.param(0);
            let val = b.param(1);
            let result = b.call_named("array_contains_arr", &[arr, val], Type::Bool);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// sin(x: f64) -> f64  — radian version
//   JS: Math.sin(x)
// ---------------------------------------------------------------------------

fn attach_body_sin(module: &mut Module) {
    module.attach_runtime_body("sin", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("sin_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// cos(x: f64) -> f64  — radian version
//   JS: Math.cos(x)
// ---------------------------------------------------------------------------

fn attach_body_cos(module: &mut Module) {
    module.attach_runtime_body("cos", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("cos_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// tan(x: f64) -> f64  — radian version
//   JS: Math.tan(x)
// ---------------------------------------------------------------------------

fn attach_body_tan(module: &mut Module) {
    module.attach_runtime_body("tan", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("tan_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// arcsin(x: f64) -> f64  — radian version
//   JS: Math.asin(x)
// ---------------------------------------------------------------------------

fn attach_body_arcsin(module: &mut Module) {
    module.attach_runtime_body("arcsin", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("asin_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// arccos(x: f64) -> f64  — radian version
//   JS: Math.acos(x)
// ---------------------------------------------------------------------------

fn attach_body_arccos(module: &mut Module) {
    module.attach_runtime_body("arccos", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("acos_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// ord(s: String) -> Float(64)
//   GML: returns the Unicode code point of the first character of s.
//   JS: s.charCodeAt(0)
// ---------------------------------------------------------------------------

fn attach_body_ord(module: &mut Module) {
    module.attach_runtime_body("ord", &[Type::String], Type::Float(64), |b| {
        let s = b.param(0);
        let zero = b.const_float(0.0);
        let result = b.call_named("string_char_code_at_str", &[s, zero], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// string_byte_at(s: String, pos: Float(64)) -> Float(64)
//   GML: returns the byte value of the character at 1-based position pos.
//   JS: s.charCodeAt(pos - 1)
// ---------------------------------------------------------------------------

fn attach_body_string_byte_at(module: &mut Module) {
    module.attach_runtime_body(
        "string_byte_at",
        &[Type::String, Type::Float(64)],
        Type::Float(64),
        |b| {
            let s = b.param(0);
            let pos = b.param(1);

            let one = b.const_float(1.0);
            let pos_minus_1 = b.sub(pos, one); // convert 1-based GML index to 0-based JS
                                               // string_byte_at_rt emits `str.charCodeAt(pos0) || 0`; the || 0 maps
                                               // charCodeAt's NaN (out-of-range) to the GML-specified 0 return value.
            let result = b.call_named("string_byte_at_rt", &[s, pos_minus_1], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// string_digits(s: String) -> String
//   GML: strips all non-digit characters from s.
//   JS: s.replace(/\D/g, "") via string_digits_rt backend primitive.
// ---------------------------------------------------------------------------

fn attach_body_string_digits(module: &mut Module) {
    module.attach_runtime_body("string_digits", &[Type::String], Type::String, |b| {
        let s = b.param(0);
        let result = b.call_named("string_digits_rt", &[s], Type::String);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// string_letters(s: String) -> String
//   GML: strips all non-letter characters from s.
//   JS: s.replace(/[^a-zA-Z]/g, "") via string_letters_rt backend primitive.
// ---------------------------------------------------------------------------

fn attach_body_string_letters(module: &mut Module) {
    module.attach_runtime_body("string_letters", &[Type::String], Type::String, |b| {
        let s = b.param(0);
        let result = b.call_named("string_letters_rt", &[s], Type::String);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// string_format(n: Float64, tot: Float64, dec: Float64) -> String
//   GML: format n with dec decimal places, padded to tot total width.
//   JS: (s => s.length < tot ? s.padStart(tot) : s)(n.toFixed(dec))
//       via string_format_rt backend primitive.
// ---------------------------------------------------------------------------

fn attach_body_string_format(module: &mut Module) {
    module.attach_runtime_body(
        "string_format",
        &[Type::Float(64), Type::Float(64), Type::Float(64)],
        Type::String,
        |b| {
            let n = b.param(0);
            let tot = b.param(1);
            let dec = b.param(2);
            let result = b.call_named("string_format_rt", &[n, tot, dec], Type::String);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// chr(n: Float(64)) -> String
//   GML: returns the character corresponding to Unicode code point n.
//   JS: String.fromCharCode(n)
// ---------------------------------------------------------------------------

fn attach_body_chr(module: &mut Module) {
    module.attach_runtime_body("chr", &[Type::Float(64)], Type::String, |b| {
        let n = b.param(0);
        let result = b.call_named("chr_f64", &[n], Type::String);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// ln(x: f64) -> f64  =  ln_f64(x)
// ---------------------------------------------------------------------------

fn attach_body_ln(module: &mut Module) {
    module.attach_runtime_body("ln", &[Type::Float(64)], Type::Float(64), |b| {
        let x = b.param(0);
        let result = b.call_named("ln_f64", &[x], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// math_get_epsilon() -> f64  =  0.00001
// ---------------------------------------------------------------------------

fn attach_body_math_get_epsilon(module: &mut Module) {
    module.attach_runtime_body("math_get_epsilon", &[], Type::Float(64), |b| {
        let result = b.const_float(0.00001);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_nan(x: Float64) -> Bool
// GML semantics: true iff x is IEEE 754 NaN.
// JS emit: Number.isNaN(x)
// ---------------------------------------------------------------------------

fn attach_body_is_nan(module: &mut Module) {
    module.attach_runtime_body("is_nan", &[Type::Float(64)], Type::Bool, |b| {
        // NaN is the only value not equal to itself: x !== x
        let x = b.param(0);
        let eq = b.cmp(CmpKind::Eq, x, x);
        let result = b.not(eq);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_infinity(x: Float64) -> Bool
// GML semantics: true iff x is +Infinity or -Infinity.
// JS emit: !Number.isFinite(x) && !Number.isNaN(x)
// ---------------------------------------------------------------------------

fn attach_body_is_infinity(module: &mut Module) {
    module.attach_runtime_body("is_infinity", &[Type::Float(64)], Type::Bool, |b| {
        let x = b.param(0);
        let pos_inf = b.const_float(f64::INFINITY);
        let neg_inf = b.const_float(f64::NEG_INFINITY);
        let is_pos = b.cmp(CmpKind::Eq, x, pos_inf);
        let is_neg = b.cmp(CmpKind::Eq, x, neg_inf);
        let result = b.bool_or(is_pos, is_neg);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_bool(x: Unknown) -> Bool  =  type_check(x, Bool)
// ---------------------------------------------------------------------------

fn attach_body_is_bool(module: &mut Module) {
    module.attach_runtime_body("is_bool", &[Type::Value], Type::Bool, |b| {
        let x = b.param(0);
        let result = b.type_check(x, Type::Bool);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_real(x: Unknown) -> Bool  =  type_check(x, Float(64))
// JS semantics: typeof val === "number"
// ---------------------------------------------------------------------------

fn attach_body_is_real(module: &mut Module) {
    module.attach_runtime_body("is_real", &[Type::Value], Type::Bool, |b| {
        let x = b.param(0);
        let result = b.type_check(x, Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_string(x: Unknown) -> Bool  =  type_check(x, String)
// JS semantics: typeof val === "string"
// ---------------------------------------------------------------------------

fn attach_body_is_string(module: &mut Module) {
    module.attach_runtime_body("is_string", &[Type::Value], Type::Bool, |b| {
        let x = b.param(0);
        let result = b.type_check(x, Type::String);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_undefined(x: Unknown) -> Bool  =  type_check(x, Void)
// JS semantics: typeof val === "undefined"
// ---------------------------------------------------------------------------

fn attach_body_is_undefined(module: &mut Module) {
    module.attach_runtime_body("is_undefined", &[Type::Value], Type::Bool, |b| {
        let x = b.param(0);
        let result = b.type_check(x, Type::Void);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_array(x: Unknown) -> Bool  =  type_check(x, Array(Unknown))
// JS semantics: Array.isArray(val)
// ---------------------------------------------------------------------------

fn attach_body_is_array(module: &mut Module) {
    module.attach_runtime_body("is_array", &[Type::Value], Type::Bool, |b| {
        let x = b.param(0);
        let result = b.type_check(x, Type::Array(Box::new(Type::Value)));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_method(x: Unknown) -> Bool  =  type_check(x, Function(...))
// JS semantics: typeof val === "function"
// ---------------------------------------------------------------------------

fn attach_body_is_method(module: &mut Module) {
    module.attach_runtime_body("is_method", &[Type::Value], Type::Bool, |b| {
        let x = b.param(0);
        // Use a zero-param function type as the representative function type.
        // The backend dispatches on `Type::Function(_)` and emits `typeof x === "function"`.
        let fn_ty = Type::Function(Box::new(FunctionSig {
            params: vec![],
            return_ty: Type::Value,
            defaults: vec![],
            has_rest_param: false,
            param_lower_bounds: vec![],
        }));
        let result = b.type_check(x, fn_ty);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_struct(x: Unknown) -> Bool  =  is_struct_unknown(x)
// JS semantics: typeof val === "object" && val != null && !Array.isArray(val)
// ---------------------------------------------------------------------------

fn attach_body_is_struct(module: &mut Module) {
    module.attach_runtime_body("is_struct", &[Type::Value], Type::Bool, |b| {
        let x = b.param(0);
        let result = b.call_named("is_struct_unknown", &[x], Type::Bool);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// is_numeric(x: Unknown) -> Bool  =  is_numeric_unknown(x)
// JS semantics: !isNaN(Number(val))
// ---------------------------------------------------------------------------

fn attach_body_is_numeric(module: &mut Module) {
    module.attach_runtime_body("is_numeric", &[Type::Value], Type::Bool, |b| {
        let x = b.param(0);
        let result = b.call_named("is_numeric_unknown", &[x], Type::Bool);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// _typeof(x: Unknown) -> String  =  typeof_gml(x)
// JS semantics: GML type name string (ternary chain)
// ---------------------------------------------------------------------------

fn attach_body_typeof(module: &mut Module) {
    module.attach_runtime_body("_typeof", &[Type::Value], Type::String, |b| {
        let x = b.param(0);
        let result = b.call_named("typeof_gml", &[x], Type::String);
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// real(s: String) -> Float(64)  =  to_number_str(s)
// ---------------------------------------------------------------------------

fn attach_body_real(module: &mut Module) {
    module.attach_runtime_body("real", &[Type::String], Type::Float(64), |b| {
        let s = b.param(0);
        let result = b.call_named("to_number_str", &[s], Type::Float(64));
        b.ret(Some(result));
    });
}

// ---------------------------------------------------------------------------
// pass() -> void   GML no-op statement
// ---------------------------------------------------------------------------

fn attach_body_pass(module: &mut Module) {
    module.attach_runtime_body("pass", &[], Type::Void, |b| {
        b.ret(None);
    });
}

// ---------------------------------------------------------------------------
// __try_hook__(begin: f64, end: f64) -> void   GML no-op
// ---------------------------------------------------------------------------

fn attach_body_try_hook(module: &mut Module) {
    module.attach_runtime_body(
        "__try_hook__",
        &[Type::Float(64), Type::Float(64)],
        Type::Void,
        |b| {
            b.param(0);
            b.param(1);
            b.ret(None);
        },
    );
}

// ---------------------------------------------------------------------------
// __try_unhook__() -> void   GML no-op
// ---------------------------------------------------------------------------

fn attach_body_try_unhook(module: &mut Module) {
    module.attach_runtime_body("__try_unhook__", &[], Type::Void, |b| {
        b.ret(None);
    });
}

// ---------------------------------------------------------------------------
// approach(value: f64, target: f64, amount: f64) -> f64
//   if value < target: min(value + amount, target)
//   else:              max(value - amount, target)
// ---------------------------------------------------------------------------

fn attach_body_approach(module: &mut Module) {
    module.attach_runtime_body(
        "approach",
        &[Type::Float(64), Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let value = b.param(0);
            let target = b.param(1);
            let amount = b.param(2);

            let (merge_block, merge_params) = b.create_block_with_params(&[Type::Float(64)]);
            let lt_block = b.create_block();
            let ge_block = b.create_block();

            let val_lt_target = b.cmp(CmpKind::Lt, value, target);
            b.br_if(val_lt_target, lt_block, &[], ge_block, &[]);

            // value < target: min(value + amount, target)
            b.switch_to_block(lt_block);
            let added = b.add(value, amount);
            let clamped_up = b.call_named("min_f64", &[added, target], Type::Float(64));
            b.br(merge_block, &[clamped_up]);

            // value >= target: max(value - amount, target)
            b.switch_to_block(ge_block);
            let subbed = b.sub(value, amount);
            let clamped_down = b.call_named("max_f64", &[subbed, target], Type::Float(64));
            b.br(merge_block, &[clamped_down]);

            b.switch_to_block(merge_block);
            b.ret(Some(merge_params[0]));
        },
    );
}

// ---------------------------------------------------------------------------
// angle_difference(a: f64, b: f64) -> f64
//   ((((a - b) % 360) + 540) % 360) - 180
// ---------------------------------------------------------------------------

fn attach_body_angle_difference(module: &mut Module) {
    module.attach_runtime_body(
        "angle_difference",
        &[Type::Float(64), Type::Float(64)],
        Type::Float(64),
        |b| {
            let a = b.param(0);
            let bv = b.param(1);
            let c360 = b.const_float(360.0);
            let c540 = b.const_float(540.0);
            let c180 = b.const_float(180.0);
            let diff = b.sub(a, bv);
            let mod1 = b.rem(diff, c360);
            let plus540 = b.add(mod1, c540);
            let mod2 = b.rem(plus540, c360);
            let result = b.sub(mod2, c180);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// rectangle_in_rectangle(sx1,sy1,sx2,sy2, dx1,dy1,dx2,dy2: f64) -> f64
//   0 = no overlap, 1 = partial, 2 = fully inside
//
//   if sx2 < dx1 || sx1 > dx2 || sy2 < dy1 || sy1 > dy2 → 0
//   if sx1 >= dx1 && sx2 <= dx2 && sy1 >= dy1 && sy2 <= dy2 → 2
//   else → 1
// ---------------------------------------------------------------------------

fn attach_body_rectangle_in_rectangle(module: &mut Module) {
    module.attach_runtime_body(
        "rectangle_in_rectangle",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Float(64),
        |b| {
            let sx1 = b.param(0);
            let sy1 = b.param(1);
            let sx2 = b.param(2);
            let sy2 = b.param(3);
            let dx1 = b.param(4);
            let dy1 = b.param(5);
            let dx2 = b.param(6);
            let dy2 = b.param(7);

            let zero = b.const_float(0.0);
            let one = b.const_float(1.0);
            let two = b.const_float(2.0);

            // No-overlap check: sx2 < dx1 || sx1 > dx2 || sy2 < dy1 || sy1 > dy2
            let sx2_lt_dx1 = b.cmp(CmpKind::Lt, sx2, dx1);
            let sx1_gt_dx2 = b.cmp(CmpKind::Gt, sx1, dx2);
            let sy2_lt_dy1 = b.cmp(CmpKind::Lt, sy2, dy1);
            let sy1_gt_dy2 = b.cmp(CmpKind::Gt, sy1, dy2);
            let no_x = b.call_named("or_bool", &[sx2_lt_dx1, sx1_gt_dx2], Type::Bool);
            let no_y = b.call_named("or_bool", &[sy2_lt_dy1, sy1_gt_dy2], Type::Bool);
            let no_overlap = b.call_named("or_bool", &[no_x, no_y], Type::Bool);

            let ret_zero_block = b.create_block();
            let check_inside_block = b.create_block();
            b.br_if(no_overlap, ret_zero_block, &[], check_inside_block, &[]);

            b.switch_to_block(ret_zero_block);
            b.ret(Some(zero));

            // Fully-inside check: sx1 >= dx1 && sx2 <= dx2 && sy1 >= dy1 && sy2 <= dy2
            b.switch_to_block(check_inside_block);
            let sx1_ge_dx1 = b.cmp(CmpKind::Ge, sx1, dx1);
            let sx2_le_dx2 = b.cmp(CmpKind::Le, sx2, dx2);
            let sy1_ge_dy1 = b.cmp(CmpKind::Ge, sy1, dy1);
            let sy2_le_dy2 = b.cmp(CmpKind::Le, sy2, dy2);
            let in_x = b.call_named("and_bool", &[sx1_ge_dx1, sx2_le_dx2], Type::Bool);
            let in_y = b.call_named("and_bool", &[sy1_ge_dy1, sy2_le_dy2], Type::Bool);
            let fully_inside = b.call_named("and_bool", &[in_x, in_y], Type::Bool);

            let ret_two_block = b.create_block();
            let ret_one_block = b.create_block();
            b.br_if(fully_inside, ret_two_block, &[], ret_one_block, &[]);

            b.switch_to_block(ret_two_block);
            b.ret(Some(two));

            b.switch_to_block(ret_one_block);
            b.ret(Some(one));
        },
    );
}

// ---------------------------------------------------------------------------
// matrix_build(x,y,z,xrot,yrot,zrot,xscale,yscale,zscale: f64) -> Array(f64)
//   Builds a 4×4 TRS matrix (column-major, 16 elements).
// ---------------------------------------------------------------------------

fn attach_body_matrix_build(module: &mut Module) {
    module.attach_runtime_body(
        "matrix_build",
        &[
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Array(Box::new(Type::Float(64))),
        |b| {
            let x = b.param(0);
            let y = b.param(1);
            let z = b.param(2);
            let xrotation = b.param(3);
            let yrotation = b.param(4);
            let zrotation = b.param(5);
            let xscale = b.param(6);
            let yscale = b.param(7);
            let zscale = b.param(8);

            let pi_over_180 = b.const_float(PI / 180.0);

            let xrad = b.mul(xrotation, pi_over_180);
            let yrad = b.mul(yrotation, pi_over_180);
            let zrad = b.mul(zrotation, pi_over_180);

            let cx = b.call_named("cos_f64", &[xrad], Type::Float(64));
            let sx = b.call_named("sin_f64", &[xrad], Type::Float(64));
            let cy = b.call_named("cos_f64", &[yrad], Type::Float(64));
            let sy = b.call_named("sin_f64", &[yrad], Type::Float(64));
            let cz = b.call_named("cos_f64", &[zrad], Type::Float(64));
            let sz = b.call_named("sin_f64", &[zrad], Type::Float(64));

            // Row 0: cy*cz*xscale, cy*sz*xscale, -sy*xscale, 0
            let cy_cz = b.mul(cy, cz);
            let r00 = b.mul(cy_cz, xscale);
            let cy_sz = b.mul(cy, sz);
            let r01 = b.mul(cy_sz, xscale);
            let neg_sy = b.neg(sy);
            let r02 = b.mul(neg_sy, xscale);
            let r03 = b.const_float(0.0);

            // Row 1: (sx*sy*cz - cx*sz)*yscale, (sx*sy*sz + cx*cz)*yscale, sx*cy*yscale, 0
            let sx_sy = b.mul(sx, sy);
            let sx_sy_cz = b.mul(sx_sy, cz);
            let cx_sz = b.mul(cx, sz);
            let r10_inner = b.sub(sx_sy_cz, cx_sz);
            let r10 = b.mul(r10_inner, yscale);
            let sx_sy_sz = b.mul(sx_sy, sz);
            let cx_cz = b.mul(cx, cz);
            let r11_inner = b.add(sx_sy_sz, cx_cz);
            let r11 = b.mul(r11_inner, yscale);
            let sx_cy = b.mul(sx, cy);
            let r12 = b.mul(sx_cy, yscale);
            let r13 = b.const_float(0.0);

            // Row 2: (cx*sy*cz + sx*sz)*zscale, (cx*sy*sz - sx*cz)*zscale, cx*cy*zscale, 0
            let cx_sy = b.mul(cx, sy);
            let cx_sy_cz = b.mul(cx_sy, cz);
            let sx_sz = b.mul(sx, sz);
            let r20_inner = b.add(cx_sy_cz, sx_sz);
            let r20 = b.mul(r20_inner, zscale);
            let cx_sy_sz = b.mul(cx_sy, sz);
            let sx_cz = b.mul(sx, cz);
            let r21_inner = b.sub(cx_sy_sz, sx_cz);
            let r21 = b.mul(r21_inner, zscale);
            let cx_cy = b.mul(cx, cy);
            let r22 = b.mul(cx_cy, zscale);
            let r23 = b.const_float(0.0);

            // Row 3: x, y, z, 1
            let r33 = b.const_float(1.0);

            let mat = b.array_init(
                &[
                    r00, r01, r02, r03, r10, r11, r12, r13, r20, r21, r22, r23, x, y, z, r33,
                ],
                Type::Float(64),
            );
            b.ret(Some(mat));
        },
    );
}

// ---------------------------------------------------------------------------
// array_copy(dest: Array(Unknown), destIndex: f64,
//            src: Array(Unknown), srcIndex: f64, count: f64) -> void
//   for i in 0..count: dest[destIndex + i] = src[srcIndex + i]
// ---------------------------------------------------------------------------

fn attach_body_array_copy(module: &mut Module) {
    module.attach_runtime_body(
        "array_copy",
        &[
            Type::Array(Box::new(Type::Value)),
            Type::Float(64),
            Type::Array(Box::new(Type::Value)),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Void,
        |b| {
            let dest = b.param(0);
            let dest_index = b.param(1);
            let src = b.param(2);
            let src_index = b.param(3);
            let count = b.param(4);

            let zero = b.const_float(0.0);
            let one = b.const_float(1.0);

            // header: if i >= count → exit; else → body
            let (header_block, header_params) = b.create_block_with_params(&[Type::Float(64)]);
            let body_block = b.create_block();
            let exit_block = b.create_block();

            b.br(header_block, &[zero]);

            b.switch_to_block(header_block);
            let i = header_params[0];
            let done = b.cmp(CmpKind::Ge, i, count);
            b.br_if(done, exit_block, &[], body_block, &[]);

            // body: dest[destIndex + i] = src[srcIndex + i]; i += 1 → header
            b.switch_to_block(body_block);
            let di = b.add(dest_index, i);
            let si = b.add(src_index, i);
            let val = b.get_index(src, si, Type::Value);
            b.set_index(dest, di, val);
            let next_i = b.add(i, one);
            b.br(header_block, &[next_i]);

            b.switch_to_block(exit_block);
            b.ret(None);
        },
    );
}

// ---------------------------------------------------------------------------
// array_equals(a: Array(Unknown), b: Array(Unknown)) -> Bool
//   if a.length != b.length: false
//   for i in 0..a.length: if a[i] != b[i]: false
//   true
// ---------------------------------------------------------------------------

fn attach_body_array_equals(module: &mut Module) {
    module.attach_runtime_body(
        "array_equals",
        &[
            Type::Array(Box::new(Type::Value)),
            Type::Array(Box::new(Type::Value)),
        ],
        Type::Bool,
        |b| {
            let a = b.param(0);
            let bv = b.param(1);

            let zero = b.const_float(0.0);
            let one = b.const_float(1.0);

            let len_a = b.call_named("array_length_arr", &[a], Type::Float(64));
            let len_b = b.call_named("array_length_arr", &[bv], Type::Float(64));
            let lengths_differ = b.cmp(CmpKind::Ne, len_a, len_b);

            let ret_false_block = b.create_block();
            let (header_block, header_params) = b.create_block_with_params(&[Type::Float(64)]);
            let body_block = b.create_block();
            let ret_true_block = b.create_block();

            // lengths differ → false; else → loop header
            b.br_if(lengths_differ, ret_false_block, &[], header_block, &[zero]);

            b.switch_to_block(ret_false_block);
            let fv = b.const_bool(false);
            b.ret(Some(fv));

            // loop header: if i >= len_a → true; else → body
            b.switch_to_block(header_block);
            let i = header_params[0];
            let loop_done = b.cmp(CmpKind::Ge, i, len_a);
            b.br_if(loop_done, ret_true_block, &[], body_block, &[]);

            // body: if a[i] != b[i] → false; else i += 1 → header
            b.switch_to_block(body_block);
            let ai = b.get_index(a, i, Type::Value);
            let bi = b.get_index(bv, i, Type::Value);
            let ne = b.cmp(CmpKind::Ne, ai, bi);
            let next_i = b.add(i, one);
            b.br_if(ne, ret_false_block, &[], header_block, &[next_i]);

            b.switch_to_block(ret_true_block);
            let tv = b.const_bool(true);
            b.ret(Some(tv));
        },
    );
}

// ---------------------------------------------------------------------------
// array_get(arr: Array(Unknown), index: f64) -> Unknown
//   GML: returns arr[index]
//   JS: arr[index]
// ---------------------------------------------------------------------------

fn attach_body_array_get(module: &mut Module) {
    module.attach_runtime_body(
        "array_get",
        &[Type::Array(Box::new(Type::Value)), Type::Float(64)],
        Type::Value,
        |b| {
            let arr = b.param(0);
            let index = b.param(1);
            let result = b.get_index(arr, index, Type::Value);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// array_set(arr: Array(Unknown), index: f64, val: Unknown) -> void
//   GML: arr[index] = val
//   JS: arr[index] = val
// ---------------------------------------------------------------------------

fn attach_body_array_set(module: &mut Module) {
    module.attach_runtime_body(
        "array_set",
        &[
            Type::Array(Box::new(Type::Value)),
            Type::Float(64),
            Type::Value,
        ],
        Type::Void,
        |b| {
            let arr = b.param(0);
            let index = b.param(1);
            let val = b.param(2);
            b.set_index(arr, index, val);
            b.ret(None);
        },
    );
}

// ---------------------------------------------------------------------------
// array_height_2d(arr: Array(Unknown)) -> f64
//   GML: length of the outer dimension (same as array_length)
//   JS: arr.length
// ---------------------------------------------------------------------------

fn attach_body_array_height_2d(module: &mut Module) {
    module.attach_runtime_body(
        "array_height_2d",
        &[Type::Array(Box::new(Type::Value))],
        Type::Float(64),
        |b| {
            let arr = b.param(0);
            let result = b.call_named("array_length_arr", &[arr], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// array_pop(arr: Array(Unknown)) -> Unknown
//   GML: removes and returns last element
//   JS: arr.pop()
// ---------------------------------------------------------------------------

fn attach_body_array_pop(module: &mut Module) {
    module.attach_runtime_body(
        "array_pop",
        &[Type::Array(Box::new(Type::Value))],
        Type::Value,
        |b| {
            let arr = b.param(0);
            let result = b.call_named("array_pop_arr", &[arr], Type::Value);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// array_delete(arr: Array(Unknown), index: Float64, count: Float64) -> Void
//   GML: removes count elements starting at index
//   JS: arr.splice(index, count)
// ---------------------------------------------------------------------------

fn attach_body_array_delete(module: &mut Module) {
    module.attach_runtime_body(
        "array_delete",
        &[
            Type::Array(Box::new(Type::Value)),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Void,
        |b| {
            let arr = b.param(0);
            let index = b.param(1);
            let count = b.param(2);
            b.call_named("array_delete_arr", &[arr, index, count], Type::Void);
            b.ret(None);
        },
    );
}

// ---------------------------------------------------------------------------
// array_insert(arr: Array(Unknown), index: Float64, val: Unknown) -> Void
//   GML: inserts val at index
//   JS: arr.splice(index, 0, val)
// ---------------------------------------------------------------------------

fn attach_body_array_insert(module: &mut Module) {
    module.attach_runtime_body(
        "array_insert",
        &[
            Type::Array(Box::new(Type::Value)),
            Type::Float(64),
            Type::Value,
        ],
        Type::Void,
        |b| {
            let arr = b.param(0);
            let index = b.param(1);
            let val = b.param(2);
            b.call_named("array_insert_arr", &[arr, index, val], Type::Void);
            b.ret(None);
        },
    );
}

// ---------------------------------------------------------------------------
// array_resize(arr: Array(Unknown), newSize: Float64) -> Void
//   GML: resizes array to newSize
//   JS: arr.splice(newSize) — removes all elements from newSize onward
// ---------------------------------------------------------------------------

fn attach_body_array_resize(module: &mut Module) {
    module.attach_runtime_body(
        "array_resize",
        &[Type::Array(Box::new(Type::Value)), Type::Float(64)],
        Type::Void,
        |b| {
            let arr = b.param(0);
            let new_size = b.param(1);
            b.call_named("array_resize_arr", &[arr, new_size], Type::Void);
            b.ret(None);
        },
    );
}

// ---------------------------------------------------------------------------
// array_get_index(arr: Array(Unknown), value: Unknown) -> Float64
//   GML: returns index of value, or -1 if not found
//   JS: arr.indexOf(value)
// ---------------------------------------------------------------------------

fn attach_body_array_get_index(module: &mut Module) {
    module.attach_runtime_body(
        "array_get_index",
        &[Type::Array(Box::new(Type::Value)), Type::Value],
        Type::Float(64),
        |b| {
            let arr = b.param(0);
            let val = b.param(1);
            let result = b.call_named("array_get_index_arr", &[arr, val], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// point_in_triangle(px, py, x1, y1, x2, y2, x3, y3: f64) -> Bool
//   Barycentric coordinate method:
//     d = (y2-y3)*(x1-x3) + (x3-x2)*(y1-y3)
//     a = ((y2-y3)*(px-x3) + (x3-x2)*(py-y3)) / d
//     b = ((y3-y1)*(px-x3) + (x1-x3)*(py-y3)) / d
//     c = 1 - a - b
//     result = a >= 0 && b >= 0 && c >= 0
// ---------------------------------------------------------------------------

fn attach_body_point_in_triangle(module: &mut Module) {
    let fid = module
        .lookup_runtime("point_in_triangle")
        .unwrap_or_else(|| {
            panic!("attach_runtime_body: 'point_in_triangle' not in runtime registry")
        });
    let sig = FunctionSig {
        params: vec![
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
            Type::Float(64),
        ],
        return_ty: Type::Bool,
        defaults: vec![],
        has_rest_param: false,
        param_lower_bounds: vec![],
    };
    let mut b = make_builder(module, "point_in_triangle", sig);

    let px = b.param(0);
    let py = b.param(1);
    let x1 = b.param(2);
    let y1 = b.param(3);
    let x2 = b.param(4);
    let y2 = b.param(5);
    let x3 = b.param(6);
    let y3 = b.param(7);

    // d = (y2-y3)*(x1-x3) + (x3-x2)*(y1-y3)
    let y2_y3 = b.sub(y2, y3);
    let x1_x3 = b.sub(x1, x3);
    let x3_x2 = b.sub(x3, x2);
    let y1_y3 = b.sub(y1, y3);
    let d_left = b.mul(y2_y3, x1_x3);
    let d_right = b.mul(x3_x2, y1_y3);
    let d = b.add(d_left, d_right);

    // px-x3, py-y3
    let px_x3 = b.sub(px, x3);
    let py_y3 = b.sub(py, y3);

    // a = ((y2-y3)*(px-x3) + (x3-x2)*(py-y3)) / d
    let a_left = b.mul(y2_y3, px_x3);
    let a_right = b.mul(x3_x2, py_y3);
    let a_num = b.add(a_left, a_right);
    let a = b.div(a_num, d);

    // bv = ((y3-y1)*(px-x3) + (x1-x3)*(py-y3)) / d
    let y3_y1 = b.sub(y3, y1);
    let bv_left = b.mul(y3_y1, px_x3);
    let bv_right = b.mul(x1_x3, py_y3);
    let bv_num = b.add(bv_left, bv_right);
    let bv = b.div(bv_num, d);

    // c = 1 - a - bv
    let one = b.const_float(1.0);
    let one_minus_a = b.sub(one, a);
    let c = b.sub(one_minus_a, bv);

    let zero = b.const_float(0.0);
    let a_ge_0 = b.cmp(CmpKind::Ge, a, zero);
    let b_ge_0 = b.cmp(CmpKind::Ge, bv, zero);
    let c_ge_0 = b.cmp(CmpKind::Ge, c, zero);
    let ab = b.bool_and(a_ge_0, b_ge_0);
    let result = b.bool_and(ab, c_ge_0);
    b.ret(Some(result));

    let built = b.build();
    let stub = &mut module.functions[fid];
    stub.blocks = built.blocks;
    stub.insts = built.insts;
    stub.value_types = built.value_types;
    stub.entry = built.entry;
    // InlineHint left as Default (not Always) — complex multi-step function
}

// ---------------------------------------------------------------------------
// variable_struct_exists(struct: Unknown, name: String) -> Bool
// JS emit: struct != null && Object.prototype.hasOwnProperty.call(struct, name)
// ---------------------------------------------------------------------------

fn attach_body_variable_struct_exists(module: &mut Module) {
    module.attach_runtime_body(
        "variable_struct_exists",
        &[Type::Value, Type::String],
        Type::Bool,
        |b| {
            let s = b.param(0);
            let name = b.param(1);
            let result = b.call_named("variable_struct_exists_rt", &[s, name], Type::Bool);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// variable_struct_get(struct: Unknown, name: String) -> Unknown
// JS emit: struct?.[name]  (or fallback ternary if optional chaining not present)
// ---------------------------------------------------------------------------

fn attach_body_variable_struct_get(module: &mut Module) {
    module.attach_runtime_body(
        "variable_struct_get",
        &[Type::Value, Type::String],
        Type::Value,
        |b| {
            let s = b.param(0);
            let name = b.param(1);
            let result = b.call_named("variable_struct_get_rt", &[s, name], Type::Value);
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// variable_struct_names_count(struct: Unknown) -> Float64
// JS emit: struct != null ? Object.keys(struct).length : 0
// ---------------------------------------------------------------------------

fn attach_body_variable_struct_names_count(module: &mut Module) {
    module.attach_runtime_body(
        "variable_struct_names_count",
        &[Type::Value],
        Type::Float(64),
        |b| {
            let s = b.param(0);
            let result = b.call_named("variable_struct_names_count_rt", &[s], Type::Float(64));
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// array_sort(arr: Array<Unknown>, ascending: Bool) -> Void
// GML: sorts arr in place
// JS emit: arr.sort((a, b) => ascending ? a - b : b - a)
// ---------------------------------------------------------------------------

fn attach_body_array_sort(module: &mut Module) {
    module.attach_runtime_body(
        "array_sort",
        &[Type::Array(Box::new(Type::Value)), Type::Bool],
        Type::Void,
        |b| {
            let arr = b.param(0);
            let ascending = b.param(1);
            b.call_named("array_sort_arr", &[arr, ascending], Type::Void);
            b.ret(None);
        },
    );
}

// ---------------------------------------------------------------------------
// array_unique(arr: Array<Unknown>, offset: Float64, length: Float64) -> Array<Unknown>
// GML: returns a new array with duplicate values removed
// JS emit: [...new Set(arr)]  (offset/length unused — runtime.ts doesn't implement them)
// ---------------------------------------------------------------------------

fn attach_body_array_unique(module: &mut Module) {
    module.attach_runtime_body(
        "array_unique",
        &[
            Type::Array(Box::new(Type::Value)),
            Type::Float(64),
            Type::Float(64),
        ],
        Type::Array(Box::new(Type::Value)),
        |b| {
            let arr = b.param(0);
            // params 1 (offset) and 2 (length) are ignored —
            // runtime.ts array_unique does not implement them.
            let result = b.call_named(
                "array_unique_arr",
                &[arr],
                Type::Array(Box::new(Type::Value)),
            );
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// variable_struct_get_names(struct: Unknown) -> Array<String>
// JS emit: struct != null ? Object.keys(struct) : []
// ---------------------------------------------------------------------------

fn attach_body_variable_struct_get_names(module: &mut Module) {
    module.attach_runtime_body(
        "variable_struct_get_names",
        &[Type::Value],
        Type::Array(Box::new(Type::String)),
        |b| {
            let s = b.param(0);
            let result = b.call_named(
                "variable_struct_get_names_rt",
                &[s],
                Type::Array(Box::new(Type::String)),
            );
            b.ret(Some(result));
        },
    );
}

// ---------------------------------------------------------------------------
// variable_struct_set(struct: Unknown, name: String, val: Unknown) -> Void
// JS emit: struct[name] = val
// ---------------------------------------------------------------------------

fn attach_body_variable_struct_set(module: &mut Module) {
    module.attach_runtime_body(
        "variable_struct_set",
        &[Type::Value, Type::String, Type::Value],
        Type::Void,
        |b| {
            let s = b.param(0);
            let name = b.param(1);
            let val = b.param(2);
            b.call_named("variable_struct_set_rt", &[s, name, val], Type::Void);
            b.ret(None);
        },
    );
}

// ---------------------------------------------------------------------------
// xorgen_next(prng: XorGen) -> i32
// XorShift128 ring-buffer step (8 slots). Reads and writes prng.x[] and prng.i.
// ---------------------------------------------------------------------------

fn attach_body_xorgen_next(module: &mut Module, xorgen_type_id: TypeId) {
    let xorgen_ty = Type::Instance(xorgen_type_id);
    let sig = FunctionSig {
        params: vec![xorgen_ty.clone()],
        return_ty: Type::Int(32),
        defaults: vec![],
        has_rest_param: false,
        param_lower_bounds: vec![],
    };
    let mut b = make_builder(module, "xorgen_next", sig);

    let xorgen = b.param(0);
    let x_arr = b.get_field(xorgen, "x", Type::Array(Box::new(Type::Int(32))));
    let i = b.get_field(xorgen, "i", Type::Int(32));

    // Pre-bind all integer constants to avoid nested mutable borrows
    let c1 = b.const_int(1, 32);
    let c3 = b.const_int(3, 32);
    let c4 = b.const_int(4, 32);
    let c7 = b.const_int(7, 32);
    let c9 = b.const_int(9, 32);
    let c10 = b.const_int(10, 32);
    let c13 = b.const_int(13, 32);
    let c24 = b.const_int(24, 32);

    // Step 1: t = X[i]; t ^= (t >>> 7); v = t ^ (t << 24)
    let t1 = b.get_index(x_arr, i, Type::Int(32));
    let t1_shr7 = b.call_named("lshr_i32", &[t1, c7], Type::Int(32));
    let t1x = b.call_named("bitxor_i32", &[t1, t1_shr7], Type::Int(32));
    let t1x_shl24 = b.call_named("shl_i32", &[t1x, c24], Type::Int(32));
    let v = b.call_named("bitxor_i32", &[t1x, t1x_shl24], Type::Int(32));

    // Step 2: t = X[(i+1)&7]; v ^= t ^ (t >>> 10)
    let i_p1 = b.call_named("add_i32", &[i, c1], Type::Int(32));
    let idx2 = b.call_named("bitand_i32", &[i_p1, c7], Type::Int(32));
    let t2 = b.get_index(x_arr, idx2, Type::Int(32));
    let t2_shr10 = b.call_named("lshr_i32", &[t2, c10], Type::Int(32));
    let t2x = b.call_named("bitxor_i32", &[t2, t2_shr10], Type::Int(32));
    let v = b.call_named("bitxor_i32", &[v, t2x], Type::Int(32));

    // Step 3: t = X[(i+3)&7]; v ^= t ^ (t >>> 3)
    let i_p3 = b.call_named("add_i32", &[i, c3], Type::Int(32));
    let idx3 = b.call_named("bitand_i32", &[i_p3, c7], Type::Int(32));
    let t3 = b.get_index(x_arr, idx3, Type::Int(32));
    let t3_shr3 = b.call_named("lshr_i32", &[t3, c3], Type::Int(32));
    let t3x = b.call_named("bitxor_i32", &[t3, t3_shr3], Type::Int(32));
    let v = b.call_named("bitxor_i32", &[v, t3x], Type::Int(32));

    // Step 4: t = X[(i+4)&7]; v ^= t ^ (t << 7)
    let i_p4 = b.call_named("add_i32", &[i, c4], Type::Int(32));
    let idx4 = b.call_named("bitand_i32", &[i_p4, c7], Type::Int(32));
    let t4 = b.get_index(x_arr, idx4, Type::Int(32));
    let t4_shl7 = b.call_named("shl_i32", &[t4, c7], Type::Int(32));
    let t4x = b.call_named("bitxor_i32", &[t4, t4_shl7], Type::Int(32));
    let v = b.call_named("bitxor_i32", &[v, t4x], Type::Int(32));

    // Step 5: t = X[(i+7)&7]; t ^= (t << 13); v ^= t ^ (t << 9)
    let i_p7 = b.call_named("add_i32", &[i, c7], Type::Int(32));
    let idx7 = b.call_named("bitand_i32", &[i_p7, c7], Type::Int(32));
    let t5 = b.get_index(x_arr, idx7, Type::Int(32));
    let t5_shl13 = b.call_named("shl_i32", &[t5, c13], Type::Int(32));
    let t5x = b.call_named("bitxor_i32", &[t5, t5_shl13], Type::Int(32));
    let t5x_shl9 = b.call_named("shl_i32", &[t5x, c9], Type::Int(32));
    let t5xx = b.call_named("bitxor_i32", &[t5x, t5x_shl9], Type::Int(32));
    let v = b.call_named("bitxor_i32", &[v, t5xx], Type::Int(32));

    // X[i] = v
    b.set_index(x_arr, i, v);

    // this.i = (i + 1) & 7
    let i_p1_new = b.call_named("add_i32", &[i, c1], Type::Int(32));
    let new_i = b.call_named("bitand_i32", &[i_p1_new, c7], Type::Int(32));
    b.set_field(xorgen, "i", new_i);

    b.ret(Some(v));

    let built = b.build();
    let fid = module.lookup_runtime("xorgen_next").unwrap();
    let stub = &mut module.functions[fid];
    stub.blocks = built.blocks;
    stub.insts = built.insts;
    stub.value_types = built.value_types;
    stub.entry = built.entry;
    // InlineHint left as Default — complex multi-step function
}

// ---------------------------------------------------------------------------
// random(_rt: GameRuntime, max: f64) -> f64
// Calls xorgen_next on rt._math.prng, converts signed i32 to [0, max).
// ---------------------------------------------------------------------------

fn attach_body_random(module: &mut Module, xorgen_type_id: TypeId, math_state_type_id: TypeId) {
    let rt_type_id = module.runtime_type_id.unwrap();
    let rt_ty = Type::Instance(rt_type_id);
    let sig = FunctionSig {
        params: vec![rt_ty, Type::Float(64)],
        return_ty: Type::Float(64),
        defaults: vec![],
        has_rest_param: false,
        param_lower_bounds: vec![],
    };
    let mut b = make_builder(module, "random", sig);

    let rt = b.param(0);
    let max = b.param(1);

    let math = b.get_field(rt, "_math", Type::Instance(math_state_type_id));
    let prng = b.get_field(math, "prng", Type::Instance(xorgen_type_id));
    let raw = b.call_named("xorgen_next", &[prng], Type::Int(32));

    // Convert signed i32 to float in [0, max):
    // (coerce(raw, f64) + 2147483648.0) * max / 4294967295.0
    let raw_f = b.coerce(raw, Type::Float(64));
    let c_i32_min = b.const_float(2_147_483_648.0);
    let c_u32_max = b.const_float(4_294_967_295.0);
    let shifted = b.call_named("add_f64", &[raw_f, c_i32_min], Type::Float(64));
    let divided = b.call_named("div_f64", &[shifted, c_u32_max], Type::Float(64));
    let result = b.call_named("mul_f64", &[divided, max], Type::Float(64));

    b.ret(Some(result));

    let built = b.build();
    let fid = module.lookup_runtime("random").unwrap();
    let stub = &mut module.functions[fid];
    stub.blocks = built.blocks;
    stub.insts = built.insts;
    stub.value_types = built.value_types;
    stub.entry = built.entry;
    // InlineHint left as Default
}

// ---------------------------------------------------------------------------
// random_range(_rt: GameRuntime, min: f64, max: f64) -> f64
// Delegates to random(rt, max - min) + min.
// ---------------------------------------------------------------------------

fn attach_body_random_range(module: &mut Module) {
    let rt_type_id = module.runtime_type_id.unwrap();
    let rt_ty = Type::Instance(rt_type_id);
    let sig = FunctionSig {
        params: vec![rt_ty, Type::Float(64), Type::Float(64)],
        return_ty: Type::Float(64),
        defaults: vec![],
        has_rest_param: false,
        param_lower_bounds: vec![],
    };
    let mut b = make_builder(module, "random_range", sig);

    let rt = b.param(0);
    let min = b.param(1);
    let max = b.param(2);

    let range = b.call_named("sub_f64", &[max, min], Type::Float(64));
    let r = b.call_named("random", &[rt, range], Type::Float(64));
    let result = b.call_named("add_f64", &[r, min], Type::Float(64));

    b.ret(Some(result));

    let built = b.build();
    let fid = module.lookup_runtime("random_range").unwrap();
    let stub = &mut module.functions[fid];
    stub.blocks = built.blocks;
    stub.insts = built.insts;
    stub.value_types = built.value_types;
    stub.entry = built.entry;
    // InlineHint left as Default
}

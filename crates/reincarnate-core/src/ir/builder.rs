use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// Core arithmetic/logic builtin registry, lazily initialized from a throwaway
/// `Module::new("__core__")`.  Used as the default registry for `FunctionBuilder`
/// so that tests that build functions without a full module still work.
///
/// All modules created via `Module::new` call `register_core_builtins()` first,
/// so the core builtins always receive the same FuncIds (0 … N-1) in every
/// module — making this static registry safe to use across all builders.
static CORE_REGISTRY: OnceLock<HashMap<String, FuncId>> = OnceLock::new();

fn core_registry() -> &'static HashMap<String, FuncId> {
    CORE_REGISTRY.get_or_init(|| {
        let m = Module::new("__core__".to_string());
        m.runtime_registry
    })
}

use crate::entity::PrimaryMap;

use super::block::{Block, BlockId, BlockParam};
use super::func::MethodKind;
use super::func::{CaptureMode, CaptureParam, FuncId, Function, InlineHint, Visibility};
use super::inst::{CastKind, CmpKind, Inst, Op, Terminator};
use super::module::{
    ClassDef, EntryPoint, EnumDef, ExternalImport, Global, Import, Module, StructDef, TypeDecl,
};
use super::ty::{FunctionSig, Type, TypeId};
use super::value::{Constant, ValueId};

/// Builder for constructing a single [`Function`].
///
/// Manages value allocation, block creation, and instruction emission.
/// Tracks a "current block" cursor — instructions are appended to it.
pub struct FunctionBuilder {
    name: String,
    func: Function,
    current_block: BlockId,
    /// Name-to-FuncId registry for string-based call resolution.
    ///
    /// Populated via [`set_registry`] by callers that need to emit named calls.
    /// Empty by default — test code that builds functions without a module
    /// registry must use [`call`] with a `FuncId` directly.
    registry: HashMap<String, super::func::FuncId>,
}

impl FunctionBuilder {
    /// Create a new function builder.
    ///
    /// Creates the entry block and allocates `ValueId`s for each parameter.
    pub fn new(name: impl Into<String>, sig: FunctionSig, visibility: Visibility) -> Self {
        let name = name.into();
        let mut blocks = PrimaryMap::new();
        let mut value_types = PrimaryMap::new();

        // Create entry block with params matching the function signature.
        let mut params = Vec::with_capacity(sig.params.len());
        for ty in &sig.params {
            let value = value_types.push(ty.clone());
            params.push(BlockParam {
                value,
                ty: ty.clone(),
            });
        }
        let entry = blocks.push(Block {
            params,
            insts: Vec::new(),
            terminator: Terminator::default(),
        });

        let func = Function {
            name: name.clone(),
            sig,
            visibility,
            namespace: Vec::new(),
            class: None,
            method_kind: MethodKind::Free,
            specializations: HashMap::new(),
            blocks,
            insts: PrimaryMap::new(),
            value_types,
            entry,
            coroutine: None,
            value_names: HashMap::new(),
            capture_params: Vec::new(),
            null_sentinel_values: std::collections::HashSet::new(),
            type_rule: None,
            inline_hint: InlineHint::Default,
        };

        Self {
            name,
            func,
            current_block: entry,
            registry: HashMap::new(),
        }
    }

    /// Install a name-to-FuncId registry so this builder can resolve named calls.
    ///
    /// Must be called before any `call_named` or arithmetic helper that uses
    /// named builtins.  Any code that creates a `FunctionBuilder` without a
    /// module context (e.g. tests) may omit this call and use [`call`] directly.
    pub fn set_registry(&mut self, r: HashMap<String, super::func::FuncId>) {
        self.registry = r;
    }

    /// The name-to-FuncId registry installed via [`set_registry`].
    ///
    /// Used by frontends that need to propagate the registry into nested
    /// translations (e.g. closure bodies translated recursively).
    ///
    /// [`set_registry`]: FunctionBuilder::set_registry
    pub fn registry(&self) -> &HashMap<String, super::func::FuncId> {
        &self.registry
    }

    /// Emit a direct call by function name, resolving via the installed registry.
    ///
    /// Resolution order:
    /// 1. Instance registry installed via [`set_registry`] (user functions + engine builtins).
    /// 2. Static core registry (arithmetic/logic builtins from [`Module::register_core_builtins`]).
    ///
    /// # Panics
    /// Panics if `name` is not present in either registry.  This indicates a
    /// builder construction error — the registry must be installed before
    /// calling user-defined or engine-specific functions.
    pub fn call_named(&mut self, name: &str, args: &[ValueId], ret_ty: Type) -> ValueId {
        let func = self
            .registry
            .get(name)
            .or_else(|| core_registry().get(name))
            .copied()
            .unwrap_or_else(|| panic!("call_named: '{name}' not in registry"));
        self.emit(
            Op::Call {
                func,
                args: args.to_vec(),
            },
            ret_ty,
        )
    }

    /// Create a new block with no parameters. Returns its `BlockId`.
    pub fn create_block(&mut self) -> BlockId {
        self.func.blocks.push(Block {
            params: Vec::new(),
            insts: Vec::new(),
            terminator: Terminator::default(),
        })
    }

    /// Create a new block with the given parameter types.
    /// Returns the `BlockId` and `ValueId`s for each parameter.
    pub fn create_block_with_params(&mut self, types: &[Type]) -> (BlockId, Vec<ValueId>) {
        let mut params = Vec::with_capacity(types.len());
        let mut values = Vec::with_capacity(types.len());
        for ty in types {
            let value = self.func.value_types.push(ty.clone());
            params.push(BlockParam {
                value,
                ty: ty.clone(),
            });
            values.push(value);
        }
        let block = self.func.blocks.push(Block {
            params,
            insts: Vec::new(),
            terminator: Terminator::default(),
        });
        (block, values)
    }

    /// Switch the current block cursor to the given block.
    pub fn switch_to_block(&mut self, block: BlockId) {
        self.current_block = block;
    }

    /// Get the current block.
    pub fn current_block(&self) -> BlockId {
        self.current_block
    }

    /// Get the entry block.
    pub fn entry_block(&self) -> BlockId {
        self.func.entry
    }

    /// Get the `ValueId` for a function parameter by index.
    ///
    /// # Panics
    /// Panics if `index` is out of range.
    pub fn param(&self, index: usize) -> ValueId {
        self.func.blocks[self.func.entry].params[index].value
    }

    /// Number of parameters in the entry block.
    pub fn param_count(&self) -> usize {
        self.func.blocks[self.func.entry].params.len()
    }

    /// Set class metadata on the function being built.
    pub fn set_class(&mut self, ns: Vec<String>, class: String, kind: MethodKind) {
        self.func.namespace = ns;
        self.func.class = Some(class);
        self.func.method_kind = kind;
    }

    /// Set just the method kind, leaving namespace/class unchanged.
    ///
    /// Used for callback functions that need a non-default kind (e.g. `Closure`)
    /// without belonging to a class.
    pub fn set_method_kind(&mut self, kind: MethodKind) {
        self.func.method_kind = kind;
    }

    /// Declare capture parameters for a closure function.
    ///
    /// Appends capture params after the regular `sig.params` in the entry block
    /// and records them in `func.capture_params`. Returns their `ValueId`s in order.
    /// Must be called before emitting any instructions.
    pub fn add_capture_params(
        &mut self,
        captures: Vec<(String, Type, CaptureMode)>,
    ) -> Vec<ValueId> {
        let mut values = Vec::with_capacity(captures.len());
        for (name, ty, mode) in captures {
            let value = self.func.value_types.push(ty.clone());
            self.func.blocks[self.func.entry].params.push(BlockParam {
                value,
                ty: ty.clone(),
            });
            self.func.value_names.insert(value, name.clone());
            self.func
                .capture_params
                .push(CaptureParam { name, ty, mode });
            values.push(value);
        }
        values
    }

    /// Get the `ValueId` for a capture parameter by index.
    ///
    /// Capture params follow regular params in the entry block.
    ///
    /// # Panics
    /// Panics if `index` is out of range.
    pub fn capture_param(&self, index: usize) -> ValueId {
        let regular = self.func.sig.params.len();
        self.func.blocks[self.func.entry].params[regular + index].value
    }

    /// Attach a debug name to a value (from source-level variable/parameter names).
    pub fn name_value(&mut self, v: ValueId, name: String) {
        self.func.value_names.insert(v, name);
    }

    /// Check whether a value already has a debug name.
    pub fn has_name(&self, v: ValueId) -> bool {
        self.func.value_names.contains_key(&v)
    }

    /// If `value` was produced by a `Cast` with `CastKind::Coerce` to an
    /// integer type, return the inner (pre-cast) value.  This is used to
    /// strip GML `Conv.v.i32` instructions that the VM emits for internal
    /// byte-layout reasons before `pushac`/`pushaf`, where the array
    /// reference should remain `Unknown` at the decompilation level.
    pub fn try_peel_int_coerce(&self, value: ValueId) -> ValueId {
        for inst in self.func.insts.values() {
            if inst.result == Some(value) {
                if let Op::Cast(inner, Type::Int(_), CastKind::Coerce) = &inst.op {
                    return *inner;
                }
                return value;
            }
        }
        value
    }

    /// If `value` was produced by a `Const` instruction, return the constant.
    pub fn try_get_const(&self, value: ValueId) -> Option<&Constant> {
        for inst in self.func.insts.values() {
            if inst.result == Some(value) {
                if let Op::Const(c) = &inst.op {
                    return Some(c);
                }
                return None;
            }
        }
        None
    }

    /// Like [`try_get_const`] but also handles block parameters (returns None
    /// for non-constant values).
    ///
    /// In compound assignments (e.g. `obj.field += expr`), the GML compiler
    /// emits a `Dup` before the read-modify-write sequence. The duplicated
    /// stack entry reuses the same ValueId, so the target value is already the
    /// constant itself.
    pub fn try_resolve_const(&self, value: ValueId) -> Option<Constant> {
        for inst in self.func.insts.values() {
            if inst.result == Some(value) {
                match &inst.op {
                    Op::Const(c) => return Some(c.clone()),
                    _ => return None,
                }
            }
        }
        None
    }

    /// Get the function name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Consume the builder and return the constructed `Function`.
    pub fn build(self) -> Function {
        self.func
    }

    // -- internal helpers --

    /// Push an instruction with a result value into the current block.
    fn emit(&mut self, op: Op, ty: Type) -> ValueId {
        let value = self.func.value_types.push(ty);
        let inst_id = self.func.insts.push(Inst {
            op,
            result: Some(value),
            span: None,
        });
        self.func.blocks[self.current_block].insts.push(inst_id);
        value
    }

    /// Push a void instruction (no result value) into the current block.
    fn emit_void(&mut self, op: Op) {
        let inst_id = self.func.insts.push(Inst {
            op,
            result: None,
            span: None,
        });
        self.func.blocks[self.current_block].insts.push(inst_id);
    }

    /// Add parameters to an existing block. Returns `ValueId`s for each new parameter.
    ///
    /// Useful when translating stack-based bytecode where merge-point types
    /// are discovered during translation, not before block creation.
    pub fn add_block_params(&mut self, block: BlockId, types: &[Type]) -> Vec<ValueId> {
        let mut values = Vec::with_capacity(types.len());
        for ty in types {
            let value = self.func.value_types.push(ty.clone());
            self.func.blocks[block].params.push(BlockParam {
                value,
                ty: ty.clone(),
            });
            values.push(value);
        }
        values
    }

    /// Look up the type of a value.
    pub fn value_type(&self, value: ValueId) -> Type {
        self.func.value_types[value].clone()
    }

    // ========================================================================
    // Constants
    // ========================================================================

    pub fn const_null(&mut self) -> ValueId {
        let c = Constant::Null;
        let ty = c.ty();
        self.emit(Op::Const(c), ty)
    }

    pub fn const_bool(&mut self, value: bool) -> ValueId {
        let c = Constant::Bool(value);
        let ty = c.ty();
        self.emit(Op::Const(c), ty)
    }

    pub fn const_int(&mut self, value: i64, bits: u8) -> ValueId {
        self.emit(Op::Const(Constant::Int(value)), Type::Int(bits))
    }

    pub fn const_uint(&mut self, value: u64) -> ValueId {
        let c = Constant::UInt(value);
        let ty = c.ty();
        self.emit(Op::Const(c), ty)
    }

    pub fn const_float(&mut self, value: f64) -> ValueId {
        let c = Constant::Float(value);
        let ty = c.ty();
        self.emit(Op::Const(c), ty)
    }

    pub fn const_string(&mut self, value: impl Into<String>) -> ValueId {
        let c = Constant::String(value.into());
        let ty = c.ty();
        self.emit(Op::Const(c), ty)
    }

    // ========================================================================
    // Arithmetic (emit typed builtin calls)
    // ========================================================================

    /// Select the type suffix for a given type.
    /// Unknown/variable types use the "any" suffix, which maps to an untyped
    /// operator in the backend (no signature constraints are emitted).
    fn builtin_type_suffix(ty: &Type) -> &'static str {
        match ty {
            Type::Float(64) => "f64",
            Type::Float(32) => "f32",
            Type::Int(32) => "i32",
            Type::Int(64) => "i64",
            other => panic!("builtin_type_suffix: unsupported type {other:?}"),
        }
    }

    /// Emit a binary arithmetic builtin call, deriving the builtin name from
    /// the type of the first operand (e.g. `Float(64)` → `"add_f64"`).
    pub fn add(&mut self, a: ValueId, b: ValueId) -> ValueId {
        let ty = self.value_type(a);
        let name = format!("add_{}", Self::builtin_type_suffix(&ty));
        self.call_named(&name, &[a, b], ty)
    }

    pub fn sub(&mut self, a: ValueId, b: ValueId) -> ValueId {
        let ty = self.value_type(a);
        let name = format!("sub_{}", Self::builtin_type_suffix(&ty));
        self.call_named(&name, &[a, b], ty)
    }

    pub fn mul(&mut self, a: ValueId, b: ValueId) -> ValueId {
        let ty = self.value_type(a);
        let name = format!("mul_{}", Self::builtin_type_suffix(&ty));
        self.call_named(&name, &[a, b], ty)
    }

    pub fn div(&mut self, a: ValueId, b: ValueId) -> ValueId {
        let ty = self.value_type(a);
        let name = format!("div_{}", Self::builtin_type_suffix(&ty));
        self.call_named(&name, &[a, b], ty)
    }

    pub fn rem(&mut self, a: ValueId, b: ValueId) -> ValueId {
        let ty = self.value_type(a);
        let name = format!("rem_{}", Self::builtin_type_suffix(&ty));
        self.call_named(&name, &[a, b], ty)
    }

    pub fn neg(&mut self, a: ValueId) -> ValueId {
        let ty = self.value_type(a);
        let name = format!("neg_{}", Self::builtin_type_suffix(&ty));
        self.call_named(&name, &[a], ty)
    }

    // ========================================================================
    // Bitwise (emit typed builtin calls)
    // ========================================================================

    /// Emit a binary bitwise builtin using the `_i32` variant.
    ///
    /// All source languages perform bitwise operations on integers.  When the
    /// operand type is not `Int(32)` (e.g. `Float(64)` from GML Reals or
    /// `Int(64)` from AVM2 numbers), coerce both operands to `Int(32)` before
    /// the operation and coerce the `Int(32)` result back to the original type.
    /// This matches the ToInt32 semantics used by every major runtime.
    fn bitwise_bin(&mut self, op: &str, a: ValueId, b: ValueId) -> ValueId {
        let ty = self.value_type(a);
        let needs_coerce = !matches!(ty, Type::Int(32));
        let (ai, bi) = if needs_coerce {
            (self.coerce(a, Type::Int(32)), self.coerce(b, Type::Int(32)))
        } else {
            (a, b)
        };
        let name = format!("{op}_i32");
        let r = self.call_named(&name, &[ai, bi], Type::Int(32));
        if needs_coerce {
            self.coerce(r, ty)
        } else {
            r
        }
    }

    /// Emit a unary bitwise NOT builtin using the `_i32` variant.
    ///
    /// See [`bitwise_bin`] for the coercion rationale.
    fn bitwise_un(&mut self, op: &str, a: ValueId) -> ValueId {
        let ty = self.value_type(a);
        let needs_coerce = !matches!(ty, Type::Int(32));
        let ai = if needs_coerce {
            self.coerce(a, Type::Int(32))
        } else {
            a
        };
        let name = format!("{op}_i32");
        let r = self.call_named(&name, &[ai], Type::Int(32));
        if needs_coerce {
            self.coerce(r, ty)
        } else {
            r
        }
    }

    pub fn bit_and(&mut self, a: ValueId, b: ValueId) -> ValueId {
        self.bitwise_bin("bitand", a, b)
    }

    pub fn bit_or(&mut self, a: ValueId, b: ValueId) -> ValueId {
        self.bitwise_bin("bitor", a, b)
    }

    pub fn bit_xor(&mut self, a: ValueId, b: ValueId) -> ValueId {
        self.bitwise_bin("bitxor", a, b)
    }

    pub fn bit_not(&mut self, a: ValueId) -> ValueId {
        self.bitwise_un("bitnot", a)
    }

    pub fn shl(&mut self, a: ValueId, b: ValueId) -> ValueId {
        self.bitwise_bin("shl", a, b)
    }

    pub fn shr(&mut self, a: ValueId, b: ValueId) -> ValueId {
        self.bitwise_bin("shr", a, b)
    }

    // ========================================================================
    // Comparison & logic
    // ========================================================================

    pub fn cmp(&mut self, kind: CmpKind, a: ValueId, b: ValueId) -> ValueId {
        let name = match kind {
            CmpKind::Eq => "cmp_eq",
            CmpKind::Ne => "cmp_ne",
            CmpKind::Lt => "cmp_lt",
            CmpKind::Le => "cmp_le",
            CmpKind::Ge => "cmp_ge",
            CmpKind::Gt => "cmp_gt",
        };
        self.call_named(name, &[a, b], Type::Bool)
    }

    /// Emit a conditional select: `cond ? on_true : on_false`.
    ///
    /// Lowered via the `"select"` core builtin so the emitter dispatches to
    /// a native ternary expression rather than an opaque function call.
    pub fn select(
        &mut self,
        cond: ValueId,
        on_true: ValueId,
        on_false: ValueId,
        result_ty: Type,
    ) -> ValueId {
        self.call_named("select", &[cond, on_true, on_false], result_ty)
    }

    pub fn not(&mut self, a: ValueId) -> ValueId {
        self.call_named("not_bool", &[a], Type::Bool)
    }

    pub fn bool_and(&mut self, a: ValueId, b: ValueId) -> ValueId {
        self.call_named("and_bool", &[a, b], Type::Bool)
    }

    pub fn bool_or(&mut self, a: ValueId, b: ValueId) -> ValueId {
        self.call_named("or_bool", &[a, b], Type::Bool)
    }

    // ========================================================================
    // Control flow
    // ========================================================================

    pub fn br(&mut self, target: BlockId, args: &[ValueId]) {
        // Invariant: args.len() must equal block param count. Mismatches indicate a bug
        // in compute_block_stack_depths (uses linear scan with or_insert → first path wins;
        // can produce wrong param counts when paths disagree). Tracked in TODO.md.
        // Using eprintln rather than assert so translation can continue and produce
        // partial output for debugging; the emitted IR will have missing param assignments.
        if cfg!(debug_assertions) && args.len() != self.func.blocks[target].params.len() {
            eprintln!(
                "[reincarnate] WARN: {} — br to {:?} with {} args but block has {} params \
                 (compute_block_stack_depths depth mismatch — see TODO.md)",
                self.name,
                target,
                args.len(),
                self.func.blocks[target].params.len()
            );
        }
        self.func.blocks[self.current_block].terminator = Terminator::Br {
            target,
            args: args.to_vec(),
        };
    }

    pub fn br_if(
        &mut self,
        cond: ValueId,
        then_target: BlockId,
        then_args: &[ValueId],
        else_target: BlockId,
        else_args: &[ValueId],
    ) {
        if cfg!(debug_assertions) && then_args.len() != self.func.blocks[then_target].params.len() {
            eprintln!(
                "[reincarnate] WARN: {} — br_if then-branch to {:?} with {} args but block has {} params \
                 (compute_block_stack_depths depth mismatch — see TODO.md)",
                self.name, then_target, then_args.len(), self.func.blocks[then_target].params.len()
            );
        }
        if cfg!(debug_assertions) && else_args.len() != self.func.blocks[else_target].params.len() {
            eprintln!(
                "[reincarnate] WARN: {} — br_if else-branch to {:?} with {} args but block has {} params \
                 (compute_block_stack_depths depth mismatch — see TODO.md)",
                self.name, else_target, else_args.len(), self.func.blocks[else_target].params.len()
            );
        }
        self.func.blocks[self.current_block].terminator = Terminator::BrIf {
            cond,
            then_target,
            then_args: then_args.to_vec(),
            else_target,
            else_args: else_args.to_vec(),
        };
    }

    pub fn switch(
        &mut self,
        value: ValueId,
        cases: Vec<(Constant, BlockId, Vec<ValueId>)>,
        default: (BlockId, Vec<ValueId>),
    ) {
        if cfg!(debug_assertions) {
            for (_, target, args) in &cases {
                if args.len() != self.func.blocks[*target].params.len() {
                    eprintln!(
                        "[reincarnate] WARN: {} — switch case to {:?} with {} args but block has {} params \
                         (compute_block_stack_depths depth mismatch — see TODO.md)",
                        self.name, target, args.len(), self.func.blocks[*target].params.len()
                    );
                }
            }
            if default.1.len() != self.func.blocks[default.0].params.len() {
                eprintln!(
                    "[reincarnate] WARN: {} — switch default to {:?} with {} args but block has {} params \
                     (compute_block_stack_depths depth mismatch — see TODO.md)",
                    self.name, default.0, default.1.len(), self.func.blocks[default.0].params.len()
                );
            }
        }
        self.func.blocks[self.current_block].terminator = Terminator::Switch {
            value,
            cases,
            default,
        };
    }

    pub fn ret(&mut self, value: Option<ValueId>) {
        self.func.blocks[self.current_block].terminator = Terminator::Return(value);
    }

    // ========================================================================
    // Memory / fields
    // ========================================================================

    pub fn alloc(&mut self, ty: Type) -> ValueId {
        // The alloc cell's value type is an inference gap, not a dynamic value:
        // emit a solvable placeholder the collector frees into an arena var.
        let result_ty = self.fresh_var();
        self.emit(Op::Alloc(ty), result_ty)
    }

    pub fn load(&mut self, ptr: ValueId, ty: Type) -> ValueId {
        // A placeholder (`fresh_var()`) load result is an inference gap — the
        // type is determinable from the alloc cell constraints. The constraint
        // solver frees the placeholder into an arena var and propagates the
        // alloc's concrete type to this load result.
        self.emit(Op::Load(ptr), ty)
    }

    pub fn store(&mut self, ptr: ValueId, value: ValueId) {
        self.emit_void(Op::Store { ptr, value });
    }

    pub fn get_field(&mut self, object: ValueId, field: impl Into<String>, ty: Type) -> ValueId {
        self.emit(
            Op::GetField {
                object,
                field: field.into(),
            },
            ty,
        )
    }

    pub fn set_field(&mut self, object: ValueId, field: impl Into<String>, value: ValueId) {
        self.emit_void(Op::SetField {
            object,
            field: field.into(),
            value,
        });
    }

    pub fn get_index(&mut self, collection: ValueId, index: ValueId, ty: Type) -> ValueId {
        self.emit(Op::GetIndex { collection, index }, ty)
    }

    pub fn set_index(&mut self, collection: ValueId, index: ValueId, value: ValueId) {
        self.emit_void(Op::SetIndex {
            collection,
            index,
            value,
        });
    }

    // ========================================================================
    // Calls
    // ========================================================================

    pub fn call(&mut self, func: super::func::FuncId, args: &[ValueId], ret_ty: Type) -> ValueId {
        self.emit(
            Op::Call {
                func,
                args: args.to_vec(),
            },
            ret_ty,
        )
    }

    pub fn make_closure(
        &mut self,
        func: impl Into<String>,
        captures: &[ValueId],
        ret_ty: Type,
    ) -> ValueId {
        self.emit(
            Op::MakeClosure {
                func: func.into(),
                captures: captures.to_vec(),
            },
            ret_ty,
        )
    }

    pub fn call_indirect(&mut self, callee: ValueId, args: &[ValueId], ret_ty: Type) -> ValueId {
        self.emit(
            Op::CallIndirect {
                callee,
                args: args.to_vec(),
            },
            ret_ty,
        )
    }

    pub fn call_method(
        &mut self,
        receiver: ValueId,
        method: impl Into<String>,
        args: &[ValueId],
        ret_ty: Type,
    ) -> ValueId {
        self.emit(
            Op::MethodCall {
                receiver,
                method: method.into(),
                args: args.to_vec(),
            },
            ret_ty,
        )
    }

    pub fn system_call(
        &mut self,
        system: impl Into<String>,
        method: impl Into<String>,
        args: &[ValueId],
        ret_ty: Type,
    ) -> ValueId {
        self.emit(
            Op::SystemCall {
                system: system.into(),
                method: method.into(),
                args: args.to_vec(),
            },
            ret_ty,
        )
    }

    // ========================================================================
    // Type operations
    // ========================================================================

    pub fn cast(&mut self, value: ValueId, ty: Type) -> ValueId {
        self.emit(Op::Cast(value, ty.clone(), CastKind::NullableCoerce), ty)
    }

    pub fn coerce(&mut self, value: ValueId, ty: Type) -> ValueId {
        self.emit(Op::Cast(value, ty.clone(), CastKind::Coerce), ty)
    }

    pub fn type_check(&mut self, value: ValueId, ty: Type) -> ValueId {
        self.emit(Op::TypeCheck(value, ty), Type::Bool)
    }

    // ========================================================================
    // Aggregate construction
    // ========================================================================

    pub fn struct_init(
        &mut self,
        name: impl Into<String>,
        fields: Vec<(String, ValueId)>,
    ) -> ValueId {
        let name = name.into();
        // Type inference (TypeInfer pass, Op::StructInit arm) will resolve the
        // correct Instance(TypeId) for this op. Emit a solvable placeholder
        // since the TypeId is not available during IR construction.
        let result_ty = self.fresh_var();
        self.emit(Op::StructInit { name, fields }, result_ty)
    }

    pub fn array_init(&mut self, elements: &[ValueId], elem_ty: Type) -> ValueId {
        let ty = Type::Array(Box::new(elem_ty));
        self.emit(Op::ArrayInit(elements.to_vec()), ty)
    }

    pub fn tuple_init(&mut self, elements: &[ValueId], types: Vec<Type>) -> ValueId {
        let ty = Type::Tuple(types);
        self.emit(Op::TupleInit(elements.to_vec()), ty)
    }

    // ========================================================================
    // Coroutines
    // ========================================================================

    pub fn yield_(&mut self, value: Option<ValueId>, resume_ty: Type) -> ValueId {
        self.emit(Op::Yield(value), resume_ty)
    }

    pub fn coroutine_create(
        &mut self,
        func: impl Into<String>,
        args: &[ValueId],
        yield_ty: Type,
        return_ty: Type,
    ) -> ValueId {
        let ty = Type::Coroutine {
            yield_ty: Box::new(yield_ty),
            return_ty: Box::new(return_ty),
        };
        self.emit(
            Op::CoroutineCreate {
                func: func.into(),
                args: args.to_vec(),
            },
            ty,
        )
    }

    pub fn coroutine_resume(&mut self, coroutine: ValueId, yield_ty: Type) -> ValueId {
        self.emit(Op::CoroutineResume(coroutine), yield_ty)
    }

    // ========================================================================
    // Misc
    // ========================================================================

    pub fn global_ref(&mut self, name: impl Into<String>, ty: Type) -> ValueId {
        self.emit(Op::GlobalRef(name.into()), ty)
    }

    pub fn spread(&mut self, value: ValueId) -> ValueId {
        let ty = self.value_type(value);
        self.emit(Op::Spread(value), ty)
    }

    /// Return a solvable placeholder for a value whose type the frontend does
    /// not yet know — an open inference target the constraint collector will
    /// free into a real arena variable.
    ///
    /// The builder has no arena, so it cannot allocate a real [`TypeVarId`];
    /// it emits [`Type::InferVar`] with a fixed placeholder id. The id is never
    /// dereferenced: the collector pattern-matches the *variant* to decide it
    /// must stay free (`is_concrete(InferVar) == false`), then allocates a fresh
    /// arena var keyed by `ValueId`. This is distinct from [`Type::Value`], the
    /// honest type of a genuinely-dynamic value, which is concrete and pre-binds.
    /// Frontends should call this wherever a value's concrete type is an
    /// inference gap (not where it is genuinely dynamic).
    pub fn fresh_var(&mut self) -> Type {
        Type::fresh_var()
    }
}

/// Builder for constructing a [`Module`].
pub struct ModuleBuilder {
    module: Module,
}

impl ModuleBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            module: Module::new(name.into()),
        }
    }

    pub fn add_function(&mut self, func: Function) -> FuncId {
        let name_id = self.module.name_table.func_names.push(func.name.clone());
        let id = self.module.functions.push(func);
        debug_assert_eq!(id, name_id);
        id
    }

    /// Delegate to [`Module::register_runtime`] for pre-translation builtin registration.
    pub fn register_runtime(&mut self, name: impl Into<String>, sig: FunctionSig) -> FuncId {
        self.module.register_runtime(name, sig)
    }

    /// Expose the runtime registry so callers can build `FunctionBuilder` registries.
    pub fn runtime_registry(&self) -> &HashMap<String, FuncId> {
        &self.module.runtime_registry
    }

    /// Expose a mutable reference to the inner [`Module`].
    ///
    /// Used by frontends that need to call `Module`-level methods (e.g.
    /// `register_gml_syscall_intrinsics`, `freshen_unknown_types_in_sig`)
    /// before translation begins.
    pub fn module_mut(&mut self) -> &mut Module {
        &mut self.module
    }

    /// Pre-register a user function stub so its `FuncId` exists before translation.
    ///
    /// Creates a minimal stub (empty entry block, no params, `Void` return, Public)
    /// and pushes it into the module.  The stub body is replaced by
    /// [`replace_function`] after translation.
    pub fn register_function_stub(&mut self, name: impl Into<String>) -> FuncId {
        use super::block::Block;
        use super::func::{InlineHint, MethodKind, Visibility};
        use super::inst::Terminator;
        use super::ty::FunctionSig;
        use crate::entity::PrimaryMap;

        let name = name.into();
        let mut blocks: PrimaryMap<super::block::BlockId, Block> = PrimaryMap::new();
        let entry = blocks.push(Block {
            params: Vec::new(),
            insts: Vec::new(),
            terminator: Terminator::Return(None),
        });
        let stub = Function {
            name: name.clone(),
            sig: FunctionSig::default(),
            visibility: Visibility::Public,
            namespace: Vec::new(),
            class: None,
            method_kind: MethodKind::Free,
            specializations: HashMap::new(),
            blocks,
            insts: PrimaryMap::new(),
            value_types: PrimaryMap::new(),
            entry,
            coroutine: None,
            value_names: HashMap::new(),
            capture_params: Vec::new(),
            null_sentinel_values: std::collections::HashSet::new(),
            type_rule: None,
            inline_hint: InlineHint::Default,
        };
        let name_id = self.module.name_table.func_names.push(name);
        let id = self.module.functions.push(stub);
        debug_assert_eq!(id, name_id);
        id
    }

    /// Replace a pre-registered stub with the real translated function.
    ///
    /// # Panics
    /// Panics (debug only) if the function name at `id` does not match `func.name`.
    pub fn replace_function(&mut self, id: FuncId, func: Function) {
        debug_assert_eq!(self.module.name_table.func_names[id], func.name);
        self.module.functions[id] = func;
    }

    /// Return the set of all function names currently in the module.
    pub fn existing_function_names(&self) -> HashSet<String> {
        self.module
            .name_table
            .func_names
            .values()
            .cloned()
            .collect()
    }

    /// Register a struct and return the [`TypeId`] for use in [`ClassDef::type_id`].
    ///
    /// If a type with the same name is already interned (e.g. from a prior
    /// `intern_type` call), its namespace, visibility, and fields are updated.
    /// Fields are only written if the existing TypeDecl has none (frontend-declared
    /// structs take precedence over any forward-interned empty stubs).
    pub fn add_struct(&mut self, def: StructDef) -> TypeId {
        let id = self.module.intern_type(&def.name);
        if let Some(TypeDecl::Object {
            namespace,
            visibility,
            fields,
            ..
        }) = self.module.types.get_mut(id)
        {
            *namespace = def.namespace;
            *visibility = def.visibility;
            if fields.is_empty() && !def.fields.is_empty() {
                *fields = def.fields;
            }
        }
        id
    }

    /// Return the number of named Object types currently in the type arena.
    pub fn struct_count(&self) -> usize {
        self.module
            .types
            .values()
            .filter(|td| matches!(td, TypeDecl::Object { name: Some(_), .. }))
            .count()
    }

    /// Return `true` if a named Object type with the given name is already present.
    pub fn has_struct(&self, name: &str) -> bool {
        self.module.type_names.contains_key(name)
    }

    pub fn add_enum(&mut self, def: EnumDef) {
        self.module.enums.push(def);
    }

    pub fn add_global(&mut self, global: Global) {
        self.module.globals.push(global);
    }

    pub fn add_import(&mut self, import: Import) {
        self.module.imports.push(import);
    }

    pub fn add_class(&mut self, class: ClassDef) {
        // Wire TypeDecl.parent from ClassDef.super_class so that the subtype
        // check in CallSiteTypeWiden can traverse the inheritance chain.
        if let Some(super_name) = &class.super_class {
            let child_id = class.type_id;
            let parent_id = self.module.intern_type(super_name);
            if let Some(TypeDecl::Object { parent, .. }) = self.module.types.get_mut(child_id) {
                *parent = Some(parent_id);
            }
        }
        self.module.classes.push(class);
    }

    /// Return [`Type::Value`] for use when the frontend does not yet know a
    /// value's type.
    ///
    /// Delegates to [`Module::fresh_var`].
    pub fn fresh_var(&mut self) -> crate::ir::ty::Type {
        self.module.fresh_var()
    }

    /// Intern a named type and return its [`TypeId`].
    ///
    /// Useful when constructing test modules that need `Type::Instance(id)`
    /// in function signatures before the module is fully built.
    pub fn intern_type(&mut self, name: &str) -> crate::ir::ty::TypeId {
        self.module.intern_type(name)
    }

    /// Get or create a static-side `TypeDecl::Object` for a class and return
    /// `Type::ClassRef(id)`.
    ///
    /// Delegates to [`Module::intern_type_classref`].  Useful when callers need
    /// to pre-intern ClassRef types before translation begins.
    pub fn intern_type_classref(&mut self, name: &str) -> crate::ir::ty::Type {
        self.module.intern_type_classref(name)
    }

    pub fn set_entry_point(&mut self, entry: EntryPoint) {
        self.module.entry_point = Some(entry);
    }

    pub fn add_external_import(&mut self, qualified_name: String, import: ExternalImport) {
        self.module.external_imports.insert(qualified_name, import);
    }

    pub fn set_room_creation_code(&mut self, map: std::collections::BTreeMap<usize, String>) {
        self.module.room_creation_code = map;
    }

    pub fn set_initial_room_name(&mut self, name: String) {
        self.module.initial_room_name = Some(name);
    }

    pub fn set_sprite_names(&mut self, names: Vec<String>) {
        self.module.sprite_names = names;
    }

    pub fn set_object_names(&mut self, names: Vec<String>) {
        self.module.object_names = names;
    }

    pub fn add_passage_name(&mut self, display_name: String, func_name: String) {
        self.module.passage_names.insert(display_name, func_name);
    }

    pub fn add_passage_tags(&mut self, display_name: String, tags: Vec<String>) {
        if !tags.is_empty() {
            self.module.passage_tags.insert(display_name, tags);
        }
    }

    pub fn add_passage_source(&mut self, display_name: String, source: String) {
        self.module.passage_sources.insert(display_name, source);
    }

    pub fn add_passage_storylet(&mut self, display_name: String, cond_func_name: String) {
        self.module
            .passage_storylets
            .insert(display_name, cond_func_name);
    }

    pub fn build(self) -> Module {
        let mut module = self.module;
        // All structs are already registered in `module.types` by `add_struct`.
        // Intern class names not yet in the arena (classes without a prior add_struct).
        for i in 0..module.classes.len() {
            let name = module.classes[i].name.clone();
            module.intern_type(&name);
        }
        // Convert all Type::Struct(name) in function bodies to Type::Instance(id),
        // interning any names not yet in the arena.  This allows frontends to use
        // the convenient string form during construction.
        module.normalize_struct_types();
        module
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_simple_add_function() {
        // Build: fn add(a: Int(64), b: Int(64)) -> Int(64) { return a + b }
        let sig = FunctionSig {
            params: vec![Type::Int(64), Type::Int(64)],
            return_ty: Type::Int(64),
            ..Default::default()
        };
        // Need a ModuleBuilder for the registry so arithmetic helpers work.
        let mb = ModuleBuilder::new("test");
        let registry = mb.runtime_registry().clone();

        let mut fb = FunctionBuilder::new("add", sig, Visibility::Public);
        fb.set_registry(registry);

        let a = fb.param(0);
        let b = fb.param(1);
        let sum = fb.add(a, b);
        fb.ret(Some(sum));

        let func = fb.build();

        assert_eq!(func.name, "add");
        assert_eq!(func.sig.params.len(), 2);
        assert_eq!(func.sig.return_ty, Type::Int(64));

        // Entry block should have 2 params and 1 instruction (add).
        // Terminators are stored in block.terminator, not as instructions.
        let entry = &func.blocks[func.entry];
        assert_eq!(entry.params.len(), 2);
        assert_eq!(entry.insts.len(), 1);

        // The add instruction should have a result.
        let add_inst = &func.insts[entry.insts[0]];
        assert!(add_inst.result.is_some());
        assert!(matches!(&add_inst.op, Op::Call { .. }));

        // The terminator should be Return.
        assert!(matches!(entry.terminator, Terminator::Return(Some(_))));

        // Value types: 2 params + 1 add result = 3.
        assert_eq!(func.value_types.len(), 3);
    }

    #[test]
    fn build_branching_function() {
        // Build: fn choose(cond: Bool, x: Int(64), y: Int(64)) -> Int(64)
        //   entry: br_if cond, then(x), else(y)
        //   then(v): return v
        //   else(v): return v
        let sig = FunctionSig {
            params: vec![Type::Bool, Type::Int(64), Type::Int(64)],
            return_ty: Type::Int(64),
            ..Default::default()
        };
        let mut fb = FunctionBuilder::new("choose", sig, Visibility::Public);

        let cond = fb.param(0);
        let x = fb.param(1);
        let y = fb.param(2);

        let (then_block, then_vals) = fb.create_block_with_params(&[Type::Int(64)]);
        let (else_block, else_vals) = fb.create_block_with_params(&[Type::Int(64)]);

        fb.br_if(cond, then_block, &[x], else_block, &[y]);

        fb.switch_to_block(then_block);
        fb.ret(Some(then_vals[0]));

        fb.switch_to_block(else_block);
        fb.ret(Some(else_vals[0]));

        let func = fb.build();

        assert_eq!(func.blocks.len(), 3);
        // Entry has 3 params, then/else each have 1 param.
        assert_eq!(func.blocks[func.entry].params.len(), 3);
        assert_eq!(func.blocks[then_block].params.len(), 1);
        assert_eq!(func.blocks[else_block].params.len(), 1);
    }

    #[test]
    fn build_module() {
        let sig = FunctionSig {
            params: vec![],
            return_ty: Type::Void,
            ..Default::default()
        };
        let mut fb = FunctionBuilder::new("main", sig, Visibility::Public);
        fb.ret(None);
        let func = fb.build();

        let mut mb = ModuleBuilder::new("test_module");
        let fid = mb.add_function(func);
        mb.add_global(Global {
            name: "counter".into(),
            ty: Type::Int(64),
            visibility: Visibility::Private,
            mutable: true,
            init: None,
        });
        let module = mb.build();

        assert_eq!(module.name, "test_module");
        assert_eq!(
            module.functions.len(),
            Module::NUM_CORE_BUILTINS as usize + 1
        );
        assert_eq!(module.func_name(fid), "main");
        assert_eq!(module.globals.len(), 1);
        assert_eq!(module.globals[0].name, "counter");
    }
}

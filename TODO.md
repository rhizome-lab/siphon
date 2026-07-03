# TODO

Completed items archived in [COMPLETED.md](COMPLETED.md).

Per-engine roadmaps (gaps, runtime coverage, open work) live in [`docs/targets/`](docs/targets/). This file tracks in-flight and near-term work across all active engines.

## `ValidateNoEscapedTypeVars` — source-location granularity (follow-up)

`ValidateNoEscapedTypeVars` (pass `validate-no-escaped-type-vars`) emits `EscapedTypeVar`
diagnostics with `line: 0, col: 0` and the function or type name as the file.  Improving
granularity to a specific `ValueId` or block location requires mapping from `ValueId` back
to a source span — work that is separate from the pass itself and not yet done.

When the IR gains stable value-to-source-span tracking, revisit this pass to emit precise
locations.  Until then, the function/type name is sufficient to identify the inference bug's
location.

## Adjacent finding: `ValidateNoEscapedTypeVars` closes a previously-unenforced invariant

The `InferVar` invariant (no `Type::InferVar` after inference) was true-by-construction
but never actively enforced until `ValidateNoEscapedTypeVars` was added.  If this pass
surfaces actual `EscapedTypeVar` diagnostics on real game input, each one is a signal of a
**real inference bug** — a value that the solver failed to resolve.

Do NOT suppress these diagnostics, disable the pass, or widen a type to silence them.
Each fired diagnostic is a defect report: investigate the inference bug that allowed the
`InferVar` to escape, and fix the upstream solve or write-back.

## Instance-reference builtin params/returns now `Value` — narrowing opportunity (2026-06-12)

Fixed a Law-4 leak: GML builtins typed their *instance* parameters/returns
(`Type_ID_Instance`, `Type_Asset_Object` in the manual) as `Type::Int(32)` — the VM
storage format (instance refs stored as numeric ids), not the source-language type. The
generator (`tools/gen-gml-builtins`) now maps these to `Type::Value` (honest "dynamic
instance reference of unknown concrete object type"). This eliminated the
`Equal(self, Int(32))` constraint that bound ownerless scripts' `self` to `number`
(`hurt`, `sayOtherExt`, etc. now narrow `self` to their caller-LCA instance type).

**Follow-up (emit quality, not correctness):** an instance-creating builtin like
`instance_create_depth` now returns `Type::Value` rather than a concrete instance type.
A human port would type the result as the created object's type. Recovering that requires
flowing the object-asset argument's classref into the return type (frontend-local, GML
`instance_create_*` family). Tracked here; not yet done. Until then the return is honestly
`Value`, not a guessed concrete type.

## `Type::Value` split — five-role overload finding and phased plan (2026-06-12)

**Finding.** `Type::Unknown` was one IR variant overloaded across five distinct semantic
roles, the root of repeated session-poisoning ("is `is_concrete(Unknown)==true` a bug?"):

- **J0 — solvable placeholder.** `builder.rs::fresh_var()` returned it; `alloc`/`load`/
  `StructInit` results; builtin "inference gaps" written before inference runs. Should be a
  free `InferVar` and must never persist. This is the root: because the variant couldn't
  distinguish J0 (re-inferable) from J4 (frozen contract), out-of-band disambiguation was
  forced into every pass that touched the type.
- **J1 — lattice top / poison.** The `unify` catch-all and occurs-check arm. Arises on
  joined functions, coroutines, and classrefs.
- **J2 — honest dynamic source type.** AS3 `*`/`Object`/`Function`; GML
  `is_real(x: …)`; `runtime.json "any"/"dynamic"`. The value is genuinely untyped in
  the source language.
- **J3 — inference-exhausted terminal.** Step-8 diagnostics; the `is_concrete==true`
  "not a free variable" comment in `constraint_collect.rs`. A diagnostic state, not a type.
- **J4 — declared accepts-any param contract.** The `param_pos.is_some()` carve-out in
  `constraint_collect.rs`; the `saw_unknown` evidence gate. Frozen against narrowing.

**Resolution (designed, verified against the code).**

J2 and J4 collapse into one concept — the honest dynamic-value type — because "frozen
against narrowing" is already the universal `unify` behavior for any concrete type, not a
J4-specific property. Named **`Type::Value`** (`Value` matches `serde_json::Value` and the
existing `ty.rs` doc-comment; avoids `Dynamic`'s C# `dynamic`/`any`-checking-disabled
prior). J1 folds into `Value` (a joined/poisoned value behaves identically to a dynamic
value at emit and in `unify`). J0 migrates to the existing `Type::InferVar` (solvable,
validator-forbidden from persisting). J3 is a diagnostic state over a value that stays
`Type::Value` — not a type variant.

**Open question, deferred.** Do NOT mint distinct `Top`/inference-failure variants — the
J1/J3 merge is correct for now; revisit only if a future pass needs to treat
"joined-away" differently from "source-dynamic."

**Phase 0 — DONE (commit `7bd1b624`):** pure mechanical rename `Type::Unknown →
Type::Value`, byte-identical behavior (55 files, 1091 occurrences).

Key files: `crates/reincarnate-core/src/ir/ty.rs`, `.../ir/builder.rs`,
`.../transforms/constraint_collect.rs`, `.../transforms/constraint_solve_hm.rs`,
`.../transforms/validate_no_escaped_type_vars.rs`,
`reincarnate-backend-typescript/src/types.rs`, CLAUDE.md.

- [x] **Phase 1 — DONE (commit `d26cd95b`):** extracted J0 → `Type::InferVar` at the builder seam (`fresh_var()`, `alloc`/`load`/`StructInit` results, `freshen_unknown_types_in_sig`). Deleted the param pre-bind carve-out in `constraint_collect.rs` (collapses to `let should_bind = is_concrete(ty);`). **LOUD posture:** unresolved `InferVar` escapes as hard `RC0006` (EscapedTypeVar) instead of silently lowering to `Value`/`unknown`. `constraint_collect.rs` complexity DROPPED (carve-out deleted). 885 tests green.
- [x] **Phase 2 — DONE (commit `347349c0`):** classified all `matches!(ty, Value | InferVar)` consumer predicates in `mem2reg`, `cfg_simplify`, `redundant_inherited_field`, `constructor_struct_infer`, and Flash gap-vs-dynamic positions. Fixed one latent never-persist bug: a `Value`-only strip that let `InferVar` persist into a struct field union.
- [x] **Phase 3 — DONE (commit `03dcc4d7`):** `saw_unknown → saw_value`; pruned stale guard comments; rewrote the CLAUDE.md `Type::Unknown` law bullet to the corrected two-claim form (`Value` = concrete/honest; `InferVar` = solvable/never-persist; unresolved = loud RC0006). Restored the fail-loud mandate a prior edit had weakened.

**Split is COMPLETE. All four phases landed (verified 2026-06-12).**

**Loud-posture impact — deliberate, do not revert.** Dead Estate check total rose from ~25,476 to ~30,282. The delta (~4,878 RC0006) is functions where inference genuinely doesn't resolve — previously hidden by the conflation (spelled `Value`, lowered to `unknown`, only soft filtered diagnostics). Emitted TS is unchanged; only the diagnostic changed. Do NOT "fix" the 30,282 by reverting the loud posture or by lowering unresolved `InferVar` to `Value` — that reintroduces the swallowing the split removed (the CLAUDE.md law now forbids it).

**RC0006 backlog is the quantified work surface.** Clean-HEAD measurement found ~67,810 unresolved cells across ~4,878 functions, by op (pre-arc baseline):

| Op | Unresolved cells | Root cause |
|---|---|---|
| `Call` | 35,541 | Constructor-erasure (`@@NewGMLObject@@`) + closure/method-var FuncId loss — call results can't be typed when callee link is lost |
| `CallIndirect` | 4,478 | Same: indirect dispatch loses callee signature |
| `GetField` | 16,933 | `self`/`this.field` receiver typing — substantially addressed by the arc below; new dominant lever is TS2339 authored-field inference |
| `GlobalRef` | 7,446 | Unresolved globals |
| `GetIndex` | 1,616 | Unresolved indexing |

These map exactly to the inference levers already identified and prioritized below. Closing each op category is what drives RC0006 down.

**Post-arc update (2026-06-12):** The receiver-typing arc (commits `7db18235`→`950f5e4a`, see item 9 below) resolved the GetField receiver gap for ownerless scripts. RC0006 count rose slightly to ~4,935 as the loud posture now honestly surfaces more previously-swallowed InferVars. The ~4,935 deliberate loud backlog is the new working baseline; do not revert. Next levers: TS2339 authored-field inference (~7,355) and closing the Call/CallIndirect callee-link gaps.

**Minor follow-up (filed):** `ValidateNoEscapedTypeVars` currently breaks after first hit per function, so 4,935 RC0006 undercounts the ~67,810 true unresolved cells. Reporting all escaped cells (not one-per-function) would make the loud count reflect the true backlog.

---

## `Type::Template` variant — deferred pending parametric-builtins feature

The `Type::Template(u32)` encoding is locked in [ADR 004](docs/adr/004-type-template-encoding.md).
The variant itself is NOT yet added to the `Type` enum — it has no producer until the
parametric-builtins feature is implemented.

**Prerequisite for landing the variant:**

- Add `Type::Template(u32)` to the `Type` enum.
- Add `FunctionSig.type_param_count: u32` (default 0, `#[serde(skip_serializing_if = "is_zero")]`).
- Extend `ValidateNoEscapedTypeVars` with one match arm for `Template` (the pass is arm-ready).
- Implement constraint-collector per-call-site instantiation: allocate `type_param_count` fresh
  `TypeVarId`s, emit `Equal(arg_var, T_i)` / `Equal(result_var, T_i)` constraints.

This entry supersedes/refines the parametric-FunctionSig design notes in the entry below
("Parametric FunctionSig `(T, T) → T` for `_any` builtins") — that entry records the
*motivation and constraint-collector mechanics*; ADR 004 records the locked *encoding decision*.
Do not delete that entry; the two are complementary.

## Risk 2: Parent-level Unknown fields emit `: unknown` despite `RedundantInheritedFieldPrune`

`RedundantInheritedFieldPrune` prunes child fields that are redundant relative to an ancestor.
It does NOT fix a field that is `Unknown` on the PARENT itself — those are separate
inference-completeness gaps and emit `: unknown` on the parent class. Confirmed observed
on Dead Estate: `declare height: unknown`, `declare maximumHp: unknown`, etc. on `WorldObject`.

These are genuine RC1001/RC1005 inference gaps; do not widen or suppress them.  Fix path:
improve constraint inference so these fields resolve to concrete types.

**`z` field fixed** — added to both `ensure_gml_object_struct` and `runtime.json type_definitions.GMLObject.fields` (commit `c60d3e06`).

**Deferred debt — `visible`, `persistent`, `alarm`, `solid` type disagreement:**
`runtime/gamemaker/ts/runtime.json` declares `visible`, `persistent`, `alarm`, `solid` as
`"unknown"` (Law-4 suppressions masking real types).  The handwritten GMLObject class types
`visible`/`persistent` as `number | boolean` and `alarm` as `number[]`, while the IR
(`ensure_gml_object_struct`) types visible/persistent as `Bool` and alarm as
`Float(64)[]`.  Three sources disagree.  Honest type for visible/persistent (Bool vs
number|boolean vs numeric) is a modeling decision; fixing the runtime.json `unknown`s may
surface currently-masked errors.  Needs a deliberate pass with before/after error
measurement, not a blind edit.

## Phase 3: register stateful runtime functions with explicit `_rt` param 0 — DONE

All ~1,000 stateful GML runtime functions now have `_rt` as param 0 in their registered
signatures. `rewrite_stateful_calls` and the `stateful_names` set have been deleted from
`crates/backends/reincarnate-backend-typescript/src/emit/rewrites.rs`. Call sites in
`emit/class.rs` and `emit/mod.rs` have been removed. Law 1 and Law 5 violations are closed
for game-to-runtime calls.

### ~~Phase 3 follow-up: `prepend_rt_arg_to_free_calls` deletion~~ DONE

### ~~Phase 3 follow-up: emit_function_imports_with_prefix `stateful_names_out` cleanup~~ DONE

---

## `Op::SystemCall` full elimination (HIGH PRIORITY)

`Op::SystemCall` currently remains in the IR because Flash and Twine frontends still use it. The GML migration (Phases 1–4 of IntrinsicKind elimination) leaves it in place. It must be eliminated from all frontends before the IR is clean.

- **Flash/AS3:** `Op::SystemCall` is used for virtual dispatch patterns and DOM/AVM2 calls. These must become `Op::Call { func: FuncId, args: [receiver, ...] }` where receiver is explicit arg 0. Real virtual dispatch needs a proper dispatch key (vtable slot / method FuncId), not `method: String`.
- **Twine (SugarCube):** Uses `Op::SystemCall` for engine hooks (`SugarCube.Engine.*`). Must become ordinary `Op::Call` to typed FuncIds registered by the Twine frontend.

Until `Op::SystemCall` is gone from all frontends, the IR carries engine-specific calling-convention knowledge in core. The linear emitter's `Expr::SystemCall` branch and `build_intrinsic_calls_map` are both dead weight.

**Blocked on:** Flash and Twine frontend migration (separate scope from GML IntrinsicKind work).

## Engine-Specific Logic in `reincarnate-core` (HIGH PRIORITY — MUST FIX BEFORE FURTHER INFERENCE WORK)

**Law 2 violation:** `reincarnate-core` is supposed to be engine-agnostic — frontends know
the source engine, backends know the target language, core knows neither. Several core
transforms currently contain GML-specific logic with no GML acknowledgment, making them
silently wrong for other source languages and hiding real inference gaps behind workarounds.

### Confirmed violations (production code, not tests or comments)

**1. `transforms/constraint_collect.rs:243-245` — `Op::Add` excluded for GML reasons**

```rust
// Op::Add is excluded — overloaded for string concatenation in GML,
// so result type cannot be assumed to match operand types.
Op::Add(_, _) => {}
```

`Op::Add` is excluded from constraint collection across ALL languages because GML overloads
`+` for string concatenation. Flash/AS3 has the same overloading issue, so the exclusion
happens to be correct for both — but the reasoning is GML-specific and the fix is wrong:
the correct solution is to model `Add` as a builtin call with an overloaded signature
(Phase 9 / RuntimeRegistry), not to silently drop all arithmetic constraints.

**Fix:** This is a real limitation of the current Op-based IR. Document it clearly as a
known gap. The real fix is Phase 9 (arithmetic ops as typed builtin calls). Until then,
the comment should reference Phase 9, not GML.

**2. `transforms/constraint_solve.rs` — memory coherence block is a GML-specific
backward-inference monkeypatch (added 2026-03-22, commit 75de59e)**

The block scans `Store`/`Load` instructions, finds allocs whose stored values are Unknown
but whose load results ConstraintSolve typed as numeric, and back-propagates the numeric
type to the stored values. Rationale: `getInstanceField` always returns Unknown even when
the field is provably numeric (e.g., when the loaded value is used as `loaded - 1`).

This is wrong for three reasons:
- It is GML-specific (`getInstanceField` is a GML runtime call), but lives in a general
  core pass with no `grep for GML` hit.
- It is a monkeypatch: the root cause is that `ResolveInstanceField` returns Unknown when
  the receiver is Unknown (line 737 in type_infer.rs: `_.unwrap_or(Type::Unknown)`).
  The fix belongs in receiver-type propagation, not post-solve backward inference.
- Backward inference from *use* to *definition* fights the forward dataflow invariant:
  a value's type should come from its defining instruction, not from downstream uses.

**Fix:** Remove the memory coherence block. The correct fix is to make the receiver
Unknown chain resolvable — either by propagating Instance types through Unknown receivers
via `HasField`-style constraints (Phase 7 / HM inference), or by adding a dedicated pass
that infers receiver types from cross-object field access patterns. Until that fix exists,
the gap must remain visible (as Unknown in the IR and RC diagnostics) rather than silently
papered over.

**Note on `constraint_collect.rs:243` `Op::Add` exclusion:** the comment "overloaded for
string concatenation in GML" should say "overloaded for string concatenation in GML and
AS3 — correct general behavior is Phase 9 (arithmetic ops as typed builtin calls)."

**3. `transforms/constraint_collect.rs:~907–964` — guard silently drops `Equal(arg_var, param_ty)` constraints on concrete type mismatch**

When emitting call constraints, the guard checks whether `arg_ty` and `param_ty` are both
concrete and differ; if so, it drops the `Equal(arg_var, param_ty)` constraint entirely
rather than emitting it. This suppresses type errors from the solver.

Root cause: the GML frontend maps `DataType::Variable` + `+` to `add_f64` even when the
accumulator is `String`. This produces a `String` vs `Float64` mismatch in the IR —
`arg_ty = String`, `param_ty = Float64` — and the guard was added to prevent the solver
from choking on the mismatch rather than fixing the upstream callee selection.

This is a Law 2 violation: the guard exists solely because one frontend emits IR with
mismatched call signatures. The correct fix is to make the GML frontend select the right
callee for the actual accumulator type (polymorphic add, or infer type before callee
selection), then remove the guard.

**Fix:** GML frontend `DataType::Variable` callee selection must be fixed first:
- When the accumulator type is `String`, select `concat_str` (or a polymorphic add with
  a `String`-typed overload) rather than `add_f64`.
- After the frontend fix, the guard in `constraint_collect.rs` becomes dead code and
  must be removed.

**Blocked on:** GML frontend `DataType::Variable` callee selection fix.

### Why "fix the root cause" is the right call

Every one of these workarounds papers over a gap that would otherwise be visible as
`Unknown` in the IR and as RC diagnostics. The gap IS the signal — it tells you exactly
where inference needs improvement. Hiding it in core passes:
- Makes the RC diagnostic counts wrong (unknown-by-inference looks like known)
- Blocks Phase 9 (RuntimeRegistry) by making the current workarounds load-bearing
- Violates Law 2 (engine-specific logic in core)
- Creates false confidence that inference is better than it is

### Action items

- [x] **Remove memory coherence block from `constraint_solve.rs`** — done 2026-03-27
  (commit 4a10069). The block had zero effect on error counts when removed; the
  "GML-specific" characterization turned out to be wrong (it affected all frontends equally,
  but removal showed it was not load-bearing for any of them either).
- [x] **Fix `constraint_collect.rs:243` comment** — no GML-specific comment remains in constraint_collect.rs; the referenced comment was removed in a prior refactor.
- [x] **Remove concrete-type-mismatch guard from `constraint_collect.rs`** — guard at ~907–964 that silently dropped `Equal(arg_var, param_ty)` constraints removed (2026-05-24). Dead Estate error count unchanged: 0→0.
- [ ] **Fix GML frontend `DataType::Variable` callee selection** — currently maps `Variable + <anything>` to `add_f64` regardless of accumulator type; must select the correct callee (e.g. `concat_str` when accumulator is `String`).
- [ ] **Root-cause fix for `ResolveInstanceField` with Unknown receiver** — the real
  source of Dead Estate's ~1,500+ remaining Unknown instance field reads. Design: when
  receiver is Unknown but field name is a constant, emit a `HasField(receiver, field,
  result)` constraint into the HM solver. The solver can then match the field against known
  struct schemas. This is the Phase 7 path; until then these remain visible Unknown values
  with RC diagnostics.

---

## Inference Improvement Loop (ACTIVE)

**Workflow:** diagnose with `--dump-inference-failures` → identify next fix → implement → measure → repeat.

### Design goal: `Unknown` should be rare and `is_concrete(Unknown) = true`

`Type::Unknown` is a concrete, final type meaning "inference verified this is genuinely
unknown." `Type::Var(_)` (unbound) means "free, not yet constrained." These have
fundamentally different semantics, but `is_concrete` currently returns `false` for both —
causing Unknown and Var to be treated identically throughout the solver.

**Why `is_concrete(Unknown) = true` is correct:**
- Unknown is not a "try again" signal — it's a real type in our system
- Treating it as non-concrete causes the solver to create TypeVars for Unknown values
  and attempt to improve them, which is wasteful and semantically wrong
- An unbound TypeVar is open/polymorphic; Unknown is decided/final

**Why we can't flip this yet:** TypeInference currently writes `Unknown` for almost
everything it doesn't immediately know. If `is_concrete(Unknown) = true`, ConstraintSolve
stops improving Unknown values (they'd be treated as already concrete). We need TypeInference
to write concrete types for all deterministic ops first — then Unknown becomes truly rare,
meaning "we couldn't infer this after exhausting all analysis."

**Rename note (commit `0bb06399`):** `Type::Var` was renamed to `Type::InferVar` to clarify that it is a solver-internal inference variable, not a user-visible type parameter. All existing mentions of `Type::Var` in this file refer to the same concept under its old name.

**Path:**
1. Fix TypeInference to write concrete types for all deterministic ops (Cmp→Bool, Not→Bool,
   BoolAnd/BoolOr→Bool, MakeClosure→Function, Select→branch type, etc.) ← IN PROGRESS
2. Fix Step 2b to link caller results to callee's `return_var` (TypeVar) unconditionally —
   currently gated on `is_concrete(sig.return_ty)` which blocks the interprocedural bridge
   when callee return type is Unknown (even if `return_var` would resolve to concrete)
3. Make `is_concrete(Unknown) = true` — then Unknown in output means "confirmed failure"
4. Retire `CallSiteTypeFlow` and `CallSiteTypeWiden` — subsumed by the unified solver ← DONE (Phase 7)

**Interprocedural bridge gap (Step 2b):** Currently, call result types are only linked to
callee return types when `sig.return_ty` is concrete. But `sig.return_ty` is TypeInference's
snapshot (stale), while the callee's `return_var` TypeVarId is the live HM arena node.
If we link via `return_var` unconditionally, the joint solver propagates concrete types
from inside callee bodies to callers — even when TypeInference wrote Unknown for the
callee's return type. This fix is blocked behind the TypeInference deterministic-ops fix
(step 1 above), since linking via return_var when callee body is all-Unknown is a no-op.

**sig.return_ty writeback — done (2026-03-20, commit d293e66):** Structurally correct;
writes inferred return types back to sig. Measured impact unclear — emit cache was stale
across agent measurements. True ground truth: **13,496 TS errors** (measured via
fresh emit after all commits through d72672d, consistent across all checkpoint checks).

**Step 7 inference diagnostics — done (2026-03-20, commits e8e80b0, d72672d+):**
RC1001/RC1005 emission in ConstraintSolve2. MakeClosure→Function(sig) in TypeInference.
Return_var interprocedural bridge (Step 2b) — structurally correct, no measurable effect
yet (callee bodies lack enough constraints for information to flow).

Dead Estate breakdown (13,496 TS errors, all inference improvements through Step 2b):
- Total remaining Unknown: 56,387
- RC1001 No constraints: 54,354 — by Op: Cast(17,862), Call(13,771), GetField(10,664),
  GlobalRef(3,804), MakeClosure(2,293→0 after d72672d), CallIndirect(2,281),
  SystemCall(1,922), GetIndex(1,025), Arithmetic(~732)
- RC1005 Inherited Unknown: 2,033

Note: Cast RC1001s are largely false positives (Phase 1 binds cast targets directly,
not via arena constraints). Real gaps: Call, GetField, GlobalRef, CallIndirect.

**Error progression (Dead Estate TS errors):**
- 13,496 → baseline (Phase 7 HM solver)
- 7,531 → after GML stdlib register_runtime fix (extern_sigs was dead) + arithmetic builtins
- 6,965 → after Int32/Int16 constants → Float(64) (−566)
- 5,959 → after Float(64) arithmetic builtins in core (−1,006)
- 4,588 → after HM Alloc handler TypeVar aliasing fix in constraint_collect.rs (−1,371,
  commit 2026-03-29): Alloc reused frontend-local TypeVarIds as global arena indices,
  aliasing unrelated values and corrupting Store/Load inference across all functions
- 2,859 → after mem2reg Var(_) refinement fix (−1,729, commit 2026-03-29): promote_multi_store
  only triggered effective-type refinement on Type::Unknown, not Type::Var(_), leaving
  multi-store allocas with Var-typed cells that emitted as `unknown` even when all stored
  values were Float(64)

**Current breakdown (2,170 errors, 2026-04-10):**
- TS18046 (1066): variable of unknown type — unresolved local vars, argument params, instance fields
- TS2345 (477): unknown arg to typed param — function parameters typed unknown + local var cascade
- TS2571 (187): property access on unknown — GetField/HasField inference gap
- TS2304 (128): cannot find name — missing runtime sigs (gpu_set_colourwriteenable etc.)
- TS2322 (122): type mismatches — unknowns assigned to typed positions
- TS2307 (58): cannot find module — `../../_runtime` import path (58 files)
- TS2339 (36): property doesn't exist — field access on void/number typed values
- TS7053 (32): can't index type — float-typed results used as object index
- TS2362 (19): arithmetic LHS not numeric — remaining void/unknown in arithmetic
- TS2365 (14): arithmetic comparisons with void — residual game stubs
- TS2749 (10): value used as type — GML constructor function structs emitted as functions
- TS2538 (9): unknown as index — loop counters with unresolved types
- TS2363 (5): arithmetic RHS not numeric
- TS2678 (4): comparison involves void
- TS2355 (2): function must return a value
- TS2740 (1): type missing properties

**Baseline (1,482 errors, 2026-03-30):** Pre-FuncId-migration baseline; current 2,170 reflects:
- Remaining ~1,250 unknown-typed variable errors (TS18046) from unresolved inference
- ~500 unknown-arg errors (TS2345) cascading from unknown locals/params
- ~130 missing runtime signatures (TS2304) for GML functions not yet in runtime.json

**Recent wins (2026-04-10):**
- user_func_registry filter: FUNC chunk entries for runtime builtins (string, max, min, etc.)
  no longer create void stubs that shadow runtime signatures; 5,875 → 2,170 (−3,705)
- DataType::Variable → Type::Var(fresh) (3bfddab): untyped GML vars now constrainable by solver
- Op::Call { func: FuncId } migration (330bed7): string-based dispatch eliminated
- gml_syscall removed from core (fd100d5): Law 2 fix
- register_arithmetic_any_builtins moved to GML frontend (b3bba3f): Law 2 fix
- arrayLocalSet → Op::SetIndex (4d53495): eliminates fake "arrayLocalSet" runtime call
- color_/colour_ aliases via register_alias (4593c27): correct multi-name registry, no hacks
- func_names from all functions in class/free-func emit (35aa68f, fd4d16b): fixed TS2304
  regression (23,566 → 3); game-defined names now shadow runtime names correctly

**Recent wins (2026-03-30):**
- GMLObject StructDef + parent-chain inference (21ce264): enabled field type resolution
  across the GMLObject hierarchy, major reduction in TS2345/TS18046/TS2571
- super() in derived constructors (5045503): fixed TS17009 (16,161 → 0)
- strip_void_returns in class methods (72497fc): fixed TS2322 void-return subset (536 → 0)
- any[] → unknown[] in ast_printer.rs (e603681): Law 4 fix, surfaces hidden inference gaps
- number|boolean override widening (class.rs, 13374cb): fixed TS2322 for persistent/visible/solid
- is_array/is_string/is_real etc. → type predicates (1a15f78): Law 4 fix, TS2339: 9→7
- Variable-typed bitwise ops → fresh_var result (87be90e): 1,085 → 1,016
- `add_str` renamed to `concat_str`; Unknown params skip constraint emission in constraint_collect

**Root cause of remaining ~1,017 TS errors — architectural plan (2026-03-30):**

`datatype_to_ir_type(DataType::Variable)` in `translate/mod.rs` has a catch-all `_ => Type::Unknown`
that maps GML's "compiler didn't track the type" tag to `Type::Unknown` (final, solver gives up).
It should map to `Type::Var(fresh_id)` (unbound, solver can constrain from call sites and typed
arithmetic). This is the dominant source of unknown-cascade errors.

**What `DataType::Variable` means:** NOT semantically dynamic. It is the GML bytecode compiler
failing to annotate the type. In practice, almost all `Variable`-typed values are numbers (floats).

**The fix — remaining changes:**

1. **`datatype_to_ir_type(DataType::Variable) → Type::Var(fresh_id)`** in `translate/mod.rs`.
   Requires passing `FunctionBuilder` or a `TypeVarAllocator` down to this call site so fresh IDs
   can be minted per-value. **Blocked on refactor: `datatype_to_ir_type` is a free fn without
   access to `FunctionBuilder`. Needs signature change or a separate helper.**

2. **~~`_any` builtins need real IR bodies, not stubs.~~** Done — `register_arithmetic_any_builtins()`
   now builds TypeCheck+Coerce dispatch chains. `Variable => "any"` still needed:
   `type_suffix_for(DataType::Variable)` must return `"any"` — the current
   `"f64"` is only load-bearing because there is no `add_any` body to call through to.
   `Variable => "f64"` was attempted as a long-term fix but is wrong: it hardcodes a GML
   assumption and poisons String-typed args to Unknown on conflict.

   **BuiltinOverloadSelect is devirtualization, not inference.** It replaces `add_any(a, b)` —
   a real runtime dispatch — with the inlined operator form (`a + b` via `add_f64`) when static
   types are known. The dedicated typed variants exist for performance (no runtime type check),
   not correctness. BuiltinOverloadSelect stays in core — the pass logic (replace calls to
   functions with a `specializations` table once arg types are concrete) is engine-agnostic.
   The GML-specific part is the data: `register_arithmetic_any_builtins` populates
   `Function::specializations` in the GML frontend. Pass in core, data in frontend — no Law 2 violation.

   **Core AST → backend AST migration: DONE.** (2026-04-01) Complete migration:
   1. Moved `rewrite_compound_assign`, `rewrite_post_increment`, `promote_while_to_for` to
      TS backend `ast_passes.rs` (operate on `JsStmt`/`JsExpr` after core→JS lowering).
   2. Collapsed `build_builtin_expr` to emit `Expr::Call { func: "builtin.{op_name}", args }`
      for all builtins. TS backend `lower.rs` expands via `lower_builtin()` match table.
      Bool-operand bitwise ops encoded in name (`bitand_bool_i32`) since backend lacks IR access.
   3. Promoted `BoolAnd`/`BoolOr` from `BinOp` to `Expr::LogicalAnd`/`Expr::LogicalOr`.
   4. Moved `BinOp`, `UnaryOp` to `js_ast.rs`; removed `Expr::Binary`, `Expr::Unary`,
      `Stmt::CompoundAssign` from core AST.

   **Three operations stay in the core AST:**
   - `Expr::Not` — boolean negation, produced by `negate_expr` during control flow reconstruction
   - `Expr::LogicalAnd`/`Expr::LogicalOr` — short-circuit `&&`/`||`, recovered by the
     structurizer from branch patterns and emitted directly by `build_builtin_expr` for
     `and_bool`/`or_bool`.
   - `Expr::Cmp` — comparisons, produced from `Op::Cmp` (still a direct IR op, not a builtin).
     Negation uses `Expr::Not(Cmp(...))`, not CmpKind-flipping — `!(a < b)` and `a >= b` are
     only equivalent for types with total ordering. Whether a type has total ordering depends
     on the source language (floats with NaN, string collation, object comparison semantics),
     so flipping is engine-specific knowledge that doesn't belong in core. Core passes
     (`rewrite_minmax`) also structurally inspect `CmpKind`, which is why `Cmp` can't become
     an opaque builtin call.

   **Why not Op variants?** Adding `Op::IAdd32`, `Op::FAdd64`, etc. to the IR enum is expensive:
   every pass, transform, and typechecker arm must handle each new variant. The `builtin.*`
   named-function approach is correct — builtins are functions.

   **Why not IR bodies for typed variants?** `add_i32`'s body in TS would be `(a + b) | 0`,
   but the same body in Rust would be `a.wrapping_add(b)`. IR bodies encode source-language
   semantics, not target-language syntax — a body can't be both. The backend is the right place
   for target-language-specific emit forms.

   **~~Hard error for called stubs.~~** Done — `validate-called-stubs` pass (RC0005, `Severity::Error`)
   runs after all transforms and flags any `Op::Call` to an unresolved `_any` stub.

   **~~Specializations table → TS overload declarations.~~** Done — `print_function()` emits
   overload signatures from `JsFunction::overloads`, populated from the specializations table.
   Runtime functions with real bodies are no longer filtered from emission.

   **`Op::Call { func: FuncId }` — NEXT.** `Op::Call` currently carries a `String` function name.
   Nothing validates that the name corresponds to a registered function. This caused
   `builtin.add_str` (nonexistent) to flow through the entire pipeline undetected until TS
   emitted broken code. `FunctionBuilder::builtin_type_suffix` composed invalid names from
   type suffixes (`String → "str"` → `builtin.add_str`); the frontend's `arith_callee` had the
   same bug. Both were patched (2026-04-01), but the root cause is string-based dispatch.
   Fix: `Op::Call { func: FuncId, args }`. Construction requires a valid FuncId from the
   module — nonexistent functions are impossible. This is the minimal slice of Phase 9 that
   eliminates the entire class of "call to nonexistent function" bugs.

   **Design (2026-04-04, from failed attempt):** Store registry in `FunctionBuilder` at
   construction time — `registry: HashMap<String, FuncId>` field, set once in `new()`.
   Arithmetic helpers (`add`, `sub`, etc.) use `self.registry` internally; signatures unchanged.
   Add `call_named(&str)` convenience that resolves via `self.registry`. Do NOT pass registry
   as a parameter to every method — that was attempted and cascaded into 45 files of wrong
   plumbing.

   **Prerequisites found during failed attempt:**
   - **Remove `gml_syscall` from core `FunctionBuilder`** — Law 2 violation added in commit
     4f0b416 (Phase 3a). GML frontend should call `fb.call_named("system.method")` or
     `fb.call(func_id)` directly.
   - **Move `register_arithmetic_any_builtins` out of core** — `_any` overloading is
     GML-specific (the comment says so), but the function and its dispatch body builders
     (`build_binary_any_dispatch`, `build_unary_any_dispatch`) live in `module.rs`.
   - **Fix `builtin_type_suffix` catch-all** — returns `"any"` for all non-scalar types
     (Bool, String, Unknown, Var, structs), composing names like `"builtin.add_any"` that
     don't exist in core. The `_ => "any"` arm assumes GML's `_any` builtins exist.
   - **Registration ordering** — GML builtins, intrinsics, and `_any` stubs must be
     registered BEFORE translation, not after `ModuleBuilder::build()`. Currently registered
     on `Module` at lines 234-262 of `lib.rs`, after `mb.build()`. User function names also
     need pre-registration as stubs so FuncIds exist during translation.
   - **`_any` naming convention** — `_any` variants registered as `"add_any"` (no `builtin.`
     prefix), but `builtin_type_suffix` would compose `"builtin.add_any"`. The GML frontend's
     `arith_callee` handles this correctly; core's builder doesn't.

3. **Parametric FunctionSig `(T, T) → T` for `_any` builtins.**
   `add_any`'s FunctionSig should be `(Type::Template(0), Type::Template(0)) → Type::Template(0)`.
   The constraint collector instantiates fresh TypeVarIds per call site for each template index,
   emits `Equal(arg_var, T)` / `Equal(result_var, T)` unconditionally (bypassing the `is_concrete`
   guard), and allows HM to propagate the type through. Unknown inputs *conflict* with T (correctly)
   rather than silently propagating — the call-site TypeVars are freshly allocated with no
   pre-bindings. Requires `Type::Template(u32)` variant in the Type enum; template types must
   never appear in `value_types` (hard error if they do). Also needed for array element type
   inference (`array_get(arr: Array<T>, i: Int) → T`).

   **`Op::Add` and the constraint_collect exclusion:** `Op::Add` is already removed from the IR as
   of Phase 9 arithmetic slice (2026-03-28). `constraint_collect.rs` still has the exclusion comment
   referencing it (lines 243-245) — clean it up when BuiltinOverloadSelect moves.

**TS2571 root cause:** `getInstanceField` on Unknown receiver → field type Unknown.
Fix: HasField reverse-index (TODO below) or better ConstructorStructInfer coverage.

**`getInstanceField`/`getAllField` → `unknown` with scalar + struct casts (2026-04-12):**
`any` removed from runtime.ts. The emitter injects `as number/boolean/string` casts (scalar)
and `as TypeName` casts (struct) at call sites where inference narrowed the field type via
`ResolveInstanceField` constraints. Both scalar and struct casts are enabled.

Dead Estate: 0 → 2062 errors (all genuine inference gaps from unresolved receiver types).

**HasField narrowing made conservative (2026-04-12):** The single-candidate narrowing in
`process_constraint` (constraint_solve_hm.rs) was over-triggering: when a leaf type redefines
a common field ("x", "y", "z") that a non-leaf ancestor also defines, the leaf was the sole
candidate and narrowing fired incorrectly. Fixed by adding a `field_in_non_leaf` guard — if
any non-leaf type defines the field in its own `own_fields`, narrowing is skipped. Both
step-4 (single-field) and step-4.5 (multi-field) paths carry this guard.

**`build_own_fields` enriched with `module.types` (2026-04-12):** `build_own_fields` in
`constraint_solve_hm.rs` was only reading from `module.structs` (frozen frontend snapshot),
missing fields injected by `ConstructorStructInfer` into `module.types`. Now prefers
`module.types[id].fields()` when non-empty; also includes types only in `module.types`.

Dead Estate: 2062 → 2043 errors after HasField narrowing fix + build_own_fields enrichment.

**GML constructor function struct types (TS2749/TS2430/class emit):** Structs like `Button`,
`Menu`, `Section`, `TextPiece`, `Challenge` are GML 2.3+ constructor functions. Current emit:
`export function Button(...)` (free function) + `export interface Button extends GMLObject {}`.
This works for type annotations but is wrong in two ways:

1. **Constructor calls should use `new`** — call sites emit `Button(rt, self, ...)` but should
   emit `new Button(rt, self, ...)`. Requires constructor functions to be emitted as classes.

2. **GetField doesn't traverse the TypeDecl parent chain** — when `section.x` is accessed,
   the type system returns `unknown` instead of `number` (inherited from GMLObject). Fix:
   `GetField` inference must walk `TypeDecl::Object { parent, .. }` to find fields declared
   on ancestor types. Child fields don't need to re-declare parent fields in the interface —
   they inherit them — but inference must follow the chain to resolve them.
   All fields are emitted (including `unknown`-typed ones) — TS2430 errors where a child
   field is `unknown` but the parent has a concrete type are correct inference failure
   signals, not bugs to suppress.

**Correct long-term fix:** emit GML constructor functions as TypeScript classes that extend
GMLObject. Then call sites use `new`, the type system is coherent, and TypeDecl fields emit
as class fields.

**Dead Estate error history:**
- 2026-03-31: 1,017 errors after `DataType::Variable → "any"` fix
- 2026-04-12: 0 errors after three fixes:
  (scalar casts only, struct casts blocked by HasField narrowing bug)
- 2026-04-12: 2062 errors after enabling scalar casts (genuine inference gaps)
- 2026-04-12: 2043 errors after HasField narrowing conservative fix + build_own_fields
  enrichment + struct/scalar casts both enabled. Breakdown:
  - TS2345 (831): unknown assignability (347 unknown→GMLObject instance args)
  - TS2571 (498): property access on unknown receiver (self: GMLObject in event handlers)
  - TS2769 (259): no overload matches (unknown args to overloaded runtime fns)
  - TS2322 (183): type mismatch (includes ~184 number→string in closure params)
  - TS18046 (172): unknown variables (argument0 41, a 27, _bulletDamage 17, argument1 10)
- 2026-04-19: 1869 errors after MakeClosure capture param offset fix (2043 → 1869).
  captures[i] now maps to entry_params[sig.params.len() + i], not entry_params[i].
  The `_self` regular param was consuming captures[0], leaving all capture tvars
  unresolved → unknown. Breakdown:
  - TS2345 (739): unknown assignability
  - TS2571 (497): property access on unknown receiver
  - TS2769 (205): no overload matches
  - TS2322 (177): type mismatch
  - TS18046 (151): unknown variables
  NOTE: check cache returned a false "0 errors" (stale entry from pre-fix state);
  confirmed 1869 via direct tsgo run after clearing ~/.cache/reincarnate/check-*.json.
- 2026-04-19: 51,705 errors after removing `[key: string]: any` from GMLObject.
  The index signature was silencing all TS2339 ("Property X does not exist on GMLObject")
  errors. Now ~50K of these surface as expected: every field access on a GMLObject
  subclass where we haven't emitted a field declaration. This is the correct state —
  the errors reflect a real inference gap, not a regression.

**Remaining known gaps (Dead Estate 51,705 baseline):**

- **GMLObject missing field declarations.** Every SetField op across all GML events must be
  collected per object type and emitted as TypeScript class field declarations. Currently
  ConstructorStructInfer only collects fields from `create` events. Fix: extend to all events.
  Until fixed, all instance field accesses produce TS2339. This is ~50K of the 51,705 errors.

- **Parametric array types.** All arrays are `Array(Unknown)` — element type is not
  inferred. `[1.0, 4.0, 7.0]` produces `unknown[]`; indexing yields `unknown`. No TS errors
  because getters return `any`. Fix: `HasElement` constraint to link array element type to
  push/get operations. Blocked on extending IR and solver.

### After inference is solid

- [ ] **Phase 8 — Core AST + reconstruction.** Structurizer → Core AST; forward
  substitution; `ForEach` lifting. Gate: emitted code measurably cleaner.
- [ ] **Phase 9 — Runtime as IR.** GML runtime as IR functions; `RuntimeRegistry`;
  arithmetic ops as builtin calls (unblocks Phase 3). Gate: same output, M+N architecture.
- [ ] **Phase 3 — Ban `SystemCall` + `GlobalRef`.** Blocked on Phase 9.
- [ ] **Phase 5b/5c — `NameInterner` + extend `NameTable`.**
- [ ] **Row variables** (`Type::Row(RowVarId)`) for anonymous struct inference (GML
  `ds_map`, SugarCube state vars). High impact, high effort.
- [ ] **Union on conflict** — replace `force_rebind(Unknown)` with `Union(t1, t2)`.
  Prerequisite for safely relaxing interprocedural constraint guards.
- [ ] **Constructor struct inference two-phase ordering** — attempted 2026-03-27,
  reverted (commits adc64a1 + cf22f72). The two-phase ordering (CSI
  requires+invalidates type-inference) causes a 4-error regression on Dead Estate
  (1601→1605, verified with full clean build). Root cause not yet identified — need to
  find which 4 errors are new before re-attempting. The companion numeric-compatibility
  fix in `CallSiteTypeWiden` was also reverted: treating Int/Float as mutually compatible
  is target-language knowledge (TypeScript `number`) baked into core — Law 2 violation.
- [ ] **HasField reverse-index (last resort).** After the fixpoint loop exhausts all
  `Equal` and `HasField` constraints, any `HasField { ty: Var(v), field: "x" }` where
  `v` is still unbound means no other constraint resolved `v`'s struct type. At that
  point, look up which structs in the module have field `"x"` and narrow accordingly:
  exactly one match → bind `v = Struct(name)`; multiple → `v = Union([...])` and result
  = union of field types; zero → RC diagnostic (unknown field on unknown type). This is
  genuinely a last resort — the fixpoint should resolve the vast majority of cases first
  via call-site type flow and argument propagation.

## Incremental Rewrite Plan (ACTIVE)

Full design: `docs/rewrite.md` (on `rewrite-v1` branch). Executed incrementally — each phase completes fully (output parity verified on Dead Estate via snapshot diff) before the next begins. No stubs. No deleting working code before its replacement is proven equivalent.

**Quality gate:** `cargo run -p reincarnate-cli -- emit --manifest ~/reincarnate/gamemaker/deadestate/reincarnate.json --dump-function <name> > ~/reincarnate/snapshots/after.txt` and diff against before snapshot. Dead Estate must stay at 0 TS errors throughout.

### Three dimensions being addressed

1. **Structural ad-hoc → clean IR:** ban `SystemCall`, `GlobalRef`, `Copy`, `CoercingEq`/`CoercingNe`, `BoolAnd`/`BoolOr` as distinct ops; introduce `Terminator` enum; `NameInterner`/`NameTable`; declarative pass manager.
2. **Inference:** multi-phase heuristic stack → single-pass HM unification; `Dynamic` → `Unknown` with honest diagnostics; `IntToBoolPromotion` ported to first-class `Bool` type.
3. **Emit quality:** Core AST reconstruction pipeline; `ForEach` lifting; forward substitution; runtime as IR (M+N not M×N).

### Phases

- [x] **Phase 1 — Terminator enum.** Extract control flow from `Op` into an explicit `Terminator` per block. No semantic change; all existing passes adapt. Gate: Dead Estate 0 errors, snapshot identical.
- [x] **Phase 2 — Ban `Copy` + `CoercingEq`/`CoercingNe`.** `Copy` → eliminated by Mem2Reg or substituted inline. `CoercingEq`/`CoercingNe` → `SystemCall` routed through backend rewrite to `JsExpr::LooseEq`/`LooseNe`. Gate: same.
  > **Note:** `CoercingEq`/`CoercingNe` were replaced with `SystemCall("SugarCube.Engine", "loose_eq"/"loose_ne")` → `JsExpr::LooseEq`/`LooseNe` as an interim step. The correct long-term replacement is `Call(js_eq_fn, ...)` via `RuntimeRegistry` (Phase 9). This is tracked.
- [ ] **Phase 3 — Ban `SystemCall` + `GlobalRef` + `MethodCall` (non-virtual).** Engine API calls → typed `Call(FunctionId, ...)` via `RuntimeRegistry`. Gate: same.
  > **Blocked on Phase 9.** `SystemCall` carries a two-part name (`system`, `method`) used by backend rewrite passes to resolve into native JS constructs. Without `RuntimeRegistry` modeling runtime functions as IR functions, any replacement is either a magic-string convention (worse), a sentinel FuncId (more complex), or a no-op rename. The current design is sound: frontends emit `SystemCall` with engine-specific names, core passes them through with declarative type rules (`SystemCallTypeRule`), backends resolve via rewrite passes. `GlobalRef` similarly requires `RuntimeRegistry` to model named globals as first-class IR entities. Phase 2 increased `SystemCall` usage (CoercingEq interim), confirming the dependency. Scope: ~25 files across all frontends, core transforms, core linear IR, and the entire backend rewrite layer. Proceed only after Phase 9 provides `RuntimeRegistry`.
  >
  > **`Op::MethodCall` design constraint (2026-04-05):** `Op::MethodCall { receiver, method: String, args }` is only valid for genuine runtime polymorphism (virtual dispatch). GML has no virtual dispatch — all GML `Op::MethodCall` uses are wrong and must become `Op::Call { func: FuncId, args: [receiver, ...] }` (receiver is just arg 0). Flash/AS3 has real virtual dispatch, so the concept is valid there, but `method: String` still needs to become a proper dispatch key (vtable slot / method FuncId) — same unresolved-string problem as `Op::Call { func: String }`. For runtime calls (`_rt.method(...)`): these are not virtual dispatch — `_rt`'s type is always `GameRuntime`. Correct representation: `Op::Call { func: FuncId, args: [rt_param, ...] }` where `rt_param` is an explicit param 0 on every translated GML function (truthfully reflecting that the TS already has `(_rt, self, ...args)` signatures). `IntrinsicKind` is eliminated: there is no `Op::Call → Op::SystemCall` translation step because `Op::SystemCall` is gone. This is Phase 9 scope.
- [x] **Phase 4 — `Dynamic` → `Unknown`.** `Type::Dynamic` removed from the IR; all uses replaced with `Type::Unknown`. The TS backend currently still emits `any` for `Unknown` to maintain 0 TS errors; switching to `unknown` surfaces ~21K TS18046/TS2345 errors because GML params (`_self`, `_other`, loop vars) are typed Unknown rather than their concrete class types. Emitting `unknown` is gated on inference improvements that eliminate Unknown from param positions.
- [ ] **Phase 5 — `NameInterner` + `NameTable`.**
  - [x] **5a — `NameTable` for function names.** `NameTable` stores `PrimaryMap<FuncId, String>` on `Module`. All core transforms and pipeline code read via `module.func_name(id)`. `Function::name` kept for backward compat (frontends, backends still write it); `ModuleBuilder::add_function` populates both. `rebuild_name_table()` for deserialized IR. Gate: Dead Estate 21110 TS errors (unchanged).
  - [ ] **5b — `NameInterner`.** Collision-free name generation replacing ad-hoc dedup in sanitize pass and frontend name logic.
  - [ ] **5c — Extend to other name fields.** Migrate struct, class, global, field, enum names into `NameTable`. Remove `Function::name` once all consumers read from `NameTable`.
    - [x] **5c part 2 — Type names in NameTable.** `NameTable` now stores `PrimaryMap<TypeId, Option<String>>` as the authoritative type name source. `Module::intern_type`, `Module::intern_enum`, `Module::intern_type_classref`, and `TypeInterner::intern`/`classref` all populate `name_table.type_names` on every push to `module.types`. `Module::rebuild_type_index()` also rebuilds `name_table.type_names`. `NameTable::type_name(TypeId)` and `type_name_expect(TypeId)` added. `Module::type_name(TypeId)` now reads from NameTable. `TypeInterner::name_of(TypeId)` migrated to read NameTable. `#[serde(default)]` on `name_table.type_names` handles old IR files. `TypeDecl::name`/`name_expect` remain as fallback read path; migrating call sites is deferred (below).
    - [ ] **5c part 1 — Remove `Function::name`.** Blocked by ~50 read sites still using `func.name` directly. Core transform sites in `constraint_solve2.rs` and `constructor_struct_infer.rs` migrated to use `module.func_name(id)`. Remaining blocked sites:
      - `coroutine_lower.rs:501` — constructs `Function { name: orig_func.name.clone(), .. }` struct literal; requires `Function::name` to exist for construction.
      - `linear/tests.rs` (20+ sites) — tests build standalone `Function` via `FunctionBuilder` with no module context; `func.name` is the only way to get the name. Unblocked only when tests use `Module`/`FuncId`.
      - `builder.rs:737,894` — `add_function` reads `func.name` to populate NameTable; test assertion uses `func.name`. Unblocked when `FunctionBuilder::build()` takes a name parameter instead.
      - `backends/` (~15 sites in `scaffold.rs`, `class.rs`, `emit/scaffold.rs`, `emit_flash_traits.rs`, `sanitize.rs`) — iterate `module.functions.values()` without FuncId; need to switch to `.iter()` or `.keys()`.
      - `frontend-gamemaker/call_site_arity_widen.rs` — iterates `.values()`.
      - `frontend-twine/lib.rs`, `harlowe/translate.rs` — use `func.name` in tests and logic.
      - `reincarnate-cli/main.rs` — debug/dump code.
    - [ ] **5c part 3 — TypeDecl::name field removal.** `TypeDecl::Object.name` and `TypeDecl::Enum.name` still exist; all call sites of `TypeDecl::name()` / `TypeDecl::name_expect()` use them. These can be removed once all call sites are migrated to `module.name_table.type_name(id)` / `module.type_name(id)`. Deferred because most sites lack clean access to `TypeId`. Globals, fields, and enum variants lack typed IDs and cannot migrate until `GlobalId`/`FieldId` are introduced.
- [x] **Phase 6 — Declarative pass manager.** `requires`/`invalidates` declarations replace implicit ordering. Gate: same.
  > **Fixpoint is correct but not efficient.** `--fixpoint` re-runs all passes on all functions until nothing reports `changed`. The correct design is function-level incremental invalidation: each pass declares which functions it dirtied, and subsequent passes only re-run on those functions. Currently fixpoint iterates the full module each round — cheap if convergence is fast (1–2 rounds), but O(N×passes×rounds) in the worst case. Tracked for a future phase.
- [x] **Phase 7 — HM inference.** Single-pass constraint collection + solve replaces four coupled passes (`type_infer`, `call_site_flow`, `call_site_widen`, `constraint_solve`, `constraint_solve2` deleted). `is_concrete(Unknown) = true`; `Type::Var` for inference gaps; unconditional write-back; alloc/store/load cell constraints; void event handlers + `strip_void_returns`. Gate: 13,496 → 8,102 errors (−40%).
- [ ] **Phase 8 — Core AST + reconstruction pipeline.** Structurizer → Core AST; forward substitution; `ForEach` lifting. Gate: emitted code measurably cleaner.
  > `rewrite_loop_to_while` wired up (was implemented but not registered). `promote_while_to_for` already active.
- [ ] **Phase 9 — Runtime as IR.** GML stdlib expressed as IR *functions with bodies*; `RuntimeRegistry` maps stdlib names to `FuncId`s. Frontends emit the runtime the same way they emit game code — same `Function` struct, same pipeline. Backends emit runtime functions like any other function; no per-backend runtime package. Deletes `runtime/gamemaker/runtime.ts` as handwritten source (it becomes generated output). Gate: same output, M+N architecture (not M×N).
  > **Arithmetic slice done** (2026-03-28): `Op::Add/Sub/Mul/Div/Rem/Neg/Not/BoolAnd/BoolOr/BitAnd/BitOr/BitXor/BitNot/Shl/Shr` removed; typed builtins (`builtin.add_f64`, etc.) registered in core. `BuiltinOverloadSelect` pass (post-HM) replaces `_any` calls with typed variants via `Function::specializations` table (2026-03-29). **Math stdlib bodies done** (2026-03-29): `lengthdir_x/y`, `point_distance`, `degtorad`, `radtodeg`, `dsin/dcos/dtan`, `darcsin/darccos/darctan/darctan2`, `arctan2`, `point_direction`, `sqr`, `power`, `logn`, `log2`, `log10`, `exp`, `clamp`, `lerp` — 22 functions in `runtime_bodies.rs`. Core math leaf builtins registered (`sin_f64`…`hypot_f64`), TS backend dispatch added. Remaining: string stdlib bodies, `RuntimeRegistry` as `FuncId`-keyed map, Phase 3 unblock. Done: IR inliner (`inline_hint: Always`) — bodies are inlined at call sites for single-block/single-return functions; TS backend IR body emission — `collect_runtime_body_fids` emits `InlineHint::Always` functions to `_runtime.ts` as fallback for multi-block bodies.
  > **Three categories of builtin, by emit strategy:**
  > 1. **Operator builtins** (`add_f64`, `sub_f64`, `bit_and_i32`, …): backend emits as operator syntax at emit time — safe because args are already values, no body expansion needed. `add_f64(a, b)` → `a + b`.
  > 2. **Stdlib functions with IR bodies** (`lengthdir_x`, `hypot` if no native, …): IR body expressed using other builtins. IR inliner (not backend) handles `inline_hint: Always`. Multi-statement bodies require IR-level inlining to avoid double-evaluation (emit-time inlining is AST-level and unsafe).
  > 3. **Leaf builtins** (`cos_f64`, `sin_f64`, `sqrt_f64`, `hypot_f64` on targets with native hypot, …): **no IR body**. Backend provides the native implementation via a string-keyed dispatch table (`"builtin.cos_f64"` → `Math.cos`). This is unavoidable — these are target-language primitives that cannot be expressed in IR without circularity. This is NOT a Law 2 violation: it is target-language knowledge in the backend, which is correct. The M+N win is that engine-specific stdlib (GML, Flash, SugarCube) moves to IR bodies; the leaf math set is small and fixed.
  > **No `IntrinsicKind` enum**: an enum variant per intrinsic breaks IR deserialization on schema changes. Backend dispatch is string-keyed (`"builtin.cos_f64"` → native). Same approach as `add_f64` already uses; no new mechanism needed.
  > **`inline_hint`**: `Function::inline_hint: Option<InlineHint>` (Always / Never / Default). Single-expression wrappers (operator and leaf builtins) get `Always`; multi-statement bodies let the inliner decide. **Not** a backend annotation — drives the IR inliner pass. Serialization-safe: `#[serde(default)]`, missing = Default.
  > **Monomorphic overload selection**: `_any` builtins resolved post-HM by `BuiltinOverloadSelect` via `Function::specializations`. GML-polymorphic stdlib functions (e.g. `int :: Real → Int | String → Int`) get separate monomorphic IR functions (`int_real`, `int_string`, …); unknown-arg fallback emitted only at genuinely unresolved sites.
  > **Core builtin registry**: `register_core_builtins()` in `Module::new()`. Typed arithmetic + `_any` stubs with specialization tables already registered. Still needed: trig/math leaf stubs (`sin_f64`, `cos_f64`, `sqrt_f64`, `abs_f64`, `floor_f64`, `ceil_f64`, `ln_f64`, `exp_f64`, `atan2_f64`, `pow_f64`) + corresponding backend dispatch entries. Then stdlib bodies become writable.
  > **Full stdlib sweep (NEXT SESSION):** Systematically implement IR bodies for the entire implementable GML stdlib using parallel subagents. Each agent takes one module, reads `gml-docs <fn>` for each function + cross-references the handwritten TS impl in `runtime/gamemaker/ts/gamemaker/` for GML-specific quirks (coordinate conventions, edge cases), writes IR bodies in `runtime_bodies.rs`, verifies against error count. Run agents in parallel — one per module. Suggested split:
  > - **Math/trig** (remaining): `sin`, `cos`, `tan`, `arcsin`, `arccos`, `arctan`, `sqrt`, `abs`, `sign`, `round`, `floor`, `ceil`, `frac`, `int`, `real` (monomorphic variants)
  > - **Random**: `random`, `irandom`, `random_range`, `irandom_range`, `choose` — need `_rt` RNG state; bodies call into runtime state
  > - **String**: `string_length`, `string_copy`, `string_pos`, `string_upper`, `string_lower`, `string_repeat`, `string_char_at`, `string_ord_at`, `string_byte_at`, `chr`, `ord`, `string_count`, `string_replace`, `string_delete`, `string_insert` — need string builtins in core first
  > - **Array**: `array_length`, `array_create`, `array_copy`, `array_push`, `array_pop`, `array_contains`, `array_sort`, `array_reverse`, `array_concat` — need array builtins
  > - **Instance/object math helpers**: `distance_to_point`, `distance_to_object`, `move_towards_point` — implementable with existing builtins
  > - **Color/draw math** (pure math only, not draw calls): `make_color_rgb`, `make_color_hsv`, `color_get_red`, etc.
  > Skip anything that requires `SystemCall` (draw, file I/O, network) — those stay as stubs until Phase 3.
  > **Blocked on**: `RuntimeRegistry` (FuncId-keyed stdlib map), then Phase 3 (`SystemCall` ban).
  > **`_rt` migration design (2026-04-05, updated 2026-05-25):** The migration is largely complete. GML-translated user functions have `_rt: GameRuntime` as explicit IR param 0 (pushed unconditionally by `build_signature_with_args`). Stateful runtime stubs have `_rt` prepended to their param lists by the frontend (`STATEFUL_RUNTIME_FUNS` + `sig.params.insert(0, rt_ty)`); `lower_call` promotes it to the JS receiver (`_rt.method(rest_args)`). `rewrite_stateful_calls` and `prepend_rt_arg_to_free_calls` are deleted. Remaining hack: inside generated class methods, `_rt` comes from `this._rt` (a property set on GMLObject by `_instanceCreate` in handwritten `runtime.ts`) via `rt_via_this: true` in `LowerCtx` — this is necessary given TS class method calling conventions and is not blocking IR body work. The actual blockers for IR bodies of stateful functions are: (1) `GameRuntime` as IR struct with typed fields (so IR bodies can access `rt.rng`, `rt.audio`, etc.); (2) platform API design (functions calling browser APIs need a defined platform API to call into from IR).
  > **`int64`/`string` lowered at call site (2026-05-25):** GML frontend intercepts `int64(x)` → `Cast(x, Int(32), Coerce)` and `string(x)` → `Cast(x, String, Coerce)`. TS backend already handles both Cast variants correctly (`(x)|0` and `String(x)`). Removed from `function_modules`. No IR bodies needed.

### Immediate next steps (2026-05-25)

**IR formalization** — The IR currently has informal semantics that work for the TypeScript target but are undefined for others. Required before modeling complex mutable state and before adding non-JS backends:
- [ ] Formally define value vs. reference semantics for `Array`, `Map`, and `Instance` types
- [ ] Define mutation rules: when does `SetIndex`/`SetField` on a value obtained via `GetField` propagate back to the parent struct?
- [ ] Adversarial audit rounds specifically against the IR design once formalized

**XorGen / RNG IR bodies** — partially done. `XorGen` and `MathState` registered as `TypeDecl::Object`; `GameRuntime._math: MathState` field added; `xorgen_next`, `random`, `random_range` have IR bodies (commit 13ff7bf2).
- [x] Define `MathState` and `XorGen` as IR structs
- [x] `xorgen_next` as IR body (XorShift128 ring-buffer step, InlineHint::Default)
- [x] IR bodies for `random`, `random_range`
- [ ] `irandom` — modulo-bias rejection loop requires multi-block IR (loops not yet expressible in the IR bodies pattern); deferred until IR body loop support exists
- [ ] `irandom_range` — same blocker as `irandom`
- [ ] `random_set_seed` / `xorgen_init` — constructor with initialization loop + 256-call warmup; deferred until loop IR bodies are supported
- [ ] `random_get_seed` — XorGen does not store the original seed; would need to add a `seed: i32` field to XorGen and thread it through `xorgen_init`
- [ ] `randomize` — blocked on platform API: needs `currentWallTimeMs()` which is a backend primitive (browser `Date.now()`)
- [ ] `choose` blocked on varargs — handle at call site once varargs design is settled

**`median`** — last function in the non-stateful `gamemaker/math` `function_modules` entry. Requires sorting; not expressible as a fold. Deferred until IR can express sorting or a backend primitive approach is designed.

**Pure IR bodies still missing** — survey `runtime.ts` for pure-GML functions (no `_rt` dependency) not yet in `runtime_bodies.rs`. Candidates: `array_create`, `array_push`, `array_pop`, `array_resize`, `array_sort`, `array_reverse`, `array_filter`, `array_map`, `array_concat`, and others. These can be done incrementally without IR formalization.
  > **Done (2026-05-25):** `array_get`, `array_set`, `array_height_2d`, `point_in_triangle` all have pure IR bodies in `runtime_bodies.rs` (commit 69c7d52a). `array_get`/`array_set` use GetIndex/SetIndex ops; `array_height_2d` delegates to `array_length_arr`; `point_in_triangle` uses barycentric method.

### Out of scope until designed

- **Twine `State.get`/`State.set`:** banned by Phase 3 (`SystemCall` removal) but the replacement for temp vars (`_args`, `$vars`) passed between passages needs explicit design first. Tracked separately below.
- **Platform API design:** `shared/platform/` is the official platform API boundary. Any code in `runtime.ts` calling browser APIs directly (`canvas.getContext`, `requestAnimationFrame`, WebGL2, DOM events, etc.) is a bug — it bypasses the M+N boundary. Platform API design requires careful deliberate work before `runtime.ts` can be deleted; it is NOT a prerequisite for IntrinsicKind elimination.

## Pipeline Architecture Redesign (HIGH PRIORITY — BACKLOG)

### Problem

The transform pipeline is structurally clean (`Transform` trait, ordered pass list,
fixpoint mode) but the passes themselves have no principled foundation. Type inference
in particular has accumulated into an ad-hoc heuristic stack:

- **Four coupled passes** (`TypeInference`, `CallSiteTypeFlow`, `ConstraintSolve`,
  `CallSiteTypeWiden`) approximate what should be a single constraint collection + solve
  phase. Their ordering contracts are implicit — documented only in CLAUDE.md and
  MEMORY.md, not enforced in code.

- **`TypeInference` is a pipeline-within-a-pipeline.** `build_global_types` runs its own
  multi-pass loop (up to 4 iterations of: global store scan → use-site heuristics → struct
  schema inference → re-run `infer_function`) inside a single `Transform::apply()`.

- **Analysis and transformation are interleaved in every pass.** No pass declares what it
  reads or writes. You can't add a pass that runs "after ConstraintSolve but before Mem2Reg"
  without reading all the code to understand the implicit ordering.

- **`Type::Dynamic` removed (Phase 4).** All inference gaps are now `Type::Unknown`.
  The TS backend still emits `any` for `Unknown` to maintain error counts; switching
  to `unknown` is gated on inference improvements that give concrete types to params
  like `_self`, `_other`, and loop variables (~21K TS18046/TS2345 errors otherwise).

- **`RedundantCastElimination` exists to clean up `TypeInference`'s mess.** Casts that
  become redundant after inference runs are emitted by one pass and cleaned up by another.

- **`ConstantFolding` runs twice** because `Mem2Reg` creates new folding opportunities —
  a symptom of no pass invalidation tracking.

### Proposed Redesign

A constraint-based type inference architecture, drawing on crescent's
`constrain → solve → unify` design (`lib/type/static/` in `~/git/rhizone/crescent/`).
Crescent infers types for Lua — a dynamically-typed language — from usage patterns alone,
which is structurally identical to recovering types from untyped bytecode.

**Key ideas:**

1. **Single constraint collection pass** — one IR walk that emits typed constraints from
   every instruction. Each `Op` variant becomes a constraint generator:
   - `Op::Add(a, b)` → `C_CALLABLE(builtin_add, [a, b], result)` (arithmetic ops are builtin calls)
   - `Op::GetField { object, field }` → `C_HAS_FIELD(object, field, result)`
   - `Op::Call { func, args }` → `C_CALLABLE(func, args, result)`
   - etc.

2. **Unified solver** — processes the full constraint set, binds type variables to concrete
   types via HM-style unification. Interprocedural constraints (call sites) handled in the
   same solve pass, not a separate pipeline stage.

3. **`Unknown` → `unknown` in TypeScript** (Phase 4 completed the IR side; TS emission
   gated on inference improvements — see Phase 4 entry above).

4. **Conflict resolution** — contradictory constraints produce union types automatically,
   not silently dropped or defaulted.

5. **Pass ordering becomes explicit** — constraint collection and solving are separate,
   declared phases. Frontend-specific constraint generators (e.g. `GlobalStore`/
   `ResolveGlobalType` rules) plug in at collection time, not via extra pipeline passes.

### Pass Ordering

The structural passes (`Mem2Reg`, `CoroutineLowering`, `CfgSimplify`, `DCE`,
`ConstantFolding`) are individually fine, but their ordering contracts are implicit:

- "Mem2Reg must run after CoroutineLowering" — CoroutineLowering introduces
  Alloc/Store/Load chains that Mem2Reg promotes.
- "ConstantFolding runs twice" — because Mem2Reg creates new folding opportunities;
  the second run exists only because nothing declares the invalidation.
- "extra_passes run last and see stale types" — GmlLogicalOpNormalize modifies block
  args without updating `value_types`, causing a known bug with no enforcement.

None of this is declared or validated anywhere — it's implicit knowledge in CLAUDE.md.

**Solution:** extend `Transform` with dependency declarations:

```rust
fn requires(&self) -> &[&str] { &[] }    // prerequisite pass names
fn invalidates(&self) -> &[&str] { &[] } // what must re-run after this pass
```

The pipeline validates ordering at startup (not derived dynamically — that's LLVM-level
complexity). If a pass's `requires` aren't satisfied by the declared ordering, it's a
startup error. If a pass's `invalidates` includes something that runs before it without
a subsequent re-run, it's a startup error. `ConstantFolding` would declare
`invalidates: []` but Mem2Reg would declare `invalidates: ["constant-folding"]`,
causing the framework to schedule a second ConstantFolding run automatically rather
than requiring it to be hardcoded.

This is the lightweight version of LLVM's analysis manager — no dynamic invalidation
tracking, just declaration-based validation and ordering derivation.

### Research Findings (2026-03-17)

**`Type::Var` status — orphaned infrastructure.** `Type::Var(TypeVarId)` is defined in
the IR but never instantiated anywhere in the codebase. No inference pass creates a
`Type::Var`. The backend previously mapped it alongside `Dynamic` → `any` (fixed: now
`unknown`). It is a skeleton waiting for the constraint solver to use it.

**`TypeConstraint` is dead scaffolding.** `ty.rs` defines `TypeConstraint { Equal,
Subtype, HasField, Callable }` — the right shape — but it is never emitted or consumed
by any pass. Can be repurposed directly for the constraint collection pass.

**`Struct(String)` is structurally opaque.** `Type::Struct(String)` carries only a name
string. The solver cannot resolve `C_HAS_FIELD(Struct("Foo"), "x", result)` because it
has no access to the fields of `Foo` from the type alone. Two consequences:
- `HasField` constraints are unresolvable without a separate struct-def lookup
- Row-variable inference for anonymous structs (e.g. GML `ds_map`, SugarCube state vars)
  is impossible without introducing `Type::Row(RowVarId)` (crescent's `TAG_ROWVAR`)
The correct long-term fix is `Struct(StructId)` with an arena of struct definitions.
Short-term workaround: the solver looks up fields via the module's class-def registry
keyed by name, accepting the string-identity assumption until `StructId` is introduced.

**`Type::Struct` variant removal — in progress.** `normalize_struct_types()` now covers
struct/class field types, static fields, abstract members, and globals (f7cab01). Backend
emit now uses `Type::Instance` directly for these cases. Remaining for full removal:
- Frontends (`reincarnate-frontend-flash`, `reincarnate-frontend-gamemaker`) use
  `Type::Struct(name)` as a deferred-resolution form — needs frontend API to accept
  TypeIds or have a pre-resolution pass that interns all names upfront before building
  function signatures.
- `resolve_js_function_types()` in backend converts `Instance → Struct` for the JsAST;
  removing this requires threading `module_types` through `ast_printer` (via thread_local
  or parameter), updating all `Type::Struct` match arms in printer/rewrites to also handle
  `Type::Instance`, and removing the Struct arms from `constraint_collect.rs`,
  `constraint_solve2.rs`, `call_site_flow.rs`, `ir/linear/tests.rs`, `red_cast_elim.rs`.
- Backend tests in `emit/tests.rs` and `ast_printer.rs` use `Type::Struct(...)` directly
  for cast/typecheck/return type tests — replace with `mb.intern_type_instance(name)`.

**`from_function` guard — root cause of heuristic immutability.** The existing
`constraint_solve.rs` only updates values whose current type is `Dynamic` or `Unknown`.
Values pre-bound by `type_infer.rs` (from write-site heuristics, call-site aggregation,
etc.) are immutable from the solver's perspective. This is the single architectural
reason the pipeline has four type passes instead of one: each subsequent pass exists
to fix what the previous heuristic got wrong while being unable to touch pre-bound
values. The new solver starts ALL values as `Type::Var` (unbound) and derives ground
types purely from constraints; `type_infer.rs`'s heuristics become constraint sources,
not pre-bound facts.

**`Dynamic` emission audit.** Audited all 25 `any` emission sites in the TypeScript
backend (`types.rs`, `ast_printer.rs`, `emit_flash_traits.rs`, `emit/class.rs`):
- **24 correct** — genuine source-language opacity (AS3 dynamic classes, XML coercion,
  GML ClassRef runtime indices, hash-array hybrids, etc.).
- **1 defect** — `emit_flash_traits.rs:211` defaults unknown property types to `"any"`;
  should be `"unknown"` (inference failure, not genuine opacity).

**`any` / `any[]` / `Record<string, any>` are three distinct TypeScript types** emitted
from three distinct IR patterns: `Dynamic`, `Array(Dynamic)`, and `Struct("Object")`.
They must not be conflated at the solver level. The heuristic that detects
"partially-inferred" types like `Array(Dynamic)` and demotes them is wrong: it equates
`any[]` with `any`. With proper HM, `_bodypart = Array(_bodypart)` triggers the occurs
check → `Dynamic`; no heuristic needed.

**IR-native constraint kinds (derived from Op variants, not 1:1 with Crescent).**
Crescent is prior art but our constraint kinds are derived from what the IR needs —
no more, no less. The minimal set:

- `Callable(func, args, result)` — `Op::Call`, `Op::MethodCall`, `Op::CallIndirect`,
  arithmetic ops (`Add`, `Sub`, `Mul`, `Div`, `Mod`, `Not`, `BoolAnd`, `BoolOr`,
  `Cmp`), and `GetIndex`. All resolved against declared signatures.
- `HasField(obj, field, result)` — `Op::GetField`, `Op::SetField`
- `Equal(a, b)` — phi merges / block args, `Op::Return` (links to return slot)
- `Bind(value, ty)` — `Op::Const` (literal type), `Op::Cast` (explicit override),
  `Op::MakeClosure` (sig is known at construction)

**No `C_COMPARE`, no `C_ARITH`, no `C_SUB` as distinct kinds.** Rationale:

- `Cmp` is `Callable(builtin_cmp, [a, b], result)` — signature `(T, T) -> Bool`.
  The "operands must match" constraint falls out of the polymorphic signature
  naturally; no special `Equal(a, b)` rule needed. Result always `Bool` falls out
  of the declared return type.
- `Not`/`BoolAnd`/`BoolOr` are `Callable` against `(Bool) -> Bool` /
  `(Bool, Bool) -> Bool` signatures. No special case.
- Arithmetic ops (`Add`, `Sub`, etc.) are `Callable` against declared builtin
  signatures (Phase 9). No `C_ARITH` — Crescent needs it for Lua metamethods;
  our IR has no metamethods.
- `C_SUB` (subtype / coercion): deferred. `Cast` is handled by `Bind` (direct type
  override). If subtype constraints are needed (e.g. for inheritance), add then.

**`Store`/`Load`/`Alloc` generate no constraints** — Mem2Reg eliminates them before
the solver runs. Control flow ops (`Br`, `CondBr`) generate no constraints.

**Design decision (2026-03-20, refined 2026-03-27): `C_ARITH` is not a distinct
constraint kind. Arithmetic ops are calls to builtins with declared signatures;
`Callable` handles them uniformly.**

Crescent needs `C_ARITH` because Lua has metamethods — the constraint means "find
`__add` on the operand type and unify result with the metamethod return." Our IR has no
metamethods: `Op::Add` is defined as numeric addition by contract, so `C_ARITH`
reduces to `Callable(builtin_add, [a, b], result)` against a declared signature.

The correct model:
- Arithmetic ops (`Op::Add`, `Op::Sub`, etc.) are calls to builtin functions registered
  in the module with declared signatures: `add: (Int(64), Int(64)) -> Int(64)`, etc.
- `FunctionBuilder` keeps convenience methods (`fb.add(a, b)`) that emit `Op::Call` to
  the appropriate builtin `FuncId` — frontend API unchanged.
- The constraint solver treats arithmetic identically to any other `Call` — reads the
  declared signature, emits `Callable`, propagates bidirectionally.
- No op-specific logic in the solver. No hardcoded numeric grounding. No false positives.

Why this is correct:
1. **Law 1 (Pipeline Stage Isolation):** type semantics of operations live in the IR
   (declared signatures), not implicitly in the solver as pattern-matched op knowledge.
2. **Law 2 (Engine Specificity at Boundaries):** the solver stays engine-agnostic; it
   handles `Callable` uniformly for all functions. Arithmetic semantics are declared by
   whoever builds the builtins, not hardcoded in core.
3. **"Fix the real problem":** the root cause of missing numeric grounding is that
   `Callable` constraints don't propagate types well — for ANY function, not just
   arithmetic. Fixing `Callable` fixes arithmetic as a consequence; patching arithmetic
   specifically would be a monkeypatch.
4. **Parameter constraining is general, not builtin-specific.** Any function with a
   declared signature constrains its callers. Builtins are just the first consumers.

**Corollary:** once `Callable` is properly bidirectional, `CallSiteTypeFlow` and
`CallSiteTypeWiden` are redundant and should be retired. The unified solver handles
interprocedural inference in one pass.

**Numeric grounding — prior limitation resolved by the above design.** The 2026-03-17
note about false positives from emitting `Float(64)` equality constraints for arithmetic
operands is no longer relevant: with builtins as declared functions, grounding comes
from the builtin's parameter types via `Callable` — which naturally back-propagates
only when the constraint is consistent with other constraints on the same variable.

**`Op::Add` and arithmetic ops are not core IR instructions — they are builtin calls.**
GML bytecode carries explicit type tags (`ADD.f64`, `ADD.i64`, `ADD.str`). The frontend
should emit different builtins per tag:
- `ADD.f64` → `Op::Call("builtin.add_f64", [a, b])` — sig `(Float, Float) → Float`
- `ADD.str` → `Op::Call("builtin.str_concat", [a, b])` — sig `(String, String) → String`
- `ADD.i64` → `Op::Call("builtin.add_i64", [a, b])`

The solver handles them via `Callable` on declared signatures — no special arithmetic
logic. A `ty` field on `Op::Add` would be a transitional detour that Phase 9 makes
obsolete immediately. Skip it; go straight to Phase 9.

**Interim (active, 2026-03-20):** `Equal(result_var, operand_var)` for Sub/Mul/Div/Rem/
Neg/bit ops in ConstraintCollect. Skips Add (ADD.str vs ADD.f64 ambiguity without type
tag). Enables the interprocedural return type chain until Phase 9 lands.

### What to Preserve

- The `TransformPipeline` / `Transform` trait structure is fine. Structural passes
  (`Mem2Reg`, `CoroutineLowering`, `CfgSimplify`, `DCE`) are not type inference and
  don't need to change.
- `Type::Var(TypeVarId)` and `TypeConstraint` exist in the IR — the solver activates
  them. No IR change needed to prototype.
- The `SystemCallTypeRule` plugin system is the right idea; it feeds into the constraint
  collection pass as constraint emitters, replacing `build_global_types`.

### Scope and Approach

Constraint collection is engine-agnostic — it maps to IR ops in `reincarnate-core`.
Crescent's `lib/type/static/` (constrain.lua, solve.lua, unify.lua) is the prior art
and close to gold standard for a practical system. Crescent adds row polymorphism,
union/intersection types, and subtyping to basic HM — all applicable here.

**Implementation sequence:**

1. **New `ConstraintSolve` pass skeleton** — add `transforms/constraint_solve2.rs`
   alongside the existing one. Introduce a `TypeVarArena` that allocates fresh
   `Type::Var(id)` nodes with levels (for generalization). Add `UnionFind` over
   `TypeVarId` with occurs check in `bind_var`. No constraint generation yet — just
   the unifier + arena infrastructure.

2. **`ConstraintCollect` pass** — single IR walk (per function, then inter-proc) that
   emits `TypeConstraint` values for every `Op`:
   - `Op::Add/Sub/Mul/Div/Mod` → `C_CALLABLE(builtin_fn, [a, b], result)` (arithmetic ops
     are calls to builtins with declared signatures; no special C_ARITH constraint kind)
   - `Op::Cmp` → `Equal(result, Bool)` ✅ (already implemented)
   - `Op::Not/BoolAnd/BoolOr` → `Equal(result, Bool)` ✅ (already implemented)
   - `Op::GetField { object, field }` → `HasField(object, field, result)` ✅
   - `Op::SetField { object, field, value }` → `HasField(object, field, value)` ✅
   - `Op::GetIndex/SetIndex` → `C_INDEX(collection, index, result)`
   - `Op::Call/MethodCall` → `Equal(result, return_ty)` ✅ (concrete returns only)
   - `Op::CallIndirect` → `Callable(callee, args, result)` ✅
   - `Op::Cast` → `Equal(result, target_ty)` ✅
   - `Op::TypeCheck` → `Equal(result, Bool)` ✅
   - `Op::StructInit` → `Equal(result, Struct(name))` ✅
   - block args → `Equal(arg, param)` ✅
   - `Terminator::Return` → `Equal(value, return_var)` ✅
   - Engine-specific rules (GlobalStore write sites, ClassRef) via `SystemCallTypeRule` ✅
   **Missing constraints (2026-03-20 audit — 54,354 RC1001 values after sig.return_ty fix):**
   - `Op::Cmp` → `Equal(result, Bool)` — IN PROGRESS (TypeInference deterministic ops)
   - `Op::Not/BoolAnd/BoolOr` → `Equal(result, Bool)` — IN PROGRESS
   - `Op::Select` → `Equal(result, on_true)`, `Equal(result, on_false)` — IN PROGRESS
   - `Op::MakeClosure` → `Equal(result, Function(sig))` from callee sig — IN PROGRESS
   - `Op::Call result → callee return_var` (unconditional, not gated on sig.return_ty) — IN PROGRESS (Step 2b interprocedural bridge)
   - `Op::Const` → `Equal(result, literal_type)` — redundant (Phase 1 already binds)
   - `Op::GlobalRef` → `Equal(result, global_type_var)` — dormant (OBJT/FUNC refs not in globals map by design)
   - `Op::Add/Sub/Mul/Div/Rem/Neg` → blocked on Phase 9 (builtins-as-calls); + is string concat in GML
   - `Op::BitAnd/BitOr/BitXor/BitNot/Shl/Shr` → `Equal(result, operand)` — integer type (safe to add after Phase 9)
   - `Op::ArrayInit` → `Equal(result, Array(element_var))` with element unification
   - `Op::GetIndex` → index-access constraint (collection element → result)
   - `Op::Load` → blocked on per-Alloc element type tracking
   - `Op::TupleInit`, `Yield`, `CoroutineCreate/Resume`, `Spread` — low priority

3. **Wire collection into `ConstraintSolve2`** — solver processes `TypeConstraint` list;
   starts ALL non-ground values as `Type::Var`; resolves vars to concrete types; falls
   back to `Type::Unknown` (not `Dynamic`) for unresolved vars. Occurs check fires for
   recursive types like `_bodypart = Array(_bodypart)` → `Dynamic`.

4. **Coexistence testing** — run both old and new passes; compare type maps on test
   games; flag regressions. Old passes remain in pipeline until new solver meets or
   exceeds their results on all games.

5. **Struct field resolution** — the solver looks up struct fields via module class-def
   registry for `C_HAS_FIELD` on `Struct(name)`. Long-term: migrate to `Struct(StructId)`
   with proper arena, enabling the solver to work without string lookup.

6. **Row variables** — introduce `Type::Row(RowVarId)` for anonymous struct inference
   (GML `ds_map`, SugarCube state vars). Modeled after crescent's `TAG_ROWVAR`. Lets the
   solver infer struct shapes from field-access patterns alone.

7. **Retire heuristic passes** — once the new solver covers all test games, remove
   `TypeInference`, `CallSiteTypeFlow`, `CallSiteTypeWiden`, `CallSiteArityWiden` and
   replace with the new pipeline: `ConstraintCollect` → `ConstraintSolve2`.

**Prerequisites done (2026-03-17):**
- `Type::Var → "unknown"` in TypeScript backend (commit `a7ba296`)
- `TypeConstraint` and `TypeVarId` exist in `ir/ty.rs` (repurposable scaffolding)

**Phase 1 done (2026-03-18, commit `302c3d2`):**
- `TypeVarArena`, `UnionFind`, occurs check, `bind_var`, `resolve` in `constraint_solve2.rs`
- `ConstraintCollect` pass in `constraint_collect.rs` — shared arena architecture; emits
  `Equal`, `HasField`, `Callable` constraints per function
- `ConstraintSolve2::apply` — shared arena across all functions; pre-allocates TypeVarIds
  for declared globals; collects+solves constraints jointly; writes back improved types
- `majority-wins` heuristic removed from `build_global_types` — `union_type` already drops
  Dynamic members, so opaque write sites add no signal and should not suppress inference

**Phase 1.5 done (2026-03-18, commit `6e44573`):**
- `GlobalStore`/`ResolveGlobalType` Equal constraints now emitted from `collect_function`
  (only `ResolveGlobalType`, not `ResolveGlobalTypeStructOnly` — the latter excludes
  Engine.resolve to avoid TS2571 regressions from JS built-in global names)
- `TypeVarArena::force_rebind` added — allows poisoning a TypeVar already bound to a
  concrete type when a conflicting concrete type arrives later (fixes first-write-wins bug)
- `unify` saves original Var IDs before `resolve`; in concrete-mismatch arm, poisons those
  vars to Dynamic so step 6 writeback sees Dynamic (not stale first-write type)
- DoL: 2570 → 2567 TS (−3 conflicting globals now correctly Dynamic)

**Inference failure diagnostics (2026-03-20):**
- `--dump-inference-failures` on `emit` subcommand — category breakdown + top functions
- RC1001–RC1005 diagnostic codes: NoConstraints, Conflict, UnresolvedDeferred, NoCallers,
  InheritedUnknown
- Dead Estate baseline (96,404 total Unknown values):
  - RC1001 No constraints:      48,954 (51%) — solver is starving, not failing
  - RC1005 Inherited Unknown:   36,701 (38%) — transitive dependents of unconstrained values
  - RC1003 Unresolved deferred:  5,835 (6%) — HasField/Callable on Unknown objects
  - RC1004 No callers:           4,488 (5%) — runtime-only closures
  - RC1002 Conflicting types:      426 (<1%) — actual unification conflicts
- Key insight: fixing constraint coverage (Tier 1+2 in missing constraints list above)
  should resolve ~30,000+ RC1001 failures and cascade into reducing RC1005

**Pipeline ordering gap (BLOCKER for Phase 2):**
- ConstraintSolve2 runs AFTER TypeInference. Phase 1 write-site types (from
  `build_global_types`) feed TypeInference's per-function inference loop (ResolveGlobalType
  return types). Removing Phase 1 would leave per-function inference blind to global types,
  and Phases 2/3 (array/struct use-site) in `build_global_types` would degrade.
- ConstraintSolve2's global writeback (step 6) correctly handles Dynamic|Unknown globals
  now, but cannot override Phase 1's concrete types (guard: `Dynamic | Unknown` only).
- Resolution requires either:
  (a) merging global inference into TypeInference's multi-pass loop using HM infrastructure, or
  (b) moving TypeInference entirely after ConstraintSolve2 (removes majority of heuristic passes)
  Track as blocker for TODO item 7 (Retire heuristic passes).

---

## Full Architecture Audit — COMPLETED 2026-03-12

Full report: [`docs/architecture-audit-2026-03-12.md`](docs/architecture-audit-2026-03-12.md)

**Verdict:** Architecture is fundamentally sound. Clean crate boundaries, proper stage
isolation, no circular dependencies. Ready for a second backend without major restructuring.

**Key findings:**
- ✅ Pipeline stages: correct, well-ordered. Pass ordering has implicit contracts (passes 2-4)
  that should be documented in code.
- ✅ Crate boundaries: clean. No cross-frontend/backend deps. All public API usage only.
- ⚠️ IR completeness: aggregate constants missing (tracked below). Data files bypass IR.
- ⚠️ Type system: no generics, no flow-sensitive narrowing, Dynamic conflation. Sufficient
  for current engines but will need expansion.
- ⚠️ Law compliance: Law 1 (aggregate constant bypass — `Constant` enum lacks Array/Map).
  Law 2 all named violations fixed. Laws 3-5 clean.
- ⚠️ Module struct: 8 engine-specific fields (kitchen sink). Fix via aggregate constants.
- ⚠️ Abstraction gaps: `abstract_members` tuple, `StructDef.fields` tuple (tracked in
  IR Class Representation section below).

## IR-Based Modding Framework (Investigation)

**Status: Investigated. Mod is logic-heavy — full IR mutation API required.**

`~/git/bounty/` is the original game hand-decompiled to JS, then **further modified** as a mod. `git diff 98423b5 HEAD` (first commit → current) shows 2262 insertions / 596 deletions across 115 files.

**Delta breakdown:**

New classes (entirely new objects not in original):
- `classes/overview_button.js` (318 lines) — new overview/character screen
- `classes/overview_main.js`, `classes/overview_finished.js` — overview system
- `classes/appearance_reader.js` (163 lines) — appearance screen
- `classes/debug_main.js`, `classes/debug_button.js` — debug tools
- `classes/name_block.js`, `classes/name_input.js` — custom name input
- New rooms: `rooms/appearance.js`, `rooms/debug.js`

New script functions:
- `scripts/main6.js` (495 lines, new) — gangbang encounter system

Modified existing classes:
- `classes/stats.js` (+144 lines) — new stat fields added to `create()`
- `classes/encounter.js` (+85 lines) — new encounter types
- `scripts/main.js` (+102 changes), `main2.js`, `main3.js`, `main5.js` — mixed logic/text

Data changes:
- `data/0.png` — texture atlas updated (new sprites)
- `data/sprites.js`, `data/textures.js` — new sprite/texture entries
- `data/roomdatas.js` — new room placements

**Conclusion:** Logic-heavy mod. But the correct mod surface under Reincarnate's design is **the emitted TypeScript** — lift once, edit the output, forward-port via cherry-pick if upstream updates. No IR mutation API needed. See [ADR 003](docs/adr/003-project-identity-and-mod-surface.md). **Closed.**

## Planned Engines (not yet started)

Full roadmaps in `docs/targets/<engine>.md`. Summary of where each stands:

| Engine | Blocker / next step |
|--------|---------------------|
| [GameMaker 8.x](docs/targets/gamemaker8.md) | New container parser for `.wad`/GM8 format + opcode adjustments; reuses GMS1 translator — test game: Hotline Miami (`~/reincarnate/gamemaker/hotlinemiami/`) |
| [GameMaker 5/6](docs/targets/gamemaker5.md) | Unpack data from PE exe, then parse GM5/6 format; older/simpler opcode set — test game: Seiklus (`~/reincarnate/gamemaker/seiklus/`) |
| GameMaker YYC | YYC-compiled games have no CODE chunk — logic is in native binary. Requires native decompiler pipeline (out of scope for now). Affects: Katana Zero, Picayune Dreams |
| [Director/Shockwave](docs/targets/director.md) | Format parsing (RIFX/Lingo bytecode) — ProjectorRays and ScummVM are references |
| [Ren'Py](docs/targets/renpy.md) | `.rpa` extractor → `.rpyc` decompile (unrpyc) → Ren'Py AST → IR |
| [RPG Maker VX Ace](docs/targets/rpgmaker.md) | `Scripts.rvdata2` extractor (Ruby Marshal) → Ruby AST → IR |
| [RPG Maker MV/MZ](docs/targets/rpgmaker.md) | JSON event command compiler → IR |
| [Inform (Z-machine/Glulx)](docs/targets/inform.md) | Story file parser → bytecode decoder → IR; well-documented specs |
| [Ink by Inkle](docs/targets/ink.md) | `.json` container reader → IR (knots/stitches → functions, choices → Yield) |
| [Visual Basic 6](docs/targets/vb6.md) | PE/VB6 header parser → P-Code decoder → IR |
| [Java Applets](docs/targets/java-applets.md) | JAR/class file parser → JVM bytecode decoder → IR; JVM spec is thorough |
| [Silverlight](docs/targets/silverlight.md) | XAP extractor → PE/CLI parser → IL decoder → IR + XAML parser |
| [HyperCard](docs/targets/hypercard.md) | Stack binary parser → HyperTalk text parser → IR (scripts are source, not bytecode) |
| [WolfRPG](docs/targets/wolfrpg.md) | `.wolf` decryption → event command compiler; `wolfrpg-map-parser` Rust crate exists |
| [SRPG Studio](docs/targets/srpg-studio.md) | `data.dts` decryption → NW.js API shim (engine is already JS) |
| [RAGS](docs/targets/rags.md) | NRBF/SDF decryption → game data extractor; `rags2html` is the reference impl |
| [QSP](docs/targets/qsp.md) | `.qsp` decoder → QSP-lang parser → IR; open-source `libqsp` is reference |
| [PuzzleScript](docs/targets/puzzlescript.md) | Source parser → rule compiler → IR (rule semantics are formally specified) |

---

## Platform Interface Redesign

**Design phase complete** (2026-03-04 through 2026-03-04 follow-up). Canonical interface for all concerns is now in `architecture.md`: Timing, Input, Persistence, Images, Graphics 2D, Graphics 3D, Audio, Window, Clipboard, Network. Three audit rounds (consistency, gaps, devil's advocate × 2) applied.

**Implementation incomplete** (marked complete 2026-03-04, audit 2026-03-15 found extensive bypasses).

GML runtime (`runtime.ts`, `draw.ts`, `storage.ts`) extensively bypasses platform abstractions:
- [ ] **localStorage** (28 sites in 2026-03-15 audit) — `ini_open/close`, `file_text_*`, `buffer_load/save`, `ds_map_secure_save`, steam stats — should use `platform/persistence.ts` OPFS-backed `PersistenceState`
- [ ] **document.createElement** (3 remaining) — `_captureCanvas` (WebGL readback), `<video>` (draw_enable_drawtoblendstate), temp canvas (sprite_save) — part of the Canvas2D refactor
- [ ] **Canvas2D** (57 sites) — `this._gfx.ctx` raw `CanvasRenderingContext2D` access instead of platform `GraphicsState` handle-based API
- [ ] **fetch/Image** — `loadImage()` module-level function uses bare `new Image()` instead of `platform/images.ts` `loadImageUrl`
- [x] **performance.now() / Date.now()** — routed through `platform/timing.ts` `currentTimeMs()` / `currentWallTimeMs()` (session 29)
- [x] **navigator.getGamepads()** — routed through new `platform/input.ts` gamepad functions (session 29)
- [x] **navigator.language / onLine / clipboard** — routed through new `platform/system.ts` (session 29)
- [x] **window.innerWidth/innerHeight, window.open, window.close, document fullscreen, download** — routed through new `platform/window.ts` (session 29)

Flash runtime: platform threading via `FlashShims` was completed correctly.

---

## Save System — Generalize `HistoryStrategy` from List to Tree

Surfaced 2026-05-23 while mining `runtime/twine/ts/platform/{save,history}.ts` as prior art for a chub-stage-factory state-persistence redesign. Reincarnate's `HistoryStrategy` (`snapshotHistory()` / `diffHistory()`) is a linear push/pop stack — `Moment[]` with `pushMoment`/`popMoment`/`peekMoment`. That shape is a tree pruned to one path, not a distinct primitive. The linear restriction is hardcoded; making it a tree would not remove any existing functionality and would unlock branch-aware histories (alternate timelines, swipe-style undo, save-point exploration) at no abstraction cost.

**Proposed shape:**

```ts
type Moment<M> = { id, parentId?, payload: M | Diff<M> }
type History<M> = { moments: Map<Id, Moment<M>>, cursor: Id }
  commit(m)             // new child of cursor; advance
  navigate(id)          // jump cursor anywhere (tree nav)
  state(): M            // peek @ cursor (snapshot) or fold-from-root (diff)
  children/parent/siblings/root
```

The current `snapshotHistory` / `diffHistory` distinction becomes orthogonal payload encoding on `Moment`. The current linear behavior becomes a combinator: `forbidBranching(history)` rejects `commit` at a non-leaf (this is exactly the current `pushMoment` semantics — no behavior change for engines that don't opt in). Other combinators fall out: `bounded(h, n)` (evict old leaves — replaces the implicit collapse in `diffHistory`), `persisted(h, backend)` (survive across sessions).

**Why it matters for reincarnate specifically:** the lifted Twine engines (SugarCube, Harlowe) both have user-facing back/forward operations that are naturally a tree the moment a player goes back and makes a different choice. Today the engine silently discards the forward branch on the new commit; with tree history, it's preserved and navigable — closer to the "undo + redo + branch explore" model some modern IF engines (e.g. SugarCube's history dialog) gesture at but don't fully implement. This is also the right substrate if `history: "diff" | "snapshot"` in `PersistenceConfig` ever grows a `"tree"` third option.

**Out of scope here:** the multi-shard, chub-three-layer-backend, React-slot-UI adaptations live in chub-stage-factory; reincarnate's runtime stays linear by default (`forbidBranching` is the default wrapper). This TODO is just "don't bake the linear assumption into the data structure."

**Touch points:** `runtime/twine/ts/platform/history.ts`, `runtime/twine/ts/platform/save.ts` (commit/load wiring), `runtime/twine/ts/{sugarcube,harlowe}/state.ts` (engine adapters shouldn't need to change if `forbidBranching` is the default).

**Config drift to fix while we're here:** `PersistenceConfig.slot_count` in `crates/reincarnate-core/src/project/manifest.rs:77-122` is ignored by the TS `SaveManager` (hardcoded `static readonly SLOT_COUNT = 8` in `platform/save.ts`). Either wire it through the scaffold substitution, or remove the manifest field.

---

## Engine-Specific Logic in `reincarnate-core` (found 2026-03-09 audit)

CLAUDE.md rule: "Frontend/backend specific logic never belongs in `reincarnate-core`."
All of the following violate it and need to move to the respective frontend crates.

- [x] **`type_infer.rs` lines 473–514: Flash and GML dispatch in the shared type inference pass.** (2026-03-11)
  Replaced with `Module.system_call_type_rules` — frontends register `SystemCallTypeRule` entries
  (ResolveClassName, ConstructFromFirstArgType, ResolveGlobalType). Type inference reads the
  map instead of hardcoding engine names. Commit: `4e88003`.

- [x] **`linear.rs` lines 765, 1638–1669: Flash-specific rewrites in the shared linearizer.** (2026-03-11)
  `is_scope_lookup_op()` now parameterized via `LoweringConfig::scope_lookup_systems` (Flash backend
  sets `["Flash.Scope"]`). `Flash.Object deleteProperty/hasProperty` dead code removed (was
  unreachable — Dictionary maps to `Type::Map`, not `Type::Struct`). Law 2 satisfied.

- [x] **`linear.rs` line ~1454–1465: GML `ClassRef as any` widening in the shared linearizer.** (2026-03-11)
  Now gated on `LoweringConfig::wrap_class_refs_as_any` (GML backend sets `true`). Law 2 satisfied.

- [x] **`ast_passes.rs` lines 1804–2140: Flash `ForOfRewrite` / `HasNext2` pattern in shared AST passes.** (2026-03-15)
  `foreach_rewrite: bool` → `foreach_iterator_system: Option<String>`. The system name (`"Flash.Iterator"`)
  now lives in the Flash backend config; `control_flow.rs` matches against the config value, not a
  hardcoded string. Law 2 satisfied.

- [x] **`control_flow.rs` line 164: Harlowe-specific `Harlowe.H` → `h.method()` rewrite in core.** (2026-03-15)
  Added `LoweringConfig::output_node_system: Option<(String, String)>`. Twine backend sets
  `("Harlowe.H", "h")`; `lower_output_nodes` uses the config value instead of hardcoded strings.
  Law 2 satisfied.

- [x] **`CastKind::AsType` in core IR is AS3-specific.** (2026-03-11)
  Already renamed to `CastKind::NullableCoerce` with language-agnostic doc: "Nullable cast —
  returns null if the value is not an instance of the target type (e.g. AS3 `as`, Kotlin `as?`, C# `as`)".

- [x] **`CmpKind::LooseEq` / `LooseNe` in core IR are JS-specific.** (2026-03-11)
  Renamed to `CoercingEq` / `CoercingNe` with language-neutral docs. Coercing equality
  is a valid cross-language semantic concept (JS, PHP, Perl) — not engine-specific.

- [x] **`datawin` crate missing `reincarnate-` prefix.** (2026-03-11)
  Already named `reincarnate-datawin` in Cargo.toml — the `reincarnate-` prefix is present.

- [ ] **IR lacks aggregate constants — root cause of all "data file" pipeline bypasses.**

  The IR `Constant` enum only has scalar values: `Null`, `Bool`, `Int`, `Float`, `String`.
  There is no `Constant::Array` or `Constant::Map`. Because frontends can't express structured
  compile-time data as IR, they fall back to writing raw TypeScript source blobs into
  `AssetCatalog` and bypassing the entire backend.

  **The correct design:**

  1. Add `Constant::Array(Vec<Constant>)` and `Constant::Map(Vec<(String, Constant)>)` to the
     core IR. These are plain typed data — no engine knowledge required.

  2. Frontends emit compile-time tables as IR constants. GML frontend: instead of calling
     `generate_objects` to write `data/objects.ts`, it emits an IR constant:
     `module.add_const("Classes", Type::ConstMap(String, u32), Constant::Map(pairs))`.
     The GML-specific knowledge (OBJT indices, sprite metadata, room tables) stays in the
     frontend. The IR just sees a typed constant.

  3. The TypeScript backend, when it encounters a `ConstMap<String, u32>` module constant,
     emits `export const Name = { "key": value, ... } as const`. No GML knowledge needed.
     A Rust backend emits `static NAME: &[(&str, u32)] = &[...]`. Neither backend is
     engine-specific.

  4. `AssetCatalog` returns to binary-only (textures, audio). TypeScript source files are
     never stored as blobs — they're always emitted by the backend from IR.

  **What goes away:** `module.object_names: Vec<String>`, `module.sprite_names: Vec<String>`,
  `generate_data_files` in the GML frontend, and all the intermediate "metadata" fields that
  exist only because the IR can't hold the data directly. The `AssetCatalog` TypeScript blob
  hack (`data/objects.ts`, `data/asset_ids.d.ts`, `data/textures.ts`, etc.) is deleted.

  **Current workaround (2026-03-10):** `generate_objects` uses quoted string keys
  (`"3platgen": 515`) to handle GML names that aren't valid TypeScript identifiers. This is
  a local fix; the real fix requires this whole design change.

- [ ] **`Module` struct is a per-engine kitchen sink — Fundamental Law 1 violated.**
  `module.rs` already has 6 GML-specific fields (`room_creation_code`, `initial_room_name`,
  `sprite_names`, `object_names`) and 4 Twine-specific fields (`passage_names`, `passage_tags`,
  `passage_sources`, `passage_storylets`). Each new engine adds more. The IR-as-sole-channel
  law is dead in practice once fields that only one engine reads start accumulating.
  **Fix:** migrate engine-specific metadata into the aggregate-constants design above, or add
  a `Module::metadata: HashMap<String, Box<dyn Any>>` typed-extension slot. Track: every
  new `Module` field added for a new engine is a regression.

- [x] **Flash `NewActivation` emitted as `Record<string,any>` heap object — wrong IR.**
  *(2026-03-11)* For functions without closures: frontend emits `Alloc` per activation slot,
  intercepts `GetSlot`/`SetSlot` → `Load`/`Store`. Mem2Reg promotes to SSA locals.
  `eliminate_dead_activations` removes the now-unread activation object. Zero `newActivation`
  calls in emitted output; all activation slots are direct `let` declarations.
  Functions WITH closures retain `SystemCall("Flash.Scope","newActivation")` + backend
  rewrite pipeline — closures need the scope-chain object until `MakeClosure.captures`
  is implemented.

- [x] **`emit.rs` Flash contamination — remaining `EngineKind::Flash` branches.** (2026-03-11)
  All 9/9 items addressed: traits→`emit_flash_traits.rs`, QN_KEY/registerClass/ctor shim/
  forwarding_setters→`emit_flash_traits`, bang→`ClassDef.zero_initialized`, index sigs→
  `ClassDef.needs_index_signature`, warn_unmapped→`rewrites::flash`, cinit→`MethodKind::StaticInit`.
  Only 3-line call site + guard remains in emit.rs.

- [ ] **`coalesced_decl_types` widening to `Dynamic` is a suppression, not a fix (Law 4).**
  When two branch arms produce different types for the same out-of-SSA variable, widening to
  `Dynamic` silences TS2322/TS2739 but commits to "this is truly dynamically typed" without
  distinguishing it from "our inference inferred wrong types" or "naming collision in the
  coalescer." A type conflict is a diagnostic signal. The correct fix: produce `Type::Union`
  of the conflicting types and let the backend emit a TypeScript union annotation
  (`TimeModel | DefaultDict`). If the union is unsound, the TS error that remains is
  telling us something real about inference quality. (2026-03-11 fix: commit 9f80254.
  Should be revisited once union types are added to the IR.)

---

## Flash Output Quality (2026-03-11 audit)

Audit of generated `.ts` files for human maintainability (see `~/reincarnate/flash/cc/out/`).
Game-logic files (CoC.ts, ConsumableLib.ts) are excellent — <1% artifact names, readable methods.
Library files (List.ts, StyleManager.ts) are poor — 22–40% artifact names, activation record objects.

- [x] **`GetIndex`/`SetIndex` with `Constant::String` key → emit dot notation.** (2026-03-11)
  `is_js_ident()` in `linear.rs`; GetIndex and SetIndex both emit `obj.field` when key is a
  valid JS identifier.

- [x] **IR fields to replace `EngineKind::Flash` branches in class emission.** (2026-03-11)
  `ClassDef.zero_initialized`, `ClassDef.needs_index_signature`, `MethodKind::StaticInit` all added.

- [x] **Recover switch discriminant from chained ternary chains.** (2026-03-11)
  `try_recover_switch_discriminant` + `extract_ternary_chain` in `ast_passes.rs`. Recovers
  both literal constants (e.g. `"["`, `"]"`, `"|"` in Parser.ts) and non-literal expressions
  (e.g. `Keyboard.UP`, `CockTypesEnum.ANEMONE`) as case labels. `JsStmt::Switch` cases are
  `Vec<(JsExpr, Vec<JsStmt>)>` — case labels are arbitrary expressions, not just `Constant`.

- [x] **Format `registerClassTraits(...)` as multi-line.** (2026-03-11)
  Already implemented in `emit_flash_traits.rs` — one trait object per line with 2-space indent.

- [x] **Separate `registerClass`/`registerClassTraits` into a companion file.** (2026-03-11)
  `emit_class` writes registration calls to `traits_out` buffer; file-mode call site generates
  `ClassName_traits.ts` companions with smart imports (registry lookup for in-module interfaces,
  `external_imports` for runtime interfaces, disambiguated TS names). 429 companions emitted;
  barrel exports include them. Scaffold tsconfig change still needed (TODO below).

- [x] **Dead code in Parser.ts `recParser` — structurizer failure.** (2026-03-11)
  Root cause: two bugs in `structurize.rs` when processing general loops (both BrIf targets
  in the loop body).
  1. `find_merge` computed the loop exit block (outside loop body) as the merge point for
     in-loop branches, then `structurize_region(merge_block)` consumed it — adding it to
     `emitted` so `structurize_loop`'s post-loop continuation found it already emitted.
     Fix: reject merge points outside the loop body when inside loop body processing.
  2. `loop_exit_shape` consumed multi-predecessor exit blocks (convergence points for
     multiple break paths). Fix: require exactly 1 predecessor for the first block in
     the exit chain.
  Follow-up: 3 TS2304 (`v54` scoping) fixed in same session — extended
  `compute_cross_scope_defs` to handle multi-use values, not just SE inlines.
  Also still open: `Parser.ts:603–610` dead assignments in switch arms.

## Flash Output Quality Audit (HIGH PRIORITY)

Systematic audit of all remaining TS errors and emitted code quality. The CC project
is the primary Flash test game. Current baseline: **15 TS errors** (was 30; fixed 15 via
class coercion rewrite, type inference, XML→any mapping, universal index signatures).

- [x] **Categorize all 30 remaining TS errors** — DONE 2026-03-12. Full per-error triage below.

### Error Triage (30 errors, by category)

**Game-author bugs (12 errors — leave as-is, these are correct diagnostics):**
- TS2367 (6): Appearance.ts:1065 (`number === false`), JeanClaudeScenes.ts:283
  (`!= "smooth" || != "latex"` — always true, should be `&&`), MinotaurKing.ts:249
  (`!_orgasms !== 0`), Utils.ts:106 (`!typeof o[field] === "number"` — precedence bug),
  Katherine.ts:3561 (`flags[FLAG > 10]` — comparison as index, should be `flags[FLAG] > 10`),
  Appearance.ts:1065 (`skinType === false` — bool/number coercion)
- TS2367 (2): CoC.ts:13871-3 (`0.0 !== 1/2/3` — dead code from constant folding;
  investigate if this is a compiler bug vs dead branch)
- TS1345 (1): Inventory.ts:337 (`!combatRoundOver()` — void tested for truthiness)
- TS2339 (1): Parser.ts:714 (`arr.len` — typo, should be `.length`)
- TS7053 (2): StatsView.ts:129 (`player[statName]` where statName is from a malformed array)

**Emitter bugs (fixed):**
- [x] TS2348 (2): CockTypesEnum.ts:55,59 — `this(this, ...)` was class coercion, not construction.
  Fixed: `this(this, arg)` in static method → `asType(arg, ClassName)` via NullableCoerce cast.
- [x] TS2417 (1): CockTypesEnum.ts:8 — WONTFIX. AS3/TS semantic gap: AS3 static methods don't
  inherit, so subclass can have incompatible signatures. TS is correct to flag this.
- [x] TS2538/TS2536 (5 of 7 fixed): UIComponent Dictionary bracket-notation fixed by teaching
  type inference to resolve field types from qualified names on Dynamic base. Remaining 2:
  BindingPane.ts (XML as index on Keyboard, not Dictionary) and Katherine.ts (boolean as index).
- [x] TS2769 (2): StatsView.ts:84 — `.replace()` with numeric args. Fixed: Flash rewriter wraps
  second arg of `.replace()` with `String()`.
- [x] TS2345 (3): `.apply()` args typed as `any[]` not matching exact tuples. Fixed: cast
  `.apply()` second arg as `any` in rewrite_this_to_prototype + generic rewrite_expr rule.
  Also widened `int()` runtime function from `(x: number)` to `(x: any)` for AS3 semantics.

**Remaining Flash errors (15 = 2 fixable + 13 game-author):**
- [ ] TS2322 (1): CoC.ts:11059 — `XML` assigned to `string` field. AS3 implicit XML→string
  coercion. `construct_string_coerce` only fires when result type is String; here it's
  Struct("XML") because type inference can't see the assignment target type. Needs either
  context-aware construct coercion or a post-emit rewrite.
- [x] TS2538 (1): BindingPane.ts — FIXED 2026-03-12: XML/XMLList mapped to `any` in ts_type.
- [x] TS7053 (2): StatsView.ts — FIXED 2026-03-12: needs_index_signature=true for ALL Flash
  classes (AS3 allows bracket access on sealed classes too).
- [x] TS7053 (1): BindingPane.ts — FIXED 2026-03-12: static index signature on Keyboard runtime class.
- [ ] TS2322 (1): Appearance.ts:987 — `string` assigned to `never`. Game-author dead code:
  outer `description === ""` makes inner `description !== ""` always false, TS narrows to `never`.

**Game-author bugs (per Law 3 — preserve):**
- TS2367 (8): constant-folded `0.0` comparisons (CoC.ts:13755-13758 — `damage = rand(4)` missing
  in Misdirection block, game-author copy-paste error), boolean/string overlaps, `smooth`/`latex`.
- TS2345 (1): Parser.ts:596 — `new Error(msg, textCtnt)` second arg is string, not ErrorOptions.
- TS1345 (1): Inventory.ts:327 — void tested for truthiness.
- TS2339 (1): Parser.ts:682 — `.len` on Array (should be `.length`, game-author typo).
- TS2538 (1): Katherine.ts:3550 — boolean as index type.

**Remaining audit items:**
- [x] **Audit emitted code readability** — DONE 2026-03-12. Key findings:
  - [x] Boolean return chains: `if (x) return true; else return false;` → `return x;` — DONE 2026-03-12 (`simplify_boolean_returns` pass, 208 simplified)
  - [x] Double `String()` wrapping: `String(String(x))` → `String(x)` — DONE 2026-03-12 (`unwrap_coerce` in printer, then `string_method_return_type` in type_infer.rs: 258→231 String() calls, −27 redundant; 1 double-String remaining)
  - [x] Redundant else after return — DONE 2026-03-12 (`hoist_else_after_terminal` pass, 560→50 instances, 91% reduction)
  - Items correctly kept as-is: `== null` (AS3 loose equality, Law 3), `as any` on shims
    (structural), concatenated string arrays (faithful to bytecode), `vN` names (from hasNext2
    tuple unpacking — structural to AVM2 iteration model)

---

## Session 2026-03-12 (session 7) — Type inference + readability + GML fixes

**String method type inference (Flash readability):**
- [x] Added `string_method_return_type()` to type inference — ECMAScript String prototype
  methods now return correct types (replace→String, indexOf→Float, startsWith→Bool).
  RedCastElim eliminates redundant Cast(String→String) after method calls.
  Flash output: 258→231 `String()` calls (−27 redundant).

**GML error reduction (Dead Estate 114→112):**
- [x] `coerce_to_bool` constant folding: All constant literal types (Int, UInt, Float, Null)
  now fold to Bool directly. Unwraps Coerce casts to reach inner literals. Fixes
  `!(!16777215)` → `true`, `!(!0)` → `false`. −2 TS2872 "always truthy" errors.
- [x] IR-level `Not` constant folding: Extended to handle Int/UInt/Float/String/Null operands
  using JS truthiness rules (was Bool-only).
- [ ] **~80 GMLObject→number errors NOT tractable via simple arithmetic widening** — see
  updated notes in TS2345/TS2322/TS2365 item below.

**GML error reduction continued (Dead Estate 112→100, session 8):**
- [x] `coerce_bool_args` reverse direction: boolean→number coercion at runtime call sites.
  Extended `is_already_boolean` to look through Cast(Coerce) wrappers. Added `may_produce_boolean`
  for ternary branches. New helpers: `coerce_to_number`, `is_numeric_param`. −6 errors.
- [x] Fixed `steam_file_read_buffer` return type in both `runtime.json` and `runtime.ts`
  (was `boolean`, should be `number` returning buffer ID). −2 errors.
- [x] Reordered extra passes: `GmlBoolArithCoerce` after `GmlLogicalOpNormalize`. LogicalOp
  creates blocks that pass Bool values to numeric block params; BoolArithCoerce must run after.
- [x] Fixed `coerce_bool_br_args`: use `value_types[param.value]` instead of stale `param.ty`;
  always cast to `Float(64)` (printer has no handler for `Coerce(_, Int(64))`). −2 errors.
- [x] Fixed `IntToBoolPromotion` internal sig demands: read `value_types[entry_param.value]`
  instead of `sig.params` (sig not updated by CallSiteTypeFlow/ConstraintSolve). −2 errors.

**GML error reduction continued (Dead Estate 100→98, session 9):**
- [x] `GmlBoolArithCoerce` Pass 3: Bool values stored via SetField to known numeric fields
  (GML built-in properties like `image_index`, `depth`, `speed` etc. + struct-defined numeric
  fields) get Cast(Bool→Float(64), Coerce). Fixes `this.image_index = (y > threshold)`. −1 error.
- [x] `GmlDefaultArgRecovery` DCE: after extracting sig defaults, replaces BrIf with
  unconditional Br to continue block. Prevents IntToBoolPromotion from changing body constants
  to boolean type when the sig default is numeric (was causing `argument4 = false` on number-typed
  param). −1 error.
- [x] `CallSiteTypeFlow` body-usage guard: skips narrowing when the callee body uses the param
  as a collection (GetIndex or GetField "length"). Defensive — prevents future regressions from
  narrowing array params to Float(64).
- [x] **TS2304 loop-escaping values (9 errors → 0)**: FIXED. Root cause was NOT
  `stack_effect()`/`compute_block_stack_depths` — it was Mem2Reg's Cytron-style phi placement
  skipping single-predecessor loop exits (dominance frontier computation skips blocks with <2
  preds). After CfgSimplify removes dead edges, self-loop exits have 1 predecessor, so no phi
  is placed. Values defined inside the loop body are referenced directly from the exit block.
  The linearizer scopes the defining `const` inside the while body, making it invisible after
  the loop. Fix: detect loop-escaping values in `EmitCtx::new()` (find self-loop blocks, find
  instruction results used in different blocks) and add their names to `shared_names` so the
  existing hoisting mechanism emits `let name!;` at function scope. File:
  `crates/reincarnate-core/src/ir/linear/emit.rs` lines 342-395.
- [ ] **TS2339 .length on number (15 errors)**: 12 from `instancePlaceList3d` — local variable
  `_meetingArray` (init noone/-4) resolved array writes to `self._meetingArray` instead of the
  local. Local stays -4, function returns -4, callers get `number` return type. Root cause:
  GML translator variable resolution maps `localvar[index] = value` to field writes instead of
  local array creation. 2 from param narrowing (CallSiteTypeFlow body-usage guard doesn't follow
  Alloc/Load chains). 1 from literal -4.

**GML cross-object stacktop field access (Dead Estate 74→52, session 12):**
- [x] **Skip PushI -9 sentinel for ref_type=0x80 cross-object stacktop**: Pattern is
  `PushLoc target → PushI -9 → Push.v/Pop.v [ref_type=0x80]`. The -9 sentinel is
  redundant; skipping it lets the stacktop handler pop the actual target instance.
  Previous instruction check covers PushLoc, PushGlb, Push, PushBltn, PushI, Call,
  CallV, Break (for pushref). −22 errors.
- [x] **@@Global@@ + PushI -9 pattern (6 errors fixed)**: `Call @@Global@@` pushes
  global scope, followed by `PushI -9`, then intermediate instructions (PushLoc for
  array index, Push Int64 for enum IDs). Fix: track `global_scope_on_stack` flag set
  by `Call @@Global@@` handler; when flag is set, unconditionally skip PushI -9.
  Without skip, 2D array handler pops dim2=-9 (self-scope) and global scope orphaned
  on stack. With skip, global scope lands in dim2 slot → dynamic-dim2 branch →
  `setOn(global_scope, field, index, value)`. −6 errors.
- [x] **instance_exists numeric object index (2 errors fixed)**: Widened runtime
  `instance_exists` to accept `number` (object indices, sentinels like -4/noone).
  Resolves numeric indices via `this.classes[target]`. −2 errors.

**GML remaining errors (Dead Estate 5, session 21):**
- [x] Shared blob arg ordering (2 TS2345): _init.ts `setInstanceFieldIndex` args swapped —
  fixed: `preceded_by_dup` check replaces `stack.len() >= 2` heuristic for compound_2d_pending.
  Dead Estate 8→6.
- [x] With-body capture gap (1 TS2345): DoctorMenu.ts:76 `variable_instance_get(id, 0.0)` —
  fixed: create alloc slots on-demand in with-body pre-store loop when CodeLocals is missing
  (obfuscated GMS2.3+ games). Dead Estate 6→5.
- ByRef capture gap (2 TS7053): OAnyaDoppelganger.ts:60,111 (`(-1)[0]`, `(-1)[int(sd)]`).
  Root cause: local `csa` initialized to `-1`, then modified inside `with` block to an array.
  IIFE ByValue capture means the modification doesn't propagate back to the outer scope — `csa`
  stays `-1` after with-body completes. Investigated ByRef capture (session 22): stripping the
  IIFE fails because Mem2Reg eliminates allocs whose only loads are capture values — the outer
  function has no `let csa` declaration, so the closure body's `csa` reference is undefined.
  Fix requires either (a) wrapper objects `{val: x}` with body rewriting, (b) preventing Mem2Reg
  from eliminating capture-source allocs, or (c) post-with write-back via modified IIFE return.
  All options are substantial; behaviorally the `-1` paths are dead code at runtime (wontfix).
- CallSiteTypeFlow narrowing (1 TS2339): _init.ts:7721 (`argument1.length` on number — callers
  pass numbers but body expects array; mixed usage)
- strictNullChecks artifact (1 TS2345): OBossRushController.ts:273 (`[].push()` on `never[]`)
- Unreachable code (1 TS7027): DiavolaEye.ts:81 (game-author infinite loop, wontfix)
- [x] **TS2349 `int` shadows import** — fixed: `rename_shadowing_locals` in backend sanitize pass
  renames local variables that collide with imported function names. Dead Estate 17→16.
- [x] **TS2362 `instance_create_depth` return type** — fixed: method overloads with `any`
  catch-all signature. `cls: any` (from `Foo as any`) resolves to `any` return, not `T`.
  Dead Estate 16→15.
- [x] **Bool-to-number coercion in call args** — fixed: Pass 4 in GmlBoolArithCoerce coerces
  effectively-boolean call args to Float(64) when callee entry block param is numeric.
  Uses `is_effectively_bool` to look through `Cast(Bool, Dynamic, Coerce)` chains.
  AchievementTester.ts:43 fixed. Dead Estate 16→15 (net: compensates strictNullChecks +1).

**Session 18 changes:**
- [x] **strictNullChecks enabled**: `RegExp.exec()![i]` non-null assertion in Flash rewrite.
  Flash 15→18 (+3 game-author), Dead Estate 15→16 (+1 shared blob `[].push(never[])`).
- [x] **Bool-to-number call args**: GmlBoolArithCoerce Pass 4. Dead Estate 16→15 (−1).

**Session 19 changes:**
- [x] **Mixed-type arithmetic result sizing**: `translate_arithmetic_op` and
  `translate_bitwise_cmp_op` now use `max(gml_slot_units(type1), gml_slot_units(type2))`
  for result size. Fixes `Add.i[v]` producing 1-unit result when operand is Variable (4 units),
  which corrupted Dup item counting. Dead Estate 15→13 (ObjSteamMusic draw_text fixed).
- [x] **Runtime signature fixes**: `buffer_load_async` param order swapped to match GML
  convention `(buf, path, offset, size)`. `buffer_save_async` param order fixed in
  runtime.json. `steam_file_get_list` returns `any[]` with `{file_name, file_size}` structs
  instead of `string[]`. `steam_music_set_volume` returns `number` (GML functions always
  return a value). Dead Estate 13→8.

**Session 20 changes:**
- [x] **Type-width-aware block depth computation**: Rewrote `compute_block_stack_depths` in
  `cfg.rs` to use `Vec<u8>` width stack instead of scalar `stack_effect` counter. Tracks
  per-item byte widths for Dup backward-walk, `pushac_pending` for popaf pop count,
  `global_scope_on_stack` for `@@Global@@` sentinel handling. Depth mismatches reduced
  182→78 across Dead Estate.
- [x] **func_ref_map resolution in cfg.rs**: `compute_block_stack_depths` used direct
  `function_names[function_id]` to detect `@@Global@@` calls, but in GMS2.3+ shared blobs
  `function_id` resolves to different names via direct lookup vs `func_ref_map` (absolute
  address → FUNC index → name). Translation uses `func_ref_map`; cfg.rs now matches.
  Also added previous-instruction guard to PushI -9 skip (matching translation).
  Depth mismatches: 78→8. Function names added to depth mismatch WARN messages.
- Remaining 8 mismatches: Barnacle::destroy (1), Barnacle::step_with (1),
  CsCharacter::draw (1), DevTool::step (2), DiavolaEye::step_with (1),
  FinalLock::step_with (1), Player::step (1) — all off by 1-3.

**Baselines (with strictNullChecks):** Flash 18, Bounty 0, Dead Estate 5, Undertale 8.

### Undertale quality sweep (2026-03-14)

First TS check on Undertale (GMS1, 1731 .ts files). Initial: 4447 errors. After bool-cmp
coerce fix (Pass 5 in GmlBoolArithCoerce): 3078.

**Error breakdown (3078 remaining):**
- TS2304 "Cannot find name" (~1200): missing runtime function stubs. Top functions:
  `script_execute` (673), `move_towards_point` (89), `sprite_replace` (65),
  `action_kill_object` (42), `tile_layer_shift` (24), plus ~50 other GML built-ins.
  Fix: add stubs to GML runtime (`instance.ts`, `draw.ts`, etc.).
- TS2339 "Property does not exist on type" (~800): `_rt` property access failures.
  Class type resolution issues — objects accessing fields/methods on other objects where
  the type system resolves to a numeric instance ID (Float64) instead of the class type.
  Same root cause as Dead Estate's fixpoint TS2339 spike. CallSiteTypeFlow limitation.
- TS2345 "Argument type not assignable" (~400): type mismatches at call sites, likely
  cascading from the TS2339/TS2304 issues above.
- TS7053 "Element implicitly has 'any' type" (~200): array/map indexing on wrong types.
- Other (~400): miscellaneous type errors.

**Chronicon**: YYC compiled (no CODE chunk). Cannot process — same as Katana Zero, Risk of Rain.

**Session 23 progress (3078→80):**
- Added 332 missing `function_signatures` entries to `runtime.json` (enables IntToBoolPromotion)
- Fixed ClassRegistry::lookup to try sanitize_ident fallback (digit-prefix names like "6parent")
- Added `RuntimeConfig::validate()` (RC0003 diagnostic) — hard-fails on function_modules/function_signatures mismatch
- Fixed `parse_type_notation` to handle `"any[]"`, `"string[]"` array notation
- Added ~300 runtime function implementations/stubs

**Session 24 progress: 80→11 errors.**

Session 24 fixes:
- `coerce_bool_cmp_operands` rework: `bool === 0` → `=== false` (not `Number(bool) === 0`)
- ~20 missing GML built-in function stubs added
- `_init.ts` import fixes
- `event_inherited()` → `super.collisionN(other)` forwarding (was missing `other` arg)
- Removed `"other"` from introduced-names lists (`rename_shadowing_locals` was renaming it to `_other`)
- `psn_init_np_libs` (0→3 params), `psn_init_trophy` (0→2 params), `action_previous_room` (1→0 params)
- `draw_text*` text param: `string` → `any` (GML auto-converts to string)
- `ini_close` return: `void` → `any` (GML allows `return void_expr`)
- `psn_setup_trophies` return: `void` → `number`
- Event method/field shadow fix: `this.create = 10` → `(this as any).create = 10`
  (GML separates event handlers and instance variables into different namespaces,
  TypeScript doesn't — typed method declaration shadows index signature)

**Remaining errors (8) — all wontfix:**
- TS7027 (6): unreachable code — game-author bugs
- TS2322 (2): Sansshadowgen `y = comparison` (game-author `===` instead of `=`),
  UndergroundExit `z = instance_create()` (game-author using built-in `z` to store instance)

*Open threads from a previous session. Treat as starting context, not instructions — verify relevance before acting.*

---

## Open threads (session 29 close)

### Subtype lower-bound design — attempted, root cause found, reverted

**Baseline after session 29:** 17,235 Dead Estate errors (−22 from session 28's 17,257).

Two changes shipped this session:
- Union constraint prepending (`abd07dc`): union constraints from `param_concrete_types` are now prepended before HasField constraints in the fixpoint. Prevents HasField single-candidate narrowing from preempting call-site union bindings. −1,517 TS2339, +1,156 TS2345, +414 TS18046; net −22.
- `gml_object_type_id: TypeId` on `TranslateCtx` (`20feea0`): replaced all `instance_types.get("GMLObject")` string lookups with a typed field initialized from `mb.intern_type("GMLObject")` at `TranslateCtx` construction. Eliminates silent None when GMLObject is not yet registered.

**What was attempted and reverted:** The full `param_lower_bounds` approach — changing ownerless script `self` from `Instance(GMLObject)` to `Var(fresh) + lower-bound fallback`, collected via `FunctionSig.param_lower_bounds: Vec<Option<Type>>` and `ConstraintSet.param_lower_bounds: Vec<(TypeVarId, Type)>`, applied as Step 4.6 in the HM solver after the fixpoint.

**Root cause of 92 `self: unknown` functions found:** Two HM passes run (separated by `inline`, `mem2reg`, etc.). Between Pass 1 and Pass 2, the intermediate transforms reset `func.value_types[self_val]` from `Instance(GMLObject)` (written by Pass 1) back to `Unknown`. Then in Pass 2's `collect_function`, `should_bind` logic fires: `is_concrete(Unknown) = true` AND the value is an entry-block param → pre-binds the solver Var to `Unknown`. Step 4.6's guard `matches!(resolve(Var(*var)), Type::Var(_))` then fails because the Var already resolved to `Unknown` — the lower bound is never applied.

**Fix for the should_bind problem (implemented then reverted with the rest):** Add `has_lower_bound` check to `should_bind` — don't pre-bind Unknown entry params that have a lower bound in `func.sig.param_lower_bounds`. This moved 92 → 5 `unknown` functions. But it exposed a second problem:

**Second regression: TS2345 from incomplete call-site capture.** The interprocedural loop only captures `Op::Call` with concrete-typed args into `param_concrete_types`. Class method callers that pass `this` via `Op::MethodCall` (or other paths) are NOT captured. When a script function's only valid callers go uncaptured, the union is biased to only the captured callers (e.g. `OLightParticle`) — all other callers then fail `TS2345: Argument of type 'this' is not assignable to 'OLightParticle'`. Third attempt net result: 19,686 (regression of +2,451).

**Design decision required for next attempt:** The union narrowing approach has a structural gap. Options:
1. Extend `param_concrete_types` capture to also include `Op::MethodCall` callers — requires identifying the param position each arg maps to through MethodCall dispatch.
2. Abandon union narrowing for `self` entirely: only apply the GMLObject lower bound (never emit a union). This is safe and correct — any call site that passes a subtype of GMLObject is already valid. 92→5 unknown would still be fixed.
3. Only apply the lower bound when there are zero captured call-site types (i.e. the union is empty) — use GMLObject as fallback only, not as a merge target when union exists.

Option 2 is lowest risk and directly fixes the `_self: unknown` problem. Option 3 would give the best narrowing but needs Op::MethodCall capture first.

**Design invariants verified this session:**
- `TypeConstraint::Subtype { sub, sup }` scaffolding exists in `ir/ty.rs` — not yet consumed
- `FunctionSig.param_lower_bounds: Vec<Option<Type>>` is the right channel (mirrors `defaults`)
- `ConstraintSet.param_lower_bounds: Vec<(TypeVarId, Type)>` for passing from collector to solver
- Step 4.6 (apply after fixpoint, before Step 5 write-back) is the right insertion point
- Union prepending (committed) must remain — it fixes a HasField/union race condition independently

**Remaining top buckets (after session 29, 17,235 baseline):**
- 10,483 TS2339 — property-not-found (bucket A residual = self typed GMLObject instead of specific union; B/C/D/E/F/G)
- 2,973 TS2345 — type argument mismatches
- 1,369 TS2571 — object of type 'unknown' (property access on unknown receiver)
- 666 TS18046 — 'x' is of type 'unknown' (includes `_self` params from withInstances U5 bucket)
- 620 TS2769 — no overload matches (downstream of unknown operands)

---

## Open threads (session 30 close)

### Subtype lower-bound — shipped, regressions diagnosed and fixed

**Baseline after session 30:** 16,501 Dead Estate errors (−734 from session 29's 17,235).

Three changes shipped this session:
- `param_lower_bounds` mechanism (`7596a5c`): ownerless script self now declared as `Unknown` with a `GMLObject` lower bound in `FunctionSig::param_lower_bounds`. Step 4.6 in the HM solver applies the lower bound when the Var is still free after the fixpoint. `should_bind` in `constraint_collect` leaves Unknown entry params with a lower bound free (not pre-bound). 519 scripts narrowed by call-site inference to concrete types; 835 fell back to GMLObject; `self: unknown` eliminated. −1,095 TS2339, +332 TS2345, net −427.
- `instance_destroy` param fix (`38649cb` partial): first param `Int(32)` → `Unknown` — was incorrectly constraining withInstances closure `_self` to number when body called `instance_destroy(_self)`.
- 41 additional instance/object builtins (`38649cb`): same wrong `Int(32)` pattern for params typed "Object Instance or Object Asset" — `instance_exists`, `object_is_ancestor`, `variable_instance_*`, etc. DS handle IDs (`ds_list_*`, `ds_map_*`, etc.) retained `Int(32)` — those are genuine integer handles. Net −307.

**Root cause of wrong Int(32) params:** Auto-generated builtins used `Type::Int(32)` for all GML "Object Instance or Object Asset" params. The constraint emitter's conflict guard (`arg_ty != param_ty`) only skips when `arg_ty` is a concrete non-Unknown type. Ownerless script self is now `Unknown`, so the conflict guard never fires → wrong `Equal(self_var, Int(32))` constraint was applied.

**Remaining top buckets (after session 30, 16,501 baseline):**
- 9,375 TS2339 — property-not-found (bucket B: GM built-ins missing from GMLObject; C: sibling field lift; F: Step/Draw/Alarm events; G: long tail)
- 2,983 TS2345 — type argument mismatches (pre-existing; net +10 from baseline, all from pre-existing patterns)
- 1,380 TS2571 — object of type 'unknown'
- 811 TS18046 — 'x' is of type 'unknown' (241 from withInstances keyword-id U5; rest from other unknown fields/params)
- 651 TS2769 — no overload matches (downstream of unknown)
- 613 TS2322 — type assignment mismatch

**Immediately actionable next buckets:**
- **U5** (241 TS18046): `withInstances(-9, ...)` closures in ownerless scripts emit `_self: unknown`. Root cause: `ctx.class_name = None` for ownerless scripts → target sentinel -9 resolves to `None` → `Var(fresh)` → Unknown. **Naive fix is a landmine**: setting `instance_class = Some("GMLObject")` converted 241 TS18046 into +7,420 TS2339 (each property access on `_self` fails instead of each variable use). The correct fix requires inference: the HM solver must be able to narrow the closure's `_self` from the field accesses (`checkInventory`, `updateInventory`, `sectionList`, etc.) rather than locking it to GMLObject. This is the same HasField-based narrowing that Step 4.5 does — but those closures' self params need to be left as `Var(fresh)` AND the HasField narrowing must succeed. Currently HasField narrowing fails for these because the fields exist on many different classes. **Real fix:** extend Step 4.5's multi-field narrowing to these closures, or propagate the outer script's narrowed self type into the closure's self (so if the script's self narrows to OAnya from call-site inference, the `with (self)` closure also gets OAnya).
- **Bucket C** (~1,300 TS2339): fields set in sibling subclasses read from shared parent. `ConstructorStructInfer` common-ancestor lift — design decision on trigger condition.
- **Bucket B** (~2,000 TS2339): GM built-in instance fields missing from GMLObject (`bbox_*`, `sprite_width/height`, `room_*`, `image_blend`, etc.). Blocked on runtime IR-generation migration but fields could be added to the existing handwritten runtime as a stopgap.
- **Bucket F** (~500 TS2339): fields set in Step/Draw/Alarm events — ConstructorStructInfer only walks Create. Landmine: earlier attempt caused regressions. Needs careful design before retrying.

---

## Open threads (session 27 close)

### NEXT LEVER (pivot, user-approved 2026-06-05) — `self` / `this.field` unknown inference

After U4 landed correctly but closed no gaps (see "Dead Estate inference work — corrected baseline …" item 6), the gap-closing effort pivots to `self` / `this.<field>` unknown inference. This is BOTH the dominant `unknown→X` source AND the `this→SpecificType` receiver-mismatch bucket (~28.5% / 599 of TS2345).

> **CORRECTED 2026-06-12:** the U4 "low payoff because evidence doesn't exist" rationale recorded here was wrong. Re-derivation shows 87% of the 393 zero-call-site functions ARE called via `@@NewGMLObject@@` constructor erasure, GMS2.3 closures, and method-vars — the evidence exists in the bytecode and is destroyed or obscured before the solver sees it. The pivot to self/this.field is still the right next move (it is the dominant error source and is independent), but the U4 residual is now understood as a pipeline defect, not an information-theoretic ceiling. Two high-priority defect entries recording the root causes are appended below.

- **Shared prerequisite (deferred from U4):** multi-call-site UNION narrowing. A consistent 2+ call-site param emits `Equal(Union([...]), var)`, but `unify`'s `(Union, _) => Unknown` arm (`constraint_collect.rs:361`) collapses it to Unknown. Fix the arm (substitute rather than collapse; its own ratchet check). This unblocks both the U4 union residual and the self/this.field receiver bucket.
- **Corrected invariant (now law, `ad31f37a`):** `Type::Unknown` is CONCRETE — terminal, not a free var. `is_concrete(Type::Unknown) == true` and the `saw_unknown` gate are correct by design, not bugs. Do not re-open.
- The "Subtype constraint design" section below is the relevant prior design BUT IS PARTLY STALE: `self` is no longer initialized to concrete GMLObject — the frontend now emits it as Unknown + `param_lower_bounds = GMLObject` with a post-fixpoint fallback (`constraint_solve_hm.rs` ~1104–1123). Verify current solver state before acting on that section.

### Subtype constraint design — the real unifier for call-site param inference

> **PARTLY STALE (2026-06-05):** the premise below — that `self` is typed concrete `GMLObject` at IR construction and thus skipped by the `is_concrete` guard — no longer holds. `self` is now emitted as Unknown + `param_lower_bounds = GMLObject` with a post-fixpoint fallback (`constraint_solve_hm.rs` ~1104–1123). Re-verify current state before using this design; keep the union/Subtype reasoning, discard the stale GMLObject-init framing.

The core inference gap driving the bulk of remaining errors is that `self` and `argumentN` params in global (ownerless) GML scripts are typed `GMLObject` at IR construction time. This makes them concrete (`is_concrete(GMLObject) = true`), so the HM solver's call-site inference loop skips them entirely (guard at `constraint_solve_hm.rs:644`).

The naive fix — init `self` as `Var(fresh)` instead of `GMLObject` — was tried and reverted: it regressed from 21,151 → 26,766 because unresolved vars emit as `unknown`, breaking all GMLObject-level field accesses.

The right model: a `Subtype(A, B)` constraint ("A is assignable to B") so:
- `self` starts as `Var(fresh)` with an implicit lower bound that it must satisfy GMLObject's field set
- Call sites add `Subtype(OPlayer, self_var)` etc.
- Solver resolves to the tightest type satisfying both directions

Key insight from session discussion: the correct param type is NOT the nearest common ancestor in the class hierarchy — it's the **union of call-site arg types** (`OPlayer | OEnemy`), BUT only valid if all union members pass all field-access constraints the function body imposes. Common-ancestor collapsing loses information; TypeScript's union type is the right representation.

Current `Equal`-based union accumulation is partially right in shape but wrong because:
1. It never runs on `self` params (blocked by `is_concrete` guard)
2. A union `OPlayer | OEnemy` is only semantically valid if both members have all fields the body accesses — the solver currently doesn't verify this

**Open questions:**
- What is the minimal `Subtype` constraint representation that fits the existing solver without full subtype lattice machinery?
- Is the inheritance graph in `module.types` (parent chains) sufficient to check "does type T have field F"? (HasField constraints already walk it — might be enough)
- Should the solver's fixpoint be extended to iterate until Subtype constraints stabilize, or is one pass sufficient?
- The GMLObject lower-bound for ownerless scripts: explicit `Subtype(Var, GMLObject)` constraint, or emit GMLObject only at emit time as a fallback when Var stays unresolved?

This is the next high-leverage inference design decision. The TS2339/TS2345/TS2571/TS18046 triage buckets (A residual, U1, U4, U5) are all downstream of this.

---

**Session 27 (Dead Estate → 21,156 stable — `any` removal + determinism + targeted inference):**

Removed `[key: string]: any` from `GMLObject` (commit `ba08b31`) — this was the primary
suppression hiding ~19k errors. Pre-session baseline was unmeasurable: error counts varied
~200 run-to-run due to pipeline non-determinism (HashMap iteration order in GM frontend
FuncId registration + HM solver). After determinism fixes, stable baseline is **21,156**.

Session 27 fixes applied (→ 21,156 stable):
- `_rt.global` typed as `GameGlobalState` intersection: emits `_global_state.ts` alongside
  `_globals.ts`; constant-key `Global.get`/`Global.set` rewrites cast `_rt.global` to it.
  Closed ~9,548 TS2339. (commits `8c5e4c7`, `691e91a`)
- `InstanceType::Arg` writes routed to self-field, not `_rt.global`. Haxe struct constructor
  fields (`argument.superClass`, etc.) were falling to the global-write default. (commit `9e12c2d`)
- Lifted script/event `self` typed from CODE entry name rather than hardcoded `GMLObject`.
  `anon@<n>@gml_Object_<Owner>_<Event>_N` pattern parsed in one localized helper.
  `translate_scripts` now passes `class_name: Some(owner)` for recognized patterns. (commit `ac96a09`)
- Pipeline determinism: `register_function_stub` now iterates `function_names` in sorted
  FUNC-index order (`lib.rs:195`); `param_concrete_types` and `var_fields` in
  `constraint_solve_hm.rs` use `BTreeMap` so constraint insertion / HasField narrowing are
  order-independent. `TypeVarId` gained `Ord` via the `define_entity!` macro.

**Dead Estate error breakdown (21,156 stable after Session 27):**

By code:
- 12,743 TS2339 "Property X does not exist on type"
- 3,342 TS2345 — type argument mismatches
- 2,634 TS2571 — Object of type 'unknown'
- 847 TS2769 — no overload matches
- 672 TS18046 — 'x' is of type 'unknown'
- 447 TS2322 — type not assignable
- 299 TS2551 — property doesn't exist on type
- 85 TS7053, 24 TS2362, 22 TS2349, 10 TS2365, long tail

**TS2339 bucket breakdown (12,743 — triaged Session 27):**

| # | Est. | Root cause | Status |
|---|------|-----------|--------|
| A | ~7,200 (−444 actual) | Lifted script self typed GMLObject — owner class encoded in CODE entry name. Residual: field-type inference after self is correctly typed. | DONE `ac96a09` |
| B | ~2,000 | GM built-in instance fields missing from GMLObject base: `bbox_*`, `sprite_width/height`, `room_height/width`, `async_load`, `image_blend`, `image_number`, `current_time`, `view_camera`. | Blocked — runtime `object.ts` slated for IR-generation deletion (`06cc501`). Fix direction: add to IR-generated runtime, not handwritten file. |
| C | ~1,300 | Field set in sibling subclasses, read from shared parent. Example: `OEmptySpaceController.makerStart` (532 hits) set in 7 `LevelController` siblings. | Open — ConstructorStructInfer needs common-ancestor lift. Design decision: trigger condition. |
| D | ~1,050 | Field set inside `withInstances` closure body, not recorded on target class. Example: `_self.witchItem = []` in `ONoSpillBloodController`. | **ADDRESSED (session 28, Phase 3):** `ConstructorStructInfer` now scans `MethodKind::Closure` when `param[0].ty == Type::Instance(id)` — these are `withInstances` closures with a resolved target. Class name derived from `module.type_name(id)`. Commit `4ee2df5`. |
| E | ~650 (−266 actual) | `InstanceType::Arg` writes routed to `_rt.global` instead of self-field. | DONE `9e12c2d`. Residual: struct shapes still not inferred — downstream of C/D/F. |
| F | ~500 | Fields assigned in Step/Draw/Alarm events missed — ConstructorStructInfer only walks Create events. | Landmine: earlier attempt caused Union regressions (reverted). Design decision needed before retrying. |
| G | ~400 | Long tail: `event14_0` (77), reserved identifiers (`__enum__`, `__class__`), etc. | Not yet categorized. |

**Unknown-propagation bucket breakdown (7,958 across TS2345/TS2571/TS18046/TS2769 — triaged Session 27):**

TS2769 is 100% downstream — all 847 sites are `add_any`/`sub_any`/`mul_any`/`div_any`/`neg_any`
overload failures where at least one operand is `unknown`. Fixing Bucket 1 eliminates TS2769.

| # | Root cause | Scale | Fix location |
|---|-----------|-------|--------------|
| U1 | Class field types declared `unknown` or `T \| unknown` (two assignment sites produced different TypeVars that were unioned instead of unified; or one branch yielded Unknown and solver preserved it in the union). | 1,515 / 3,679 class fields. Drives most TS2571, all 847 TS2769, chunks of TS2345. Example: `WanderingStar.ts:17` `declare starParticle: unknown` despite assignment from `part_type_create(): number`. `OAnyaFinalRank.ts:44` `declare textAlphaLerp: number \| unknown`. | **ADDRESSED (session 28, Phase 2):** `constraint_solve_hm.rs` Step 6.5 resolves stale TypeVars in struct field types post-HM; drops `Unknown` from unions when a concrete alternative exists, mirroring CSI's merge policy. Commit `0859d81`. |
| U2 | `getInstanceField` returns `unknown` by signature; emitter sometimes lowers to direct typed access (`as number`) and sometimes doesn't. | Several hundred TS2571/TS18046. Example: `_init.ts:1048` casts `(… as number)` correctly; `_init.ts:3554` `_rt.getInstanceField(_rt.global, "destroy")()` does not. | Emitter dispatch — audit why direct-access lowering fires only sometimes when the field name is a literal and receiver type is known. |
| U3 | `GameGlobalState` fields all `unknown` | Steady share across all four codes wherever `_rt.global` is touched. | Global-state builder — same inference failure as U1 but for globals. |
| U4 | Script-function params `unknown` despite default literal (`= 0.0`) and concrete call-site args. | 478 occurrences of `argumentN: unknown` in `_init.ts`. ~500+ TS2345, big TS18046 slice. Example: `_init.ts:2243` `healAnyaExt(_rt, self, argument0: unknown = 0.0)`. | **SUPERSEDED.** The `1688012` default-seeding approach was a floor (unsound) and is gone. Reimplemented correctly via floor-removal (`5ab379d4`) + evidence gate (`e48b488c`) — see 2026-06-05 item 6. ~~Bounded payoff: 66% of freed `argumentN` params have no call sites; the dominant unknown→X source is self/this.field, untouched by U4.~~ **CORRECTED 2026-06-12:** re-derived population is 645 `_init.ts` script functions with `argumentN: unknown`; 393 (60.9%) have zero direct call sites. Of those 393, only ~50 (~13%) are genuinely dead (GIF library + platform stubs). ~343 (87%) ARE called via mechanisms the pipeline doesn't statically resolve — `@@NewGMLObject@@` constructor erasure, CallV closures, method-vars — and the `saw_unknown` gate is correctly starved of evidence by those pipeline defects (see new defect entries below and in item 6). |
| U5 | `withInstances(keyword_id, ...)` callbacks emit `_self: unknown` (e.g. -9 = `other`); the typed form works for concrete class targets (e.g. `OTentacleChunks.ts:386` has `_self: OTentacleChunks`). | 120 occurrences, ~246 TS18046. Example: `_init.ts:3555` `_rt.withInstances(-9.0, (_self: unknown): void => {...})`. | `withInstances` lowering — keyword-id branch should resolve `other` to caller's `_other` type, not fall back to `unknown`. |
| U6 | Array/ds_list element types `unknown` | Small TS2571/TS18046. Example: `BloodController.ts:85` `b[2.0] * 1.5` where `b` is unknown-element array. | Array-field element inference in `constraint_solve_hm.rs`. Subset of U1 when the field is array-shaped. |
| U7 | `_other.*` field access typed `unknown` | 54+ TS18046. Example: `OStickyBomb.ts:82` `_self.damage = _other.damage * 15.0`. | Same pass as U5 — `_other` should be parameterized with caller's instance type. |

Priority: U1 (highest ROI) > U4 > U5 > U3 > U2 > U7 > U6.

**Session 26 (HM inference improvements — Dead Estate 5→24249):**

This session focused on error count reduction after a structural regression: emit was switched
to typecheck mode (using `cargo run -p reincarnate-cli -- check`) which revealed 52,195 errors
that had been hidden behind cast-based suppression. These are genuine inference gaps.

Session 26 fixes applied (52,195 → 24,249):
- `HasIndex` constraint for array element type inference via `Op::GetIndex`/`Op::SetIndex`
- `TypeVarArena::did_bind` flag to detect fixpoint progress across deferred constraints
- `is_definitely_scalar` guard replacing `!is_struct_arg` in inter-proc constraint linking
- Union accumulation: instead of conflicting `Equal(A, param)` + `Equal(B, param)` from
  different call sites, emit `Equal(Union([A, B]), param)` — fixes script function self params
- `declare` field emission for GML create-event fields: reads from `module.types` (enriched
  by `ConstructorStructInfer`) instead of only `module.structs` (frozen frontend snapshot)
- `ConstructorStructInfer` extended to scan all `MethodKind::Instance` methods (not just
  Constructor), accumulating fields per class name. Fixes event14_0 (GML Pre-Create / variable
  definition event) field declarations — affected 887/~900 objects.

**Dead Estate error breakdown (24,249):**
- 16,202 TS2339 "Property X does not exist on type" — dominated by:
  - 9,555 where receiver is typed `GMLObject` (includes `_rt.global.X` global var accesses)
  - ~6,647 where receiver is a specific class but field is missing (cross-function fields)
- 2,879 TS2345 — type argument mismatches
- 2,503 TS2571 — Object of type 'unknown'
- 949 TS18046 — 'x' is of type 'unknown'
- 832 TS2769 — no overload matches
- 428 TS2322 — type not assignable
- 280 TS2551 — property doesn't exist on type (similar to TS2339)

**Remaining known gaps:**
- `_rt.global` typed as `GMLObject` — needs game-specific `GlobalState extends GMLObject`
  class with `declare` fields for each GML global variable. Affects ~9,555 TS2339 errors.
  Currently global variables appear in `_globals.ts` but `_rt.global.X` accesses don't
  use that type. Fix requires emitting a GlobalState class and retyping `GameRuntime.global`.
- Fields set by script functions on passed-in instances (e.g., `_self.radius = 40.0` in a
  script called with Gun instances) not collected by `ConstructorStructInfer` — the script
  function's SetField ops aren't attributed to any class because `_self: GMLObject`.
  Fix: when union accumulation resolves `_self` to a specific class, attribute those SetField
  ops to that class for field declaration purposes.
- `object_index` missing from `GMLObject` runtime base class (114 TS2339 errors).

**Session 25 fixes (11→8):**
- For-loop scoping (TS2304 `len`): guard in `try_promote_while` prevents promotion when
  update expression references variables declared in the loop body.
- Local/global name collision (TS2345 `$set_confirm`): `rewrite_global_assignments` now
  tracks local VarDecl names through recursive scope traversal, skipping shadowed names.
  Root cause was NOT type inference — it was the rewrite incorrectly treating a local
  `confirm` assignment as a global setter call.
- Noone sentinel (TS2367 Discoball): `coerce_noone_sentinel` pass replaces `-4` with `null`
  in equality comparisons where the other operand is a call result. Fixes both the TS error
  and a behavioral bug (our runtime returns null, so `null !== -4` was always true).

---

## Adversarial Commit Audit (2026-03-14)

Automated review of 366 commits (2026-03-01 to 2026-03-14) by 6 parallel haiku auditors.
Each commit diff was read and evaluated against project principles. Findings below.

### Suppressions (cast/widen to hide type errors)

- [x] **`5788533` — `coerce_bool_cmp_operands` inserts `Number(x) === 0`** (fixed: replaced numeric const with bool const)
  Instead of `=== false`. Known issue; rework to `bool === 0` → `=== false`, `bool === true` → `bool`,
  `bool === false` → `!bool`.

- [ ] **`72cd19e` — instance_create_depth/layer overloads with `any` catch-all**
  Silences TS2362 from `Cls as any` arguments. Root cause: emitter passes class refs as `any`.

- [x] **`23bf784` — Object.values/keys cast to `any[]` in ForOf**
  Fixed: `binding_ty: Option<Type>` on ForOf + printer casts to `any[]` only when Dynamic.
  See architecture audit entry (2026-03-13).

- [ ] **`9f8a89c` — addEventListener event type widened to `any`**
  AS3 events are covariant; TypeScript function params are contravariant. Needs proper typing.

- [ ] **`a329a8e` — `null as unknown as Type` for null→struct casts**
  Double cast hides that the IR produces invalid `Coerce(null, Struct)`.

- [ ] **`9419220` + `763c745` — param type widened to `any` for default value mismatch**
  Two commits generalizing "if param type doesn't match default, use any". Root cause: type inference
  doesn't account for GML's variadic calling convention default values.

- [x] **`b7d8c57` — `(target as unknown) === -4` in instance_exists**
  Addressed: `coerce_noone_sentinel` pass (e5916d4) translates `-4` → `null` at call sites.
  Runtime still accepts both null and -4 for safety. The `as unknown` cast was already removed.

- [x] **`f9b5159` — `(expr as any) instanceof T` to prevent TS never-narrowing**
  Valid translation: GML objects can change type at runtime, so `this instanceof OwnClass` can
  genuinely be false. TS narrowing assumes it can't → else-branch becomes `never`. The `as any`
  cast preserves correct runtime semantics per Law 3.

- [x] **`7b74dc9` — void conditions wrapped in `as any` cast**
  Valid translation: AS3/GML allow `if (voidExpr)` (always falsy). TS disallows (TS1345). The
  `as any` cast is behaviorally necessary per Law 3, not a suppression.

### Workarounds (narrow fix instead of root cause)

- [x] **`8ca4fff` — trailing `throw` for TS2366 non-void functions**
  Acceptable workaround: `throw new Error("unreachable")` is the standard TS pattern for
  unprovable exhaustiveness. Fails loudly at runtime if reached. Root-cause fix (restructuring
  emitter output for TS satisfaction) is deep structural work with no behavioral benefit.

- [ ] **`ee0a98c` — `null as any` shims placeholder in static construct calls**
  Static methods have no `this` for shims. Should make shims optional or thread through properly.

- [ ] **`59c4c93` — downgraded debug_assert to warning for block arg count mismatch**
  Hides a real bug in `compute_block_stack_depths`. Should fix the CFG analysis.

### strictNullChecks preparation (debatable)

- [ ] **`2167f85` — Flash getters widened to non-nullable**
  AS3 properties can genuinely be null; widening to non-nullable loses information. May be acceptable
  if AS3 semantics guarantee non-null in practice.

- [ ] **`d9477bd` — null-initialized fields widened to `T | null`**
  May be correct (fields ARE nullable) or may be hiding wrong zero-initialization inference.

- [ ] **`16f6696` — `!` definite assignment assertion on Flash instance fields**
  AS3 zero-initializes all fields; TS doesn't know this. Debatable whether `!` or explicit init is better.

### Silent stubs

- [x] **`5c2bb7e` — `irandom` returns 0 for negative/NaN input**
  This IS correct GML behavior: `irandom(-5)` returns 0 in GML. Not a silent stub — faithful impl.

### Already fixed

- [x] **`26708e7` — Bool→Number coercion in arithmetic (reverted in `c9e5096`)**

### Conversation-log audit (2026-03-14)

Second pass: 10 parallel haiku auditors reviewed ~950KB of assistant messages from 40 sessions
(last 2 weeks). Extracts from `~/git/rhizone/normalize/target/debug/normalize sessions messages --days 14 --role assistant --limit 0`.
Items below are NEW findings not in the commit-diff audit above.

**tsconfig flag suppressions** — disabling TS strictness flags instead of fixing the emitter:
- [ ] `noUncheckedIndexedAccess: false` — AS3 arrays don't have `| undefined`, but the fix should
  be in the emitter (emit non-nullable array types), not globally disable the check.
- [x] `noImplicitReturns: false` — re-enabled in `a085631`. `effective_return_type()` handles
  non-void functions with bare returns (widens to `T | undefined`, emits `return undefined;`).
- [x] `allowUnreachableCode: true` — re-enabled 2026-03-13. Unreachable code now surfaces as
  TS7027 (correct: game-author bugs like infinite loops and `&& false`).

**Index signatures on non-dynamic AS3 classes** — `[key: string]: any` added to TimeModel,
MainView, Player etc. that are NOT dynamic in AS3. The real issue is the emitter generating
dynamic property access on statically-typed objects. Need to audit which classes genuinely need
index signatures vs. which got them as suppressions.

**`null!` / NULL_ASSERT thread-local** [883fa191] — Flash strictNullChecks: a thread-local flag
makes all `null` literals print as `null!`. This is a blanket suppression across all Flash null
assignments. Should instead: (a) emit explicit field initialization, or (b) use AS3-aware nullable
type inference.

**ClassRef → `any` in `ts_type`** [8823a47c] — `Type::ClassRef` emits `any` in function signatures
(was `typeof ClassName`). The `typeof` was wrong (propagated constructor type to callers), but `any`
is also wrong — it loses all type information. Root cause: ClassRef values are GML integer object
indices that should resolve to class types in the IR, not be carried as ClassRef to the backend.

**Runtime field accessor widening** [6099df2c] — `getInstanceField`/`setInstanceField`/
`setInstanceFieldIndex` accept `number` (object type index). 32 errors suppressed. The real fix:
resolve integer object indices to class references in the IR (partially done in `f3d3915`).

**`undefined as unknown as FlashShims`** [b1df458e] — double cast in static/cinit context where
there's no `this._shims`. Should make shims optional or thread it through properly (same root
cause as `ee0a98c` above).

**XML/XMLList → `any`** [cebbfa87] — both `ts_type` and `flash_ts_type` map XML/XMLList to `any`
to avoid TS2538 (can't use as index) and TS2322 (not assignable to string). Should define proper
XML type with index signature and toString() coercion.

**AS3 `Class` → `any`** [b1df458e] — `Type::Struct("Class")` mapped to `any` because `Function`
doesn't have index signatures. Should use a proper Class interface with index signature.

**`return 0 as any` implicit returns** [c5f8f8fd] — catch-all trailing return for GML implicit-0
semantics. Later self-corrected to type-appropriate returns (`return false` for bool, `return 0`
for number). The fix in `051e5a9` addressed this.

### Remaining `as any` in TypeScript emitter (2026-03-18)

Every item below is a Law 4 / no-any violation in the emitter. Root causes tracked here; fixes
must close the inference gap, not widen further.

- [ ] **ForOf Dynamic binding → `any[]` iterable cast** (`ast_printer.rs` ~753)
  `for (x of (iter as any[]))` emitted when `binding_ty` is `Dynamic` (AVM2 for-each-in).
  TypeScript disallows type annotations on for-of bindings (TS2483). Root cause: AVM2 for-each-in
  iterates over object values whose element type is unknown. Fix: infer the element type from the
  iterable's IR type (e.g. `Array<T>` → `T`) so the cast can be narrowed to a specific type; or
  express the iterable as `Iterable<T>` and let TS infer the binding type without a cast.

- [ ] **Switch literal discriminant → `as any`** (`ast_printer.rs` ~819)
  `switch (0.0 as any) { case 1: ... }` — constant folding leaves a float literal where an integer
  case is expected, causing TS2678. Fix: constant folding should preserve integer vs float
  distinction (Law 4); a float `0.0` that is used as a switch discriminant should fold to `0`
  (integer). Once discriminant and case types agree, the cast is unnecessary.

- [ ] **`NullableCoerce + Dynamic` → `as any`** (`ast_printer.rs` ~1082)
  `(voidExpr as any)` when a void-typed value is used as a boolean condition (TS1345 in AS3/GML).
  Root cause: a `Dynamic` result type means inference failed to determine the real type. Fix:
  infer the actual return type of void-returning functions; the condition should then be wrapped
  with a proper truthiness helper rather than erased via `any`.

- [ ] **`NullableCoerce + ClassRef` → `as any`** (`ast_printer.rs` ~1088)
  GML OBJT class refs (integer object indices) are widened to `any` at use sites so they can
  appear in numeric/arithmetic contexts. Root cause: `Type::ClassRef` carries the class constructor
  where the IR should carry an integer ID. Fix: resolve OBJT integer indices to a typed
  `ObjectIndex` IR type (or `Int(32)`) so the backend never sees `ClassRef` in arithmetic context.
  Tracked separately under `ClassRef → any` above.

- [ ] **`TypeCheck instanceof` → `(expr as any) instanceof T`** (`ast_printer.rs` ~1332)
  GML objects can change type at runtime, so `this instanceof OwnClass` can genuinely be false.
  TS control-flow narrows the else-branch to `never`. The `as any` prevents the spurious
  narrowing. Fix: use a type predicate function (`isInstance(this, Wall)`) that TS treats as
  opaque, removing the need for `as any` while preserving runtime semantics.

- [ ] **Flash `null as any` shims placeholder in static construct calls**
  (`rewrites/flash/expr.rs` ~592-596)
  Static factory methods (`cinit`) have no `this._shims`; `null as any` is injected as a
  placeholder. Root cause: shims are threaded through `this`, unavailable in static context.
  Fix: make shims an optional parameter (`shims?: FlashShims`) or pass them via a module-level
  accessor so static callsites can supply a real value. Also tracked as `ee0a98c` above.

- [ ] **Flash XML/XMLList construct → `as any`** (`rewrites/flash/expr.rs` ~611-616)
  `new XML(...)` wrapped in `as any` so the result is assignable to string-typed fields (TS2322).
  AS3 XML has implicit `toString()` coercion on string assignment; TypeScript's XML class does
  not. Fix: define a proper `XML` runtime class with implicit string coercion via `toString()`
  and an appropriate union/interface type so no cast is needed. Also tracked as `XML/XMLList →
  any` above.

- [ ] **GML `(this as any).field` for event-method name conflicts** (`rewrites/gamemaker.rs` ~962)
  When a game variable name matches an event method (e.g. `self.create = 10`), the typed method
  declaration on `GMLObject` shadows the `[key: string]: any` index signature, causing TS2339.
  Fix: separate event methods from the instance variable namespace in the emitted class hierarchy
  (e.g. emit event methods on a separate `GMLObjectEvents` mixin) so the index signature is
  never shadowed. Alternatively, emit event methods with a mangled name and alias them.

- [ ] **`Type::ClassRef` → `any` in `ts_type()`** (`types.rs` ~57)
  Function signatures with `ClassRef` parameters emit `any` because `typeof ClassName` propagates
  the constructor type misleadingly to callers. Root cause is the same as `NullableCoerce +
  ClassRef` above: ClassRef should be an integer ID in the IR, not a type-level class reference.
  Fix: resolve to `ObjectIndex` / `Int(32)` in the IR; remove the ClassRef arm from `ts_type`.

---

## Adversarial Architecture Audit (2026-03-13)

Review of ~175 commits (2026-03-06 to 2026-03-13). The error-count reduction campaign
(Dead Estate 228→16) introduced suppression patterns that weaken output quality.

### Critical

- [x] **`noImplicitReturns: true`** — re-enabled 2026-03-13. `effective_return_type()` detects
  non-void functions with bare `return;` and widens to `T | undefined`, emits `return undefined;`.
  Eliminates 8 Flash TS7030 errors. 0 GML impact.
- [x] **`allowUnreachableCode: false`** — re-enabled 2026-03-13. +1 correct TS7027 per game
  (DiavolaEye infinite loop, HelFollower `&& false`).
- [x] **`strictNullChecks: true`** — enabled (scaffold.rs). **Phase A done:** GML runtime widened
  (`getInstanceField`/`setInstanceField`/`setInstanceFieldIndex` accept `null`). **Phase B
  partial (2026-03-13):** Flash runtime getters widened to non-nullable (`parent`, `getChildByName`,
  `applicationDomain`, `bytes`, `content`, `loader`, `focus`, `restrict`). Flash strictNullChecks
  now 21 (down from 39). Dead Estate = 18 (+1), Bounty = 0.
  **Remaining Flash strictNullChecks work (~6 new errors):**
  - `this.field = null` assignments where zero-initialized fields use `T!` (~40 errors).
    Widening ALL zero-init reference fields to `T | null` causes 12K+ cascading errors.
    Needs targeted analysis: track which fields are assigned `null` in method bodies and
    widen only those fields' types.
  - 3 `RegExp.exec` nullable returns (TS2531) — need `!` or null guard in emitted code.
  - 3 game-author patterns (nullable used as non-null).
  **File:** `scaffold.rs:336`

- [x] **`switch (x as any)` on every switch statement.**
  `ast_printer.rs` wraps every switch discriminant with `as any` to suppress TS2678.
  Defeats TypeScript exhaustiveness checking and hides genuine type mismatches. Law 4 violation.
  **Fixed (2026-03-13):** `as any` now only emitted for literal discriminants (constant folding
  artifacts like `switch (0.0) { case 1: ... }`). Non-literal switches get full TS type checking.
  Fixes: (1) `ensure_trailing_unreachable` injects throw into switch default body instead of
  after switch, preventing TS7027. (2) `video_get_format` runtime type corrected to `number`.
  **File:** `crates/backends/reincarnate-backend-typescript/src/ast_printer.rs`

- [x] **Return type inference keeps `Dynamic` for no-return functions — GML-specific in core.**
  `type_infer.rs:888-894` skips return-type narrowing for functions with no value-bearing
  returns, with comment "In GML, all functions implicitly return a value." Flash functions
  with no `return <value>` should infer `void`, not `Dynamic`. Law 2 violation.
  **Fix:** Add a `Function`-level or `Module`-level flag (e.g. `implicit_return_value: bool`)
  set by GML frontend, read by type inference. Don't hardcode GML semantics in core.
  **File:** `crates/reincarnate-core/src/transforms/type_infer.rs:888-894`

### Concerning

- [x] **`CallSiteArityWiden` is a GML-only pass living in core.**
  Docs and `PassConfig` comment say "GML loose calling convention." No other engine needs it.
  Should be a GML `extra_pass`, not a core pass. Harmless as no-op for other engines but sets
  a bad precedent.
  **File:** `crates/reincarnate-core/src/transforms/call_site_arity_widen.rs`

- [x] **`Object.values()/keys()` cast to `any[]` in printer.** (2026-03-13)
  Fixed: added `binding_ty: Option<Type>` to `Stmt::ForOf` and `JsStmt::ForOf`.
  `control_flow.rs` sets to `Some(Dynamic)` for AVM2 for-each-in. Printer casts iterable
  to `any[]` when `binding_ty` is Dynamic (TS doesn't allow type annotations on for-of
  bindings — TS2483). Replaced fragile `Object.values`/`Object.keys` pattern-matching hack.

- [x] **`is_gml_numeric_field` hardcodes 20+ field names.** (2026-03-13)
  Replaced with `build_external_numeric_fields()` reading from `module.external_type_defs`.
  Also fixed runtime.json: `visible`, `solid`, `persistent` changed from `"number"` to `"*"`
  to match the `number | boolean` class declarations — prevents TS2322 regressions.
  **File:** `crates/frontends/reincarnate-frontend-gamemaker/src/bool_arith_coerce.rs`

- [ ] **Accumulated guards in PushI -9 skip (3 commits widening same guard).**
  `ops.rs:168-203` has 8 preceding-opcode variants, 2 conditions, and a state flag. The
  underlying issue: translator doesn't model -9 as a stack-machine scope marker. Each new
  pattern adds another guard instead of fixing the abstraction.

---

## Next Session: Remaining Refactors

**Completed module splits (2026-03-12):**
- [x] `translate.rs` (4650 lines) → `translate/` module directory (7 files)
- [x] `emit.rs` (7767 lines) → `emit/` module directory (7 files)
- [x] `flash.rs` (3197 lines) → `flash/` module directory (8 files: context, scope, expr, stmt, super_hoist, method_bind, dead_activation, mod)

**Remaining splits:**
- [ ] `linear.rs` (4434 lines) — `crates/reincarnate-core/src/ir/linear.rs`.
  Already split into `linear/{mod,linearize,resolve,emit,tests}` but some files still large.
- [ ] `variable_access.rs` (991 lines) — `translate_push_variable` (498 lines) and
  `translate_pop` (422 lines) each need internal refactoring (shared stacktop resolution,
  2D array access, instance dispatch helpers). Prerequisite: `TranslationState` struct
  to bundle the 10+ function parameters.

**Also remaining:**
- core `pub` → `pub(crate)` visibility tightening
- error handling consistency redesign (save for dedicated session)
- test coverage (Flash frontend: 14 tests, GML rewrites: 4 tests, no e2e snapshots)

---

## Architecture & Adversarial Audit (2026-03-11) — mostly complete

Ran 3 parallel Opus subagents. 15/18 findings fixed. Remaining items below.

### Law 2 Violations — Engine-Specific Logic in Core (HIGH)

Items already tracked and fixed above (type_infer.rs, linear.rs, ast_passes.rs) are marked [x].
New findings from this audit:

- [x] **`type_infer.rs:590` — `build_global_types` hardcodes `"GameMaker.Global"/"set"`.** (2026-03-11)
  Added `SystemCallTypeRule::GlobalStore { name_arg, value_arg }`. GML frontend registers it.
  `build_global_types` reads from `store_rules` map — no hardcoded strings in core.

- [x] **`int_to_bool.rs:319` — `function_has_with_instances` hardcodes `"GameMaker.Instance"/"withInstances"`.** (2026-03-11)
  Added `Module::callback_return_calls: BTreeMap<(String, String), ()>`. GML frontend
  registers `("GameMaker.Instance", "withInstances")`. `int_to_bool` reads the map.

- [x] **`linear.rs:2456` — `xml_construct_string_coerce()` inserts `.toString()` for Flash XML.** (2026-03-11)
  Gated behind `LoweringConfig::construct_string_coerce` (default false, Flash backend sets true).

- [x] **`call_site_flow.rs:97` — ClassRef narrowing guard comment updated to be engine-neutral.** (2026-03-11)

- [ ] **`register_core_builtins()` in `module.rs` — `add_any`/`sub_any`/`mul_any`/`div_any`/`mod_any`
  and their `specializations` tables are GML-specific.** The polymorphic `_any` arithmetic stubs
  and their overload-selection tables exist only because GML `+` is overloaded for string
  concatenation. Other languages don't need them. These registrations belong in the GML frontend's
  module-init code, not in core's `Module::new()`.
  **Fix:** Move `_any` registrations + specialization tables to GML frontend. Remove from core.
  **Blocked on:** `DataType::Variable → Type::Var` fix above (after which `_any` is no longer
  emitted for the dominant Variable-arithmetic path and can be deleted cleanly).

- [ ] **`transforms/builtin_overload_select.rs` — GML-specific pass in core.**
  Post-HM pass that replaces `_any` calls with typed variants via `Function::specializations`.
  Uses `specializations` map that only GML frontend populates. No other engine needs this.
  **Fix:** Move to GML frontend's `extra_passes`. Remove from core transform list.
  **Blocked on:** same as above.

### Structural — Dead Code & Config Bugs (CRITICAL/HIGH)

- [x] **System traits in `reincarnate-core::system/` are 100% dead code (~350 lines).** (2026-03-11)
  12 traits (`Audio`, `Graphics`, `Input`, `Timing`, `Persistence`, `Dialog`, `Files`,
  `Images`, `Layout`, `Network`, `SaveUi`, `SettingsUi`) — zero implementations, zero
  consumers. These were aspirational API surface that was superseded by the platform
  interface design. Delete or move to `docs/` as design reference.

- [x] **`VALID_PASS_NAMES` missing `call-site-arity-widen`.** (2026-03-11)
  `transform.rs:48-60` — pass exists in pipeline and `PassConfig` but absent from
  `--dump-ir-after` validation list. Users can't dump IR after this pass.

- [x] **`--skip-pass int-to-bool-promotion` silently does nothing.** (2026-03-11)
  `config.rs:144` — docstring lists it as valid, but no `PassConfig` field exists and
  the match arm falls through to `_ => {}`. The pass comes via `extra_passes` from the
  GML frontend, so the skip mechanism can't reach it. Either add a field or remove from docs.

- [x] **Duplicated `--skip-pass` match logic in `config.rs`.** (2026-03-11)
  `PassConfig::from_skip_list` (line 144) and `Preset::resolve` (line 281) have identical
  copy-pasted match blocks. Adding a new pass requires updating both. Extract shared method.

### Panics on Malformed Input (HIGH)

- [x] **53 `args.pop().unwrap()` in system call rewriters** (2026-03-11)
  Extracted `take_arg(args, "call_name")` helper in `rewrites/mod.rs`. All 53 call sites
  in gamemaker.rs, sugarcube.rs, and engine.rs now use it with descriptive call names.

- [x] **Flash translator stack underflow panics** (2026-03-11)
  `resolve_property` and `translate_op` now return `Result<_, String>`. Errors propagate
  to `translate_method_body` which already returned `Result`.

### Code Quality — Monster Functions (MEDIUM, mostly done)

| File | Function | Lines | Status |
|------|----------|-------|--------|
| ~~`translate.rs` (GML)~~ | ~~`translate_instruction`~~ | ~~730~~ | DONE 2026-03-11 — split into 9 themed helpers |
| ~~`emit.rs`~~ | ~~`emit_module_to_dir`~~ | ~~589~~ | DONE 2026-03-11 — split into 6 focused functions |
| `structurize.rs` | `structurize_region_inner` | 494 | Recursive CFG recovery |
| `translate.rs` (GML) | `translate_push_variable` | 498 | GML variable access |
| `translate.rs` (GML) | `translate_pop` | 423 | GML variable store |
| `emit.rs` | `emit_class` | 389 | Class emission + field layout + methods + traits |

Also done: ~~`ast_passes.rs`~~ (2026-03-11 — `ast_passes/` with `AstPass` trait),
~~`linear.rs`~~ (2026-03-11 — `linear/{mod,linearize,resolve,emit,tests}`).
Also done: ~~`runtime.ts` GML~~ (2026-03-11 — split into `object.ts`, `room.ts`, `particles.ts`; 4463→4115 lines).

### Code Quality — Inconsistent Error Handling (MEDIUM, partially fixed 2026-03-11)

- [x] `CoreError` dead variants removed (`UnsupportedFormat`, `Type`, `Codegen`) (2026-03-11)
- [x] `CoreError::Translate` added — distinguishes translation errors from file parse errors (2026-03-11)
- [x] CLI `anyhow!("{e}")` → plain `?` operator — preserves typed `CoreError` in error chain (2026-03-11)
- Frontends still use `Result<_, String>` internally. The 5 error sites (1 Flash, 4 GML)
  produce bytecode-offset messages; a typed `TranslateError` per frontend would add
  structure but the benefit is marginal given so few sites.
- Transform panics are deliberate ICE-style assertions (same pattern as LLVM/rustc).
  These guard internal invariants assumed from prior passes — converting to `Result`
  would propagate error handling through the entire pipeline for bugs that should never
  reach users. Correct as-is.

### ~~Code Quality — `sanitize_ident` Violates CLAUDE.md Rule~~ (FIXED 2026-03-11)

Now uses `unicode_ident::is_xid_start`/`is_xid_continue` per CLAUDE.md rule.

### ~~Code Quality — Silent `_ => {}` Catch-Alls~~ (FIXED 2026-03-11)

Replaced with explicit variant lists in emit.rs type/import collection, call_site_flow.rs,
and mem2reg.rs. New Op/Type variants now trigger compiler errors.

### Visibility — Overly Broad `pub` (LOW, partially fixed 2026-03-11)

- [x] Flash frontend `lib.rs` — internal modules changed to `pub(crate)` (2026-03-11)
- [x] GML frontend `naming` module — changed to `pub(crate)` (2026-03-11)
- [x] Twine frontend internal modules — changed to `pub(crate)` (2026-03-11)
- [x] `reincarnate-core` visibility tightening (2026-03-11):
  `ir::ast_passes` module → `pub(crate)` (all items internal-only).
  `ir::structurize` re-exports: removed `build_cfg`, `compute_dominators_lt`,
  `dominates`, `Cfg`, `Shape`, `BlockArgAssign` (all internal-only). Only
  `structurize()` remains re-exported.

### ~~Duplicated Platform Directories~~ (FIXED 2026-03-11)

Unified 3 diverging files (audio.ts, images.ts, persistence.ts) — GML copies updated
to match Flash's stricter types. All 7 platform files now byte-identical. Drift risk
remains if edited independently; consider symlink or build-time copy if it recurs.

### Test Coverage Gaps (LOW — already partially tracked)

- GML translator: 13 tests for 4021 lines (improved 2026-03-12, up from 5)
- Flash translator: 14 tests for 2302 lines (improved 2026-03-12, up from 4)
- GML rewrites: 4 tests for 1482 lines
- No end-to-end snapshot tests (tracked at "Snapshot tests for both frontends" above)

### Minor

- [x] `datawin` crate: workspace package metadata (`version.workspace = true` etc.) (2026-03-11)
- [x] `datawin` crate: renamed from `reincarnate-datawin` to `datawin` (2026-03-11)
- [x] `unicode-ident` dep in backend: now uses `workspace = true` (was hardcoded `"1"`) (2026-03-11)
- [x] `Preset` unit struct → `resolve_preset()` free function (2026-03-11)
- [x] `Linker` unit struct → `link_modules()` free function (2026-03-11)
- `eprintln!` diagnostics in `builder.rs:436-498` — debug-only (`cfg!(debug_assertions)`),
  5 warnings for stack-depth mismatches. Too few to justify a logging framework; fine as-is.
- ~~`Module` and `Function` derive `Clone` — accidental deep copies~~ Audited 2026-03-11:
  7 clone sites, all necessary (coroutine lowering, test helpers, CLI backend input).
- ~~Bool-coercion passes~~ — analyzed: Flash and GML passes solve different root causes
  (Flash: bool comparisons + void handling; GML: bool arithmetic + block param mismatches).
  Only ~15 lines of `insert_cast_before` helper are duplicated. Not worth extracting until
  a third similar pass emerges.

---

## Warning Diagnostics for Game-Author Bugs

- [x] **Diagnostic infrastructure** — `Diagnostic` type (level: Warning/Info, source location,
  message) threaded through pipeline stages and reported in `check` output. Implemented.
- [x] **`dedup_object_keys` AST pass** — RC0002 diagnostic for duplicate keys in object literals.
  Flash `newObject` rewrite preserves duplicates; AST pass deduplicates with warning.
- [x] **Duplicate-case check on all switch statements** — `check_switch_duplicate_cases` runs
  as the final step of `recover_switch_statements`, catching duplicates in native switches
  AND those introduced by `try_recover_switch_discriminant`. Verified on TelAdre.ts (CC).
- [ ] **Emit diagnostics from more passes** — Audit existing passes for similar
  opportunities (dead code, type mismatches that are game-author bugs not inference failures).
- [x] **Switch recovery passes placement** — Investigated 2026-03-12. These passes correctly
  belong in the TS backend: they operate on `JsExpr` case labels (arbitrary expressions like
  `Keyboard.UP`, enum values) which core `Stmt::Switch` cannot represent (uses `Constant` only).
  The engine-agnostic vs engine-specific distinction is correct here.

---

## Developer Experience / Tooling Gaps (HIGH PRIORITY)

- [x] **Session tooling review** — Completed 2026-02-28. Found gaps below (items added as separate entries).

- [x] **`--dump-function` doesn't match class-qualified names** — Fixed 2026-02-28. `should_dump()` now tries: (1) case-sensitive substring, (2) case-insensitive substring, (3) split-part matching on `.`/`::` — so `"Gun.step"` matches `"Gun::event_step_2"`. Flag ordering was a non-issue (clap handles it fine).

- [x] **`list-functions` subcommand** — Implemented 2026-02-28. `reincarnate list-functions [--filter <pattern>]` prints all IR function names, using same matching as `--dump-function`.

- [x] **`--dump-ir-after <pass>` flag** — Implemented 2026-02-28. Runs pipeline up through named pass, dumps IR (honoring `--dump-function`), then exits. Special value `frontend` dumps raw IR before any transforms. Valid pass names listed in `VALID_PASS_NAMES`.

- [x] **Bytecode disassembler subcommand** — Implemented 2026-02-28. `reincarnate disasm [--function <filter>]` disassembles GML bytecode directly from DataWin (no IR pipeline). Resolves variable names, strings, function names, instance types, and break signal names. Same `--function` filter matching as `--dump-function`. Without `--function`, lists all CODE entry names.

- [ ] **TypeScript error archaeology tool** — No way to go from a specific TS error (file + line) back to the IR value that caused it. An `explain-error <file> <line>` subcommand could resolve the error back to the IR value ID and the GML bytecode offset that produced it.

- [x] **`reincarnate check` workflow adoption** — Documented in CLAUDE.md 2026-02-28.

- [x] **`reincarnate check` curated diagnostic output** — Implemented 2026-02-28. Default output shows counts by code + up to 3 deduplicated examples per code. `--examples N`, `--filter-code`, `--filter-file`, `--filter-message` all implemented. Filters compose (AND), show matching/total counts, affect `--json` output. Pass/fail and baseline comparison always use unfiltered totals.

- [x] **Pipeline fixpoint stress tester** — Implemented 2026-02-28. `reincarnate stress [--runs N] [--skip-pass P] [--preset P]` runs the pipeline N times (default 5), reports fixpoint convergence or oscillation (detects matches against any prior run, not just adjacent), and per-run changed function count.

## API / Interface Tech Debt Audit (HIGH PRIORITY)

Audit the entire codebase for awkward APIs, fragmented interfaces, and places where the right fix was avoided in favor of an easier one. The bar: if a fresh reader would reasonably ask "why does this exist separately?" or "why does this take this parameter?", it's a candidate. Specifically look for:

- Methods that exist only because callers were too numerous to update (e.g. `foo_variant()` alongside `foo()` that should have replaced it)
- Parameters added to avoid breaking existing callers instead of updating them
- Parallel data structures that should be unified (e.g. two maps that always move together)
- `Option<&T>` parameters that are always `Some` or always `None` in practice
- Traits with a single implementor (abstraction that never paid off)
- Builder methods that silently assume defaults (width, depth, kind) that callers should be forced to state
- Any `_v2`, `_new`, `_impl`, `_inner` suffix naming

For each finding: state the correct API, estimate the refactor scope, and add as a tracked item. Do not add workaround items — only correct-fix items. Flag anything where the scope is large enough to warrant its own phase in the Incremental Rewrite Plan.

## TODO.md Staleness Audit (HIGH PRIORITY)

- [x] **Audit TODO.md for stale items** (2026-03-11)
  Sweep complete. Found 4 stale items marked done: flash/memory.ts, project registry,
  HAL audio library, feature-gate CLI. Remaining open items are genuinely unimplemented.

---

## End-to-End Regression Tests

- [ ] **Snapshot tests for both frontends** — No snapshot infrastructure yet.
  - **Flash**: 15 new vN identifiers regressed in `91fe86e` (MethodCall
    refactor). Pre-existing 5 vN (hasNext2 one-shot, split-path phi).
  - **GML**: Reference decompilation at `~/git/bounty/`.

## Unit Test Infrastructure

### GML Translator Tests

- [ ] **Narrow `DataWin` dependency in `TranslateCtx`** — `translate_code_entry`
  takes `ctx.dw: &DataWin` but only uses it for string resolution (3 call sites:
  local var name resolution × 2, push-string operand × 1). Replace with a
  `resolve_string: &dyn Fn(u32) -> &str` field (or equivalent) so tests can
  construct a `TranslateCtx` without a real `DataWin`.

- [x] **GML translator unit tests** — 13 tests added (2026-03-12): 2D array write
  (scalar + non-scalar), compound 2D assignment, argument mapping, PushEnv/PopEnv
  with-block, self-field read/write, call opcode, arithmetic, cmp, bf→br_if,
  push string, push double. `DataWin` dependency narrowed via `string_table` field.

### Flash Translator Tests

- [x] **Flash translator unit tests** — 14 tests added (2026-03-12): return void,
  push int+return, add two bytes, getlocal/setlocal, iffalse/iftrue branch,
  get/set property, callproperty, newarray, newobject, coerce/convert, pushstring,
  pushdouble, dup+swap. Minimal `AbcFile` builder (`make_abc`, `build_abc`,
  `pool_with_qname` helpers) included — no real SWF files needed.

## Type System

### Open

- [ ] **GML instance ID type propagation** — When `instance_create_depth(x, y, d, OFoo)` is
  called, the return type should be inferred as `OFoo` (or `InstanceRef<OFoo>`), not `any`.
  This type must flow through assignments and field accesses — `let enemy = instance_create(..., OEnemy);
  enemy.health -= 1` should type `health` as a field of `OEnemy`. Also: `withInstances(inst, cb)`
  should type `_self` in `cb` as the same type as `inst`. This is critical for emitting maintainable
  code — without it, every cross-instance field access is `any`-typed. Requires the type inference
  pass to understand `instance_create_*` return types, and to propagate the object class through
  the type system as `Struct(className)`.

- [ ] **Flow-sensitive narrowing** — Narrow types after guards
  (`if (x instanceof Foo)` → `x: Foo` in then-branch). Requires per-block type
  environments rather than the current single `value_types` map.
- [ ] **Flash frontend: emit concrete types** — AVM2 bytecode has type
  annotations on locals, parameters, fields, return types. `resolve_type`
  failures cause unnecessary `Dynamic` entries.
- [ ] **Untyped frontend validation** — Test the inference pipeline against a
  fully-untyped IR (simulating Lingo/HyperCard).

### Remaining `:any` analysis (Flash cc-project, 541 total)

Measured after TypeInference + ConstraintSolve + Alloc refinement.

| Category | Count | Root cause |
|----------|-------|------------|
| `any[]` arrays | 185 | Array element type unknown — no element-type inference yet |
| Parameter `: any` | 186 | Untyped function params from ABC metadata gaps |
| Return `: any` | 79 | Functions returning Dynamic (unresolved return types) |
| Field `: any` | 80 | Struct fields without type info (empty class defs, external supers) |
| `let` locals | 11 | Block params where incoming args don't all agree |
| `const` locals | 9 | Genuinely untyped values from calls/block params |

- [x] **Enum constants not initialized (CockTypesEnum)** — Fixed 2026-03-12. Two-part fix:
  (1) `is_redundant_static_assign` now only strips cinit assignments for const fields with
  `default.is_some()`. (2) Flash construct rewrite treats `is_cinit` as static context,
  injecting `null as any` instead of `this._shims`. The 2 TS2348 errors from
  `ParseConstant`/`ParseConstantByIndex` were fixed separately — `this(this, arg)` is type
  coercion (not construction), emitted as `asType(arg, ClassName)`.

### Known Issues

- **Multi-typed locals** — Some Flash locals are assigned different types in
  different branches (e.g. `race` initialized to `0.0` as a sentinel, then
  assigned `this.player.race()` which returns `string`). These correctly stay
  `Dynamic` / `:any` today. For TypeScript this is ugly but functional. For
  Rust emit this is a hard blocker. Options:
  - **Split into separate variables** per SSA def when types disagree
  - **Enum wrapper** — tagged union for the specific types observed
  - **Sentinel elimination** — recognize sentinel-then-overwrite → `Option<T>`
  - Emit union type annotation (`number | string`) instead of `any`

## Runtime Audits (Evergreen)

These are recurring health-checks, not one-off fixes. Run them periodically and update the "last audited" date.

### Module-level mutable state — last audited 2026-03-09

State at module scope prevents two game instances from coexisting on the same page. Any `let` or lowercase-named singleton `const` at the top level of a `.ts` file is a smell.

```bash
# Top-level let declarations (mutable by definition)
grep -rn "^let \|^export let " runtime/ --include="*.ts" | grep -v node_modules

# Top-level singleton instances (lowercase name + new = likely singleton)
grep -rn "^const [a-z].* = new " runtime/ --include="*.ts" | grep -v node_modules
```

Known violations as of 2026-03-09:
- `runtime/flash/ts/flash/memory.ts` — `heap`/typed array views at module scope (AVM2 Alchemy domain memory); fix requires moving to `FlashRuntime` + updating emitter to call `this._rt.memory.load_i8(...)` instead of free function imports

### GML runtime stubs — silent returns audit — last audited: (never)

Many GML built-in stubs added in 2026-02 silently return `0`, `""`, `false`, or `-1` for
functions that require real implementations (collision, path-finding, particle systems,
video, vertex buffers, DS operations, etc.). Per the CLAUDE.md rule, these should throw
`Error("name: not yet implemented")` instead of returning wrong values silently.

```bash
# Find suspicious silent-return stubs in GML runtime
grep -n "{ return 0; }\|{ return \"\"; }\|{ return false; }\|{ return -1; }" \
  runtime/gamemaker/ts/gamemaker/runtime.ts | grep -v "// genuine"
```

Functions to audit (partial list):
- `mp_grid_*` — pathfinding (returns -1/void, but caller uses return value for grid ID)
- `collision_point/circle/ellipse` — collision (returns -4 but caller may iterate over results)
- `path_start/path_get_length` — path following
- `part_*` — particle system state
- `instance_deactivate_all`, `instance_furthest`, `instance_position`
- `ds_priority_*` — DS priority queue (partially implemented)
- `buffer_*` async variants
- `surface_copy`, `vertex_*` — graphics
- `layer_get_depth`, `layer_x`, `layer_y` — layer state queries

Confirmed untracked silent stubs (found 2026-03-09 audit):
- [x] **`asset_get_tags` / `asset_has_tags`** — now `throw Error("... not yet implemented")` (2026-03-10).
- [x] **`file_find_first` / `file_find_next`** — now throw (2026-03-10).
- [x] **`directory_exists`** — now returns `false` with explicit no-op comment (2026-03-10).
- [x] **`buffer_async_group_end`** — now throws (2026-03-10).

### Runtime type widening — last audited: (never)

Review runtime files for type signatures widened to silence TypeScript errors from game code (e.g. `string` → `any`, required param made optional). Such changes hide real bugs.

```bash
# Hunt for any-typed params/returns that shouldn't be
grep -rn ": any\b" runtime/ --include="*.ts" | grep -v node_modules | grep -v "Record<string, any>"
```

Audit targets: `runtime/twine/ts/harlowe/`, `runtime/twine/ts/sugarcube/`.

The rule: never widen types to accommodate buggy game code — TypeScript catching game author mistakes is correct behavior.

---

## GML Runtime Stub Remediation — Critical

Audit all `throw Error("X: not yet implemented")` stubs in the GML runtime. For each:
1. **Implement fully** if the function is straightforward (math, string, data structure ops).
2. **Categorize by system** if the function needs design work before implementation.

### Throw-stubs added
- `event_inherited()` — runtime fallback; normally rewritten to `super.eventName()` at compile time. Only hit if backend rewrite doesn't fire (e.g. arrow function context). See `rewrites/gamemaker.rs`.
- `draw_healthbar()` — needs proper canvas 2D rendering with gradient interpolation between mincol/maxcol, background/border drawing, and directional fill.

### Systems requiring design before stubs can be implemented

- **Particles** — `part_system_*`, `part_type_*`, `part_emitter_*`. Needs a particle system
  architecture (emitter/type/system hierarchy, integration with draw loop).
- **Surfaces** — `surface_*`. Needs offscreen canvas management, texture lifecycle.
- **Networking** — `network_*`. Needs socket abstraction, async integration.
- **Physics** — `physics_*`. Needs physics engine integration (Box2D equivalent).
- **Shaders** — `shader_*`. Needs WebGL shader pipeline.
- **Buffers** — `buffer_*`. Needs typed ArrayBuffer wrapper with GML's type system (u8/s8/u16/s16/u32/s32/f16/f32/f64/bool/string).
- **Cameras** — `camera_*`. Needs camera/viewport matrix system.
- **Tiles** — `tile_*`, `tilemap_*`. Needs tile layer rendering system.
- **Paths** — `path_*`. Needs path point/interpolation system.
- **Timelines** — `timeline_*`. Needs timeline moment scheduling.
- ~~**Steam API** — `steam_*`. **~50 functions silently return falsy values** (`0`, `false`, `""`, `[]`) instead of throwing.~~ **FIXED 2026-03-15 (`410bb4a`)**: ~119 stubs converted to `throw new Error("funcname: not yet implemented")`. Persistence-backed functions kept: `steam_file_*`, `steam_get/set_stat_*`, `steam_get/set_achievement`, `steam_store_stats`. Long-term: proper extension/adapter interface where browser adapter returns appropriate no-op values *opt-in*, not by default.
- **OS/Platform** — `os_*`, `file_*`, `ini_*`, `directory_*`. Needs platform abstraction (browser vs native).
- **3D / Primitives** — `d3d_*`, `vertex_*`, `matrix_*`. Needs WebGL 3D pipeline.

### Known type/emit gaps requiring prerequisite work

- **`int64` bigint**: `int64()` return type is currently registered as `Unknown` and the
  handwritten TS stub emits `n | 0` (lossy 32-bit truncation). The correct return type is
  `Type::Int(64)`. TypeScript emit should use `bigint` / `BigInt(x)`. Proper fix requires
  int64 type representation throughout the backend (separate TS type, separate arithmetic ops).
  Do not close this gap with a widened `number` return — that silently loses the upper 32 bits.

- **`string` format lowering**: The multi-arg form `string(fmt, v1, v2, ...)` uses GML
  format-string placeholders (`{0}`, `{1}`, ...). The frontend should lower format-string calls
  to explicit IR string operations at the call site (same pattern as `with`-inlining in Phase
  B/C). The 1-arg form `string(val)` maps to `to_string_unknown`. No IR body is attached to
  `string` because the stub has `has_rest_param: true` — attaching a 1-arg body would silently
  produce wrong output for multi-arg calls (extra args ignored). The correct fix is to lower
  format-string calls to explicit IR at the frontend before any body is attached.

### Straightforward stubs (implement immediately when encountered)

These need no design — just correct GML semantics → TypeScript translation:
- Math functions (`point_direction`, `lengthdir_x/y`, `dot_product`, etc.)
- String functions (`string_count`, `string_pos`, `string_delete`, etc.)
- Data structure operations (`ds_list_*`, `ds_map_*`, `ds_grid_*`, `ds_stack_*`, `ds_queue_*`, `ds_priority_*`)
- Array functions (`array_length`, `array_copy`, `array_sort`, etc.)
- Color/drawing utility (`merge_colour`, `make_colour_*`, `colour_get_*`)
- Instance queries (`instance_nearest`, `instance_furthest`, `instance_position`)
- Room/view queries (`room_get_name`, `room_goto_next`, `room_goto_previous`)

---

## Runtime Architecture — High Priority

### Flash runtime: module-level singletons (multi-instance blocker)

All mutable state in the Flash runtime lives at module scope, which means two Flash games cannot run on the same page and a second `createFlashRuntime()` call would stomp on the first. All of the following need to move onto the `FlashRuntime` instance (analogous to how `GameRuntime` was designed from the start).

- [x] **`flash/display.ts` — exported drag state** (`_dragTarget`, `_dragBounds`, `_dragLockCenter`, `_dragOffsetX`, `_dragOffsetY`). Replaced with `_dragStateByStage: WeakMap<Stage, DragState | null>` — now keyed by Stage so each FlashRuntime has isolated drag state.
- [x] **`flash/display.ts` — `_displayState` singleton** — fixed (2026-03-09)
- [x] **`flash/display.ts` — `_timelineFactories` map** — fixed (2026-03-09)
- [x] **`flash/text.ts` — `_textState` singleton** — fixed (2026-03-09)
- [x] **`flash/deflate.ts` — `_deflateState` singleton** — fixed (2026-03-09)
- [x] **`flash/timing.ts` — `TimingState` singleton** — fixed (2026-03-09)
- [x] **`flash/input.ts` — `InputState` singleton** — fixed (2026-03-09)
- [x] **`flash/audio.ts` — `AudioState` singleton** — fixed (2026-03-09)
- [x] **`flash/renderer.ts` — hardcoded canvas** — fixed (2026-03-09)
- [x] **`flash/memory.ts` — shared heap** (2026-03-11, found done in staleness audit)
  `FlashMemory` class with per-instance `ArrayBuffer` heap; instantiated via `FlashShims.create()`.

### Twine platform: module-level singletons (multi-instance blocker)

- [x] All Twine platform singletons fixed (2026-03-09): `save.ts`, `_overlay.ts`, `input.ts`, `layout.ts`, `save-ui.ts`, `settings-ui.ts`

### `flash/utils.ts` — module-level class registries (multi-instance, emitter change required)

`_interfaceRegistry` (WeakMap<Function, Set<Function>>) and `_traitRegistry` (WeakMap<Function, ClassTraits>) are already keyed by constructor identity and thus effectively per-game. But `_classRegistry` (Map<string, Function>) is keyed by qualified name string, meaning `getDefinitionByName("com.example::Foo")` would return whichever game registered that name last.

- [ ] **`flash/utils.ts` — `_classRegistry` string-keyed map** (`getDefinitionByName` returns wrong class if two games use the same qualified name). Fix requires the emitter to generate per-runtime registration calls (`_rt.registerClass(Foo)` instead of module-level `registerClass(Foo)`) — non-trivial emitter change. Low priority: Flash games running simultaneously is rare.
- [ ] **`flash/utils.ts` — `_bindCache` WeakMap** — `as3Bind()` caches bound functions globally across all games. Since it's keyed by `(function, thisArg)` pairs which are game-specific objects, no cross-game pollution occurs in practice. Safe to leave.

### `flash/vector.ts` — `Array.prototype` mutation

- [ ] **Prototype pollution** — `vector.ts` adds `removeAt` and `insertAt` to `Array.prototype` (lines 17–27) as a side effect of import. This bleeds across the entire page. Correct fix: either use a typed `Vector<T>` wrapper class (correct but invasive), or at minimum scope the side effect to only apply when the Flash runtime is initialized rather than on module load.

---

## Third-Party Engine Libraries

Many real-world games embed third-party macro/plugin libraries alongside the engine.
These appear as `unknown_macro` warnings (Twine) or unresolved calls (other engines)
but are *not* missing built-ins — they are authored libraries distributed alongside
the game. Each needs to be identified, understood, and either implemented against the
platform layer or stubbed with a clear diagnostic.

**General strategy:** For each library, read its source embedded in the game, understand
its API, and decide:
1. **Implement** — map to the platform layer (e.g. audio library → platform audio)
2. **Stub** — no-op with a runtime warning (purely visual effects, dev tools)
3. **Warn at compile time** — emit a named diagnostic instead of per-call-site noise

**Detection:** During extraction, scan for library registration patterns
(e.g. `Chapel.Macros.add(` in Harlowe, `Macro.add(` in SugarCube) and record
discovered library names + macro lists in `FrontendOutput` metadata. This enables
precise warnings ("uses HAL audio — implement platform audio shim") rather than
generic unknown-call spam.

**Known libraries observed in the wild:**

- [x] **HAL (Harlowe Audio Library)** (2026-03-11, found done in staleness audit)
  All 7 macros implemented in `harlowe/translate.rs`: `masteraudio`, `track`, `playlist`,
  `group`, `newtrack`, `newplaylist`, `newgroup`.


## Format Spec (game_maker_data.ksy)

- [ ] **Submit to kaitai_struct_formats** — PR to `kaitai-io/kaitai_struct_formats`
  under `game/game_maker_data.ksy`. This is the "never RE it again" step — the spec
  is only findable by the community once it's in the format gallery.
  *Low priority — gated on polish (full chunk coverage, fixture-validated, clean doc strings).*

- [x] **gml_bytecode.ksy** — Separate Kaitai spec for GML instruction encoding.
  Covers: opcode layout (v14 vs v15+ numbering), operand formats (Double/
  Int32/Int64/String/Variable/Int16), Break signal encoding (GMS2.3+ extended
  signals including pushref/chknullish/isstaticok), Dup type-size semantics,
  variable_ref bit layout, instance_type enum, branch offset encoding.
  Lives at `crates/formats/datawin/gml_bytecode.ksy`.

- [ ] **datawin fixture tests (ongoing — goal: 100% format coverage)** — 15 synthetic
  fixtures covering all 20 chunk types now exist in `crates/formats/datawin/tests/fixtures/`
  (57 Rust tests, validated by Kaitai). The goal is complete coverage of every variant the
  format specifies: every field, every conditional branch, every version difference. Keep
  expanding when there's time: GMS2 TXTR entries (currently only GMS1 format tested),
  OBJT with actual events + event actions, ROOM with object instances, physics vertices,
  BC≥17 OBJT `_managed` field, SPRT with multiple tpag entries, multi-entry FONT with
  multiple glyphs, SEQN with GMS2.3+ full entries, etc. Each new case: add builder in
  `gen_fixtures.rs`, regenerate (`cargo run -p datawin --bin gen_fixtures`), add tests in
  `fixture_tests.rs`, update `kaitai_validate.py`. Real-game tests in `tests/read_files.rs`
  still provide broader coverage when run with `--include-ignored`.

## CLI — Project Registry

- [x] **Project registry** (2026-03-11, found done in staleness audit)
  `ProjectRegistry` in `registry.rs` with `~/.config/reincarnate/projects.json`, versioned schema,
  `add`/`remove`/`list` subcommands, bare path positional args, `--all`/`--parallel` emit.

## CLI — Build Configuration

- [x] **Feature-gate frontends, backends, and checkers in the CLI** — All 5 plugin crates are behind
  Cargo feature flags (`frontend-flash`, `frontend-gamemaker`, `frontend-twine`, `backend-typescript`,
  `checker-typescript`), all enabled by default. Build with `--no-default-features` for a minimal binary.

## Future

- [ ] **Game icon extraction → web favicon** — Extract game icon from engine-specific location and
  set as `<link rel="icon">` in the emitted `index.html`. Current state: nothing exists.
  - GameMaker: icon stored in OPTN chunk of data.win; OPTN parser reads flags/constants but not icon binary. Needs Win32 .ico / bitmap parsing. ~200 LOC.
  - Flash: icons are regular image assets (DefineBits/DefineBitsLossless) — no semantic distinction from sprites. Heuristic: smallest/first image.
  - Phase 1 (trivial): Add favicon tag to HTML scaffold (`scaffold.rs`), accept optional icon path param.
  - Phase 2 (easy): Add `icon: Option<AssetMapping>` field to `ProjectManifest`; wire through emission.
  - Phase 3 (medium): Parse GameMaker OPTN icon; add `AssetKind::Icon` variant; mark in Flash extractor.

- [ ] **Split OPFS out of localStorage backend** — `runtime/gamemaker/ts/shared/platform/persistence.ts` currently bakes OPFS in as a fire-and-forget side effect of localStorage writes. Per the persistence design, OPFS should be its own backend composed via `tee(localStorage, opfs)`. Split into `persistence/localstorage.ts` + `persistence/opfs.ts`, wire with `tee()` in `platform/index.ts`. Twine platform (`runtime/twine/ts/platform/persistence.ts`) is localStorage-only and will benefit from the same split.

- [ ] **Cloud save backends** — Platform persistence interface already abstracts save/load/remove; swapping the backend is just a different platform implementation. Candidates: OneDrive, Google Drive, Dropbox, S3/R2/B2. Design: `platform/onedrive.ts`, `platform/gdrive.ts`, etc., each re-exporting the same persistence interface. Config: deployer switches backend by changing re-export in `platform/index.ts`.

- [x] **IR-level closure representation** — `Op::MakeClosure { func, captures }`, `CaptureMode::ByValue`/`ByRef`, `CaptureParam`, and `add_capture_params` on `FunctionBuilder` are all implemented. SugarCube, Harlowe, and GML frontends emit `MakeClosure` with explicit capture lists. TypeScript backend rewrites to IIFE-with-captures (by value) or plain arrow (no captures). DCE tracks captures as uses. `<<capture>>` is a correct no-op — our IIFE pattern already snapshots by value, making SugarCube's workaround unnecessary. Remaining gaps: (1) Flash closures still use `MethodKind::Closure` + TS lexical closure rather than `Op::MakeClosure` — see "Inline closures" below; (2) `CaptureMode::ByRef` is defined but unused.
- [x] **GML default argument recovery pass** — `GmlDefaultArgRecovery` detects the GMS2.3+
  `if (arg === self.undefined) arg = default` IR pattern and folds constant defaults into
  `FunctionSig.defaults`. Also sets type-matched defaults for variadic script params
  (post-inference, reads narrowed types from `value_types`): `""` for string, `false` for
  bool, `0` for number. Dead Estate TS2554: 2069→859, TS2555: 149→0. Bounty TS2555: 251→0.
- [x] **GML param type inference gaps — cross-function call-site narrowing** — 76% of GML
  function `argumentN` params remain `: any` after inference. Dead Estate: 1407/1855 `any`.
  Bounty: 53/77 `any`. Root cause: ConstraintSolve runs per-function and only propagates
  callee param types → caller arg types, never the reverse. Call sites are the biggest
  untapped source of type info.
  **Implemented then deleted**: `CallSiteTypeFlow` pass implemented (committed), then
  deleted in Phase 7 (commit 485325d, 2026-03-27) as part of the HM solver replacement.
  The HM solver was expected to subsume this — see "Retire CallSiteTypeFlow" (Step 4 in
  the HM plan above). As of Phase 7, 2245 TS18046 ("'argument0' is of type 'unknown'")
  errors remain — see below.
- [x] **HM solver: Alloc handler aliased frontend TypeVarIds — root cause of 2245 TS18046
  errors** — FIXED in `constraint_collect.rs` (`Op::Alloc` handler, 2026-03-29).
  Root cause: the `Op::Alloc(Type::Var(id))` branch reused the frontend-local `TypeVarId`
  directly as the cell TypeVar in the shared arena. Frontend TypeVarIds are assigned by a
  per-function counter (0, 1, 2, ...) in `build_signature_with_args`; they have no identity
  in the global arena. Reusing `TypeVarId(5)` meant the cell aliased the 5th TypeVar
  allocated in the very first function's `collect_function` run — an entirely unrelated value.
  This corrupted Store/Load inference chains across all functions that used Alloc with a
  `Type::Var` inner type (i.e., all GML locals/params initialized with TypeVars). Fix:
  always call `arena.fresh()` for the cell TypeVar; emit `Equal(cell, inner_ty)` only when
  `inner_ty` is concrete. Result: 2245 → 1953 TS18046, 5959 → 4588 total Dead Estate errors.
  Remaining 1953 TS18046 errors are: loop counters (`i`, `j`, `yy`, `xx`) whose arithmetic
  chains use `builtin.add_any`/`sub_any` (typed from the first operand, which is Unknown when
  built), functions with no direct call sites (only `CallIndirect`), and params used as
  collections (correctly guarded from narrowing).
- [ ] **CallSiteTypeFlow: union type narrowing (intentionally deferred)** — When callers
  disagree on types (e.g. one passes `string`, another `number`), the param stays `Dynamic`
  rather than being narrowed to `string | number`. Adding union narrowing would require
  downstream work in ConstraintSolve (union unification) and emitter (union type annotations).
  Deferred indefinitely — the single-type narrowing covers the majority of cases where all
  callers agree, and disagreement genuinely suggests `any` is appropriate.
- [ ] **GML fixpoint + CallSiteTypeFlow: instance ID narrowing causes TS2339 spike** —
  With `--fixpoint` enabled on Dead Estate, TS2339 "Property does not exist" errors jump from
  44 (baseline) to 217. Root cause: GML instance IDs are `Float(64)` at the call site (returned
  by `instance_create`, `instance_find`, etc.), so fixpoint propagates `Float(64)` → `number`
  into callee `self` params via ConstraintSolve. The callee then accesses `self.field` directly,
  which TypeScript rejects on `number`. `run_once` on CallSiteTypeFlow helps slightly (217 → 217,
  same pattern) but the bulk comes from ConstraintSolve re-propagating the first round's
  narrowing in subsequent iterations. Two potential fixes:
  1. CallSiteTypeFlow: skip narrowing `Float(64)` → `Float(64)` for GML (instance IDs are
     semantically opaque handles, not plain numbers). But this would block all float narrowing.
  2. Deeper fix: GML instance IDs need a distinct IR type (e.g. `InstanceId`) separate from
     `Float(64)`, so they don't conflate "this is a float" with "this is an object handle".
  The remaining 16 new TS2339s are `string.push` errors — array params narrowed to `string`
  from a control-flow path that initialises as a string. Likely game author bugs surfaced by
  narrowing.
- [ ] **Derive `function_signatures` from `runtime.ts` at build time** — `runtime.json` function
  signatures are manually maintained and drift from `runtime.ts` as the source of truth. The fix:
  add a build step (e.g. `bun` script or `build.rs`) that parses `runtime.ts` with `oxc` or
  `ts-morph`, extracts all public method names + parameter/return types, and generates the
  `function_signatures` section of `runtime.json` automatically. The manual entries then become
  the generated baseline; overrides for special cases (e.g. `classref` params) can be a separate
  `function_signature_overrides` map. This eliminates the gap where new runtime methods are typed
  Dynamic until someone notices.

- [ ] **Frontend-controlled pass ordering** — `extra_passes` are currently appended after the
  entire default pipeline. Frontends should be able to specify where their passes run (e.g.
  "after constraint-solve but before mem2reg"). Current approach works for IntToBoolPromotion
  and GmlLogicalOpNormalize which are fine running last, but won't scale.
- [ ] **LÖVE/love2d backend** (emit Lua files targeting LÖVE 2D framework) — HIGH priority after
  getting CC gameplay working. LÖVE provides game loop, graphics, audio, input — maps well to
  GameMaker's runtime model. Lua is dynamically typed so many IR→TS pain points (union types,
  `any` widening) vanish. Good forcing function for second-backend readiness. Would need:
  `reincarnate-backend-lua` crate, Lua AST types, LÖVE-specific scaffold (`main.lua`, `conf.lua`),
  runtime package in `runtime/gamemaker/lua/` (or shared). Key design question: does the runtime
  port as Lua modules, or do we emit self-contained Lua with inlined runtime?

- [ ] **Twine multi-backend rendering strategy** — Twine output is HTML-centric (DOM nodes, CSS
  styling, `<tw-link>` elements). Web backend (TypeScript) handles this natively. Non-web backends
  (LÖVE, native) need a different approach. Options:
  1. IR captures passage structure semantically (text, link, styled-span, conditional) — each
     backend decides rendering. Harlowe already works this way (hooks are IR nodes). SugarCube's
     wikifier mode processes HTML at runtime — inherently needs an HTML engine.
  2. For non-web: strip HTML, emit plain text + structured menu choices. Loses visual formatting
     but preserves interactivity and story logic.
  3. Embed lightweight markup renderer per platform (e.g., LÖVE text layout).
  Design work needed before implementing a non-web Twine backend.

- [ ] Rust codegen backend (emit `.rs` files from typed IR — **blocked on multi-typed locals**)
- [ ] wgpu + winit renderer system implementation
- [ ] Web Audio system implementation
- [ ] Native binary decompilation (C/C++, DirectX games with no scripting layer) — requires disassembly + decompilation pipeline rather than bytecode decoding; far harder than any current target; no timeline

## CLI — `reincarnate check` (language-agnostic output validation)

- [x] **`reincarnate check` subcommand** — Implemented: Checker trait in core, TsChecker crate
  (tsgo via bunx), CLI wiring with `--no-emit`, `--json`, `--all` flags. Supports registry names,
  paths, and bare positional args. Prints per-code and per-file breakdown.
- [x] **`--baseline <file>` for check** — `--save-baseline <path>` saves check results as JSON;
  `--baseline <path>` compares against it and reports per-code/per-file deltas. Exits non-zero on regression.

## Diagnostics

- [ ] **External type member validation** — 90 member warnings from types
  inheriting Flash stdlib classes. Need structured member metadata from runtime
  type definitions to validate these.

### Discarded AVM2 Metadata

The Flash frontend discards several categories of ABC metadata that could
improve output fidelity:

- [ ] **Exception handler metadata** — `from`/`to` byte offsets,
  `variable_name`, `type_name`. No try/catch in IR yet.
- [ ] **Class flags** — `is_sealed`, `is_final`.
- [ ] **Protected namespace** — Per-class protected namespace for `protected`
  member visibility.
- [ ] **Trait metadata annotations** — `[Embed]`, `[Bindable]`, custom.
- [ ] **Trait `is_override` / `is_final` flags** — `override` keyword in TS.
- [ ] **Slot/dispatch IDs** — AVM2 vtable layout, irrelevant to decompilation.
- [ ] **`DebugLine` source line info** — Could emit `// line N` or source maps.

## Flash Output Quality

### Correctness

- [ ] **Complex loop decompilation** — Some while-loop bodies have unreachable
  code after `continue`, wrong variable assignments.

### Optimizations — Safe (no semantic change)

- [x] **Redundant type casts** — `strip_redundant_casts` AST pass eliminates
  `as number` when VarDecl/param type already matches. 584 → 108 in Flash.
  Remaining 108 are field accesses where field types aren't tracked.
- [ ] **Constant `rand(n)` where n <= 1** — `rand(1)` always returns 0. Only 1
  known instance (PhoukaScene).
- [ ] **Dead store elimination** — Remove assignments whose values are never
  read. Requires liveness analysis.
- [ ] **Condition inversion** — Structurizer sometimes inverts conditions.
  Heuristic to match original branch polarity.

### Optimizations — Requires alias/purity analysis

- [ ] **Cross-side-effect const sinking** — Sink `const v = expr` past
  side-effecting statements when `expr` is provably pure and unaliased.
- [ ] **Method reference inlining** — `const v = this.method; ... v(args)` →
  `this.method(args)`. Only safe if `method` is not a getter.
- [ ] **Field read deduplication** — `this.x` read twice → read once, reuse.

### Optimizations — Requires control flow analysis

- [ ] **Inline closures** — Some Flash closures still fall back to
  `this.$closureN` field references when `compile_closures()` fails to
  compile them (e.g. dynamic features). These should be diagnosed and fixed
  case-by-case. Twine closures are now fully inlined as of `26ecc6a`.
- [x] **Loop variable promotion** — Fixed `match_compound_assign` and
  `is_var_update` to look through `AsType` casts. Flash: ~65 additional
  while→for promotions. Remaining while-loops use class fields, parameters,
  pre-increment patterns, or complex multi-step increments.

## GameMaker — Version-Gating Audit (HIGH PRIORITY)

The GML frontend does not pass `BytecodeVersion` to any translator, decoder, or rewrite.
Every behavior runs unconditionally regardless of whether the game is GMS1, GMS2, or GMS2.3+.
Many behaviors are version-specific and applying them to the wrong version can produce silent
wrong output. The `BytecodeVersion` is already extracted from GEN8 and available on `DataWin`
(`dw.bytecode_version()`). It needs to be threaded into `TranslateCtx` and all relevant code
paths that make version-sensitive decisions.

**What to audit** — every file under `crates/frontends/reincarnate-frontend-gamemaker/src/`:

- [ ] **`lib.rs`** — `gml_GlobalScript_` skip (added for GMS2.3+ migration pattern; safe for
  GMS1/GMS2 since those games don't have `gml_GlobalScript_*` CODE entries in SCPT, but should
  be documented and guarded with a version assertion). Also: FUNC chunk translation, GLOB chunk
  translation, `scan_code_refs` — all may need version guards.

- [ ] **`decode.rs`** — The `DataType` and `OpCode` decoding may differ between v14 and v15+
  instruction formats — confirm `has_new_instruction_format` is respected.

- [x] **`translate.rs`** — Partial. Completed 2026-02-28:
  - [x] `bytecode_version: BytecodeVersion` field added to `TranslateCtx`; threaded through all
    4 construction sites (3 in lib.rs, 1 in object.rs) plus the with-body inner context.
  - [x] `is_gms23_plus()` added to `BytecodeVersion` (bc >= 17).
  - [x] Break signal -10 (chknullish, 0xFFF6): version guard added — logs warning if seen on
    bytecode_version < 17.
  - [x] Break signal -11 (pushref, 0xFFF5): version guard added — logs warning if seen on
    bytecode_version < 17.
  - [x] Dup swap-mode encoding (`dup_extra != 0`): comment added explaining it is safe
    unconditionally because older versions always have high byte == 0.
  - Remaining items still open:
  - [ ] `filter_reachable` — only needed for GMS2.3+ shared CODE entries
  - [ ] `scan_body_argument_indices` + `argument` captures in with-body — may not apply to GMS1
  - [ ] `InstanceType::Stacktop` (-9) as struct method self-reference — GMS2.3+ construct
  - [ ] `args_count & 0x7FFF` masking — 0x8000 flag meaning differs between versions
  - [ ] Negative instance IDs below -9 (e.g. -16 for `Arg`) — confirm range is version-stable

- [ ] **`object.rs`** — Event type encoding may differ between GMS1 and GMS2; object/event
  structure differences (e.g. `persistent`, `visible` fields, parent indices).

- [ ] **`data.rs`** — Sprite/texture/audio asset structures differ between versions. TXTR
  external textures (GMS2.3+), SEQN/TAGS/ACRV/FEDS chunks — guard against parsing these on
  older versions.

**Action**: Add `bytecode_version: BytecodeVersion` to `TranslateCtx`; add version-check
helper methods (`is_gms23_plus()`) to `BytecodeVersion`; replace any implicit version
assumptions with explicit version guards. Log a warning when a GMS2.3+ feature is detected
on a game that reports an older version.

## GameMaker — New Game Failures (discovered 2026-02-22)

Batch-emitting 7 new games from the Steam library exposed 4 distinct bugs:

### 1. `argument` inside `with`-body panics — blocks 10SecNinjaX, 12BetterThan6, VA-11 HALL-A

- [x] **`argument[N]` accessed inside a `with`-body closes over wrong param index** — **FIXED**.
  Guards added to `translate_push_variable` and `translate_pop` for both the named (`argument0`)
  and stacktop (`argument[N]`) forms: check `locals` map for `_argumentN` capture first (with-body
  case), then bounds-check before calling `fb.param()`. Unblocked 10SecNinjaX, 12BetterThan6.

### 2. TXTR external textures panic — blocks Downwell

- [x] **`txtr.rs:102` slice-end underflow when textures are stored externally** — **FIXED**.
  `texture_data()` now returns `None` when `end < start || end > data.len()`. Downwell now emits
  (remaining errors are runtime API gaps, not parse bugs).

### 3. PE-embedded `data.win` not supported — blocks Momodora RUtM

- [x] **Reader requires FORM at offset 0, but Momodora embeds it in a PE exe** — **FIXED**.
  `DataWin::parse` detects MZ magic and scans all FORM occurrences for the first one whose
  declared size fits within the file (avoids false positives in PE sections). Also fixed 0-size
  CODE/VARI/FUNC chunks for YYC-compiled games (early-return empty structs). Momodora now emits.

### 4. Forager parse error at EOF / Risk of Rain CODE chunk empty

- [ ] **Forager `game.unx` hits unexpected EOF while parsing** — the reader reaches absolute
  file offset 81446624 (= EOF) and attempts a 4-byte read. All top-level chunks parse correctly;
  the failure is inside a chunk's content parser. Likely the CODE chunk bytecode decoder reading
  a function entry whose stated length extends to exactly EOF, then attempting to read past.
  Needs `--dump-ir` + targeted investigation.

- [x] **Risk of Rain `game.unx` has empty CODE/VARI/FUNC chunks (YYC-compiled)** — **FIXED**
  by the same early-return guards added for Momodora (Bug 3). Both GMS1 and GMS2 YYC games now
  parse correctly with empty bytecode chunks.

### 5. Sprite name bracket notation missing for access side — blocks Nubby's, Mindwave, MaxManos2

- [x] **`Sprites.3DPegBase` emitted instead of `Sprites["3DPegBase"]`** — **FIXED** in commit
  `9e1b5d7`. Both `resolve_sprite_constant` (emit.rs) and `try_resolve_sprite_assign` (rewrites/
  gamemaker.rs) apply `is_valid_js_ident` and use bracket notation when false. Remaining errors
  in Nubby's/MINDWAVE/MaxManos2 are Bug 7 (struct field access on Number) and runtime API gaps.

### 6. pushac/popaf array capture coerces array to int — Schism syntax errors

- [ ] **`int(argument0)[FxDoomApply.gunMod] = 0` produces TS1005 syntax error** — `pushac`
  captures `int(argument0)` as the "array reference", but `argument0` is actually an array
  passed by reference. The `coerce v1, i32` from a preceding `Push Variable(argument0,
  type=Int32)` converts the array to an integer before `pushac` saves it. `popaf` then calls
  `set_index(int_value, array_value, 0)` which the TS printer emits as `int(argument0)[array]
  = 0` — invalid JS. Root cause: type mismatch between the Int32 push type and the actual
  array type of argument0. Fix requires either: (a) not coercing when the value is used as a
  pushac target, or (b) the TS printer detecting integer-as-collection in SetIndex and routing
  to a GameMaker.setIndex runtime call. Only 6 errors in Schism, low priority.

### 7. Dead Estate / GML open bugs

- [ ] **Inject `+expr` for Bool operands in GML arithmetic (TS2362/TS2365)**
  In `rewrites/gamemaker.rs`, detect `JsExpr::BinOp` nodes where one operand has a `Bool`-typed IR value
  and the operator is arithmetic (`+`, `-`, `*`, `/`, `%`, etc.). Wrap the Bool operand with a unary
  `+` expression (`JsExpr::UnaryOp("+", expr)`). This is a TypeScript-emission-layer fix only — no IR
  changes needed. Must check that the unary `+` is inserted at the correct sub-expression (the Bool
  operand, not the whole expression).

- [ ] **GmlLogicalOpNormalize false positive (3 instances: OAdultLevi:98, MainMenu:381, _init.ts:3428)**:
  else_target ≠ merge_target so the guard doesn't fire, but the pattern is still an if-then-else
  not `||`. The fundamental problem: GmlLogicalOpNormalize runs AFTER TypeInference, so replacing
  a block arg changes the arg type without updating `value_types` for the block param. Refreshing
  `value_types` fixes the declaration but then passing `boolean | number` to `number` params causes
  TS2345 regressions (net +1 error). Needs a smarter guard OR fixing at the TypeInference level
  (re-run TypeInference after extra passes, or propagate type changes through the pass).

- [ ] **Direct bool→number field/variable assignment (3 instances: PassageGate:27, NewCharacterSelect:551, ParentMenu:204)**:
  Game author assigns a comparison result to a `number`-typed field (e.g. `image_index = y > height/2`).
  This is a GML idiom (comparison gives 0/1). Our `image_index` struct field is declared `number` in the
  runtime types. Fix would require widening affected field types to `number | boolean` in the runtime,
  or accepting these 3 errors as intended diagnostics (game author using bool as int).

- [ ] **TS2367: `return value` inside `with` block (5 errors)** — Our `withInstances` callback
  model doesn't propagate return values — the function is typed as `void` even though it should return
  a number. Correct fix: detect exit PopEnv within body_insts (Branch offset ≈ -4194304 sentinel).
  Truncate body_insts there; keep post-break code in the outer function. Requires the outer function
  to branch to the post-break code after `withInstances` call (needs block map entries for post-break
  offsets). Complex because the post-break code must also return the value captured in the local.

- [ ] **TS2554: GML loose calling convention (5 errors)** — `loadSetting`, `getScreenType`,
  `getPiecesWidth`, `getPiecesHeight`, `drawTextPieces` called with more args than their signatures
  declare. GML extra args accessible via `argument[N]`. Fix: `CallSiteArityWiden` IR transform pass
  — scan all `Op::Call` sites; if any passes more args than the declared param count, append
  `argumentN: T = default` params to the `FunctionSig`.

- [ ] **Unreachable code after return/continue in switch cases (TS7027)** — The switch-case emitter
  emits both a `continue`/`return` AND a trailing `break` for the same case. Fix: suppress the
  auto-generated `break` when the case body already ends with `return`, `continue`, or `break`.

- [ ] **Undeclared variables — `vNNN` and named vars (TS2304)** — Three sub-patterns:
  1. SSA register names in ternary chains — structurizer's ternary/select lowering doesn't ensure
     all referenced values have declarations in the enclosing scope.
  2. `spd` in with-body (Barnacle.ts:142-143) — linearizer or translator lost the assignment target.
  3. `pass` in controller objects (OAnya*Controller.ts) — same category as `spd`.
  All are correctness bugs — emitted code references variables that don't exist.

- [ ] **TS7053: `int(x)[field]` indexing number** — 2D array access patterns where the array variable
  is coerced to `int` before being used as an array target. The `coerce i32` instruction wraps the
  array in `int()` when it should access the array directly. Fix in GML translator or as a rewrite pass.

- [ ] **TS7053: struct field access on number-typed vars** — `v['fieldName']` where `v` is typed as
  `number` rather than `any`. Root cause: instance variables that hold object references (e.g. created
  by `instance_create_depth`) are typed as `number` (the raw instance ID) rather than as the class type.
  Fix requires GML instance ID type propagation.

- [ ] **TS2304: `dynamic` identifier (Dead Estate)** — GML `dynamic` keyword used as an identifier
  in GML 2.3+ struct definitions. Translator emits `dynamic` as a bare identifier which TypeScript
  doesn't recognize. Needs investigation — may be a translator keyword lookup bug.

- [ ] **TS2345: OBJT class constructors passed to user scripts typed as `number`** — Game-author
  pattern: user scripts inferred `argument0: number` from arithmetic context now receive `typeof
  ClassName` (class constructor). Future: CallSiteTypeFlow could propagate `typeof GMLObject` into
  user script params.

- [ ] **TS2339: Missing particle system properties** — `part_system_depth` and
  `part_system_automatic_draw` not declared on the particle system type. Need to add to GMLObject
  type definition or particle system type.

- [ ] **TS2552: `sarr` in with-body** — `sarr.length` on `_init.ts:6198` should be `self.sarr.length`.
  Adjacent references correctly use `self.sarr[...]` — one reference dropped the `self.` prefix.
  With-body codegen bug; needs investigation in GML translator.

- [ ] **TS2308/TS2300/TS2440: struct constructor / object class name collision in barrel** —
  `_init.ts` exports `function TextEffect(...)` (GML struct ctor) and `objects/TextEffect.ts` exports
  `class TextEffect extends GMLObject`. Same for `___struct___127/128`. Fix: when a struct ctor name
  collides with an object class of the same name, skip emitting the struct ctor as a separate export.

- [ ] **`instance_type_flow.rs` line 165: `Ne` TypeCheck case not handled.**
  The `Ne` comparison falls through to the same `TypeCheck` rewrite as `Eq`, which is wrong —
  `Ne` should produce a `Not(TypeCheck(...))`. Currently any `obj != SomeClass` check gets the
  same type narrowing as `obj == SomeClass`, yielding wrong narrowed types in the else branch.
  Untracked `// TODO: handle Ne case properly.` in source.

- [ ] **TS2345/TS2322/TS2365 (~80 errors): GmlInstanceTypeFlow arithmetic widening** —
  When a class constructor (`Struct(ClassName)` / `typeof ClassName`) participates in arithmetic
  (Add, Sub, Mul, Div, etc.), the result type should NOT inherit the constructor type. The game uses
  GML object indices as plain integers in arithmetic (e.g. `return Game - self.cameraX`, or
  field `creatorObject` narrowed to `typeof Player` then used in comparisons). Fix options:
  (a) In GmlInstanceTypeFlow, don't narrow fields/variables to class constructor types when they
  appear in arithmetic operand positions — use `Dynamic` instead. (b) In type inference, when
  an arithmetic op has a `Struct` or class-ref operand, widen the result to `Dynamic`/`Float`.
  Also affects return type inference: `getUIMouseX` returns `typeof Game` instead of `number`.
  **Attempted 2026-03-12:** Options (a) and (b) both don't help — the Struct types propagate
  through field accesses and assignments, not direct arithmetic operands of `instance_create`.
  The real issue is that GML object IDs ARE integers that happen to be narrowed to class types
  by CallSiteTypeFlow and ConstraintSolve, then passed to functions expecting `number`. Needs
  a deeper approach: possibly widen Struct→Dynamic in the backend when a Struct-typed value
  is passed to a `number`-typed parameter, or add `as any` casts at call sites.

### 8. RunLoader::step stack underflow on Popz at 0x19c — Dead Estate translation error

- [ ] **`compute_block_stack_depths` uses `or_insert` — first path to reach a join point wins** —
  `RunLoader::step` fails with `0x19c: stack underflow on Popz`. The depth pre-computation walk
  sets `terminated = true` on `Bt`/`Bf`, which is correct since fall-through is always a block
  start. But `or_insert` means if two converging paths have different stack depths, only the first
  recorded depth is used. If the true runtime path arrives with fewer items than the recorded depth,
  subsequent pops underflow. Investigation needs bytecode dump of RunLoader::step to see which
  join point has inconsistent depths. Consider: (a) asserting stack depth consistency at join
  points, (b) using a worklist-based depth propagation instead of linear walk.

### 10. GMS1 integer object indices not resolved to class references — FIXED 2026-03-09

**Fixed.** Bounty TS2345 errors dropped from 48 → 13 (all TS2345 eliminated; 13 remaining are
pre-existing game-author type errors: TS2322/TS2362/TS2365/TS2366). Dead Estate also improved
from 248 → 186 (GMS2.3+ games sometimes use integer literals for object indices too).

**Secondary fix:** `parse_type_notation("classref")` in `ir/ty.rs` now maps to `Type::Dynamic`
(not the default `Type::Struct("classref")`). The `"classref"` string in runtime.json param types
was leaking into IR via `ConstraintSolve`'s `external_function_sigs` table → `parse_type_notation`
→ `Type::Struct("classref")` → callee params typed as `classref` → `ts_type` rendering as the TS
type annotation `classref` (e.g. `argument0: classref = 0.0`). Fix: treat `"classref"` as a
backend-only annotation, mapping to `Dynamic` at IR level.

**Root cause:** In GMS1, object type names are compile-time integer constants. `Push Int32(4)` +
`Conv v->v` creates `Cast(Const(Int(4)), Dynamic)` in the IR — no ClassRef type. Backend rewrite
had no mechanism to resolve these to class names.

**Fix (initial, 2026-03-09):** Backend rewrite pass `resolve_classref_args` in `rewrites/gamemaker.rs`
ran after `coerce_bool_args` for both free functions and class methods, replacing `JsExpr::Literal(Int(N))`
or `JsExpr::Cast { Literal(Int(N)), Dynamic }` at classref positions with `JsExpr::Var(object_names[N])`.
Import scanner in `collect_type_refs_from_function` extended with `cast_const_ints` map and `Op::Call`
handler to register newly-introduced class names.

**Architectural fix (2026-03-09):** Replaced backend rewrite with `GmlClassRefResolve` IR transform
pass in `crates/frontends/reincarnate-frontend-gamemaker/src/classref_resolve.rs`. The IR pass inserts
`Op::GlobalRef(name)` instructions typed `Type::ClassRef(name)` at classref-typed argument positions,
so every backend sees already-resolved class references in the IR. Backend `resolve_classref_args`,
`cast_const_ints` map, and associated import-scanner code removed from TypeScript backend.

**runtime.json changes:** Updated 31 functions with `"classref"` param type:
- instance_create/depth/layer (param[2]/[3])
- instance_number/find/change/nearest/furthest (param[0]/[0]/[0]/[2]/[2])
- instance_exists/activate_object/deactivate_object
- object_is_ancestor/get_name/exists/get_sprite/get_parent
- distance_to_object, place_meeting/position_meeting
- instance_place/position/place_list/position_list
- collision_point/rectangle/line/circle/ellipse + *_list variants
- mp_grid_add_instances

### 9. Pushref type_tag mapping is version-dependent — wrong for pre-2024.4 games

`build_asset_ref_names` currently uses the GM 2024.4+ type_tag layout. Pre-2024.4 games use a
different mapping where types 4–13 are shuffled (e.g. 4=Background, 6=Script, 8=Timeline,
10=Shader). Types 0–3 (Object, Sprite, Sound, Room) are the same in both versions, so games
using only those assets are unaffected. Need:
- [ ] Version detection (check `IsVersionAtLeast(2024, 4)` equivalent — likely in GEN8 chunk)
- [ ] Dual mapping in `build_asset_ref_names` based on detected version
- [ ] Same dual mapping in `generate_asset_ids`

Reference: UndertaleModTool `AdaptAssetType` / `AdaptAssetTypeId` in `UndertaleCode.cs`.

### New game inventory

| Game | Source | Status |
|------|--------|--------|
| 10 Second Ninja X | `data.win` 134MB | ⚠️ 272 TS errors (2026-03-14) |
| 12 is Better Than 6 | `game.unx` 179MB | ⚠️ 21 TS errors (2026-03-14, was panicking) |
| Cauldron | `data.win` 169MB | ❌ YYC |
| CookServeDelicious2 | `game.unx` 805MB | ❌ EOF parse error in CODE (same as Forager) |
| Dead Estate | `data.win` 192MB | ⚠️ 5 TS errors + 1 translation error (2026-03-14) |
| Downwell | `data.win` 27MB | ❌ TXTR external textures |
| Forager | `game.unx` 78MB | ❌ EOF parse error in CODE |
| Just Hit The Button | `data.win` 1MB | ⚠️ 2 TS errors (2026-03-14) |
| Max Manos | `data.win` 47MB | ⚠️ 6 TS errors (2026-03-14) |
| Max Manos 2 | `data.win` 10MB | ⚠️ 30 TS errors (tilemap/skeleton stubs, vNN scoping) |
| MINDWAVE Demo | `data.win` 324MB | ⚠️ ~26k TS errors (runtime API gaps) |
| Momodora RUtM | `.exe` 36MB | ❌ PE-embedded FORM |
| Nova Drift | `data.win` 415MB | ❌ YYC |
| Nubby's Number Factory | `data.win` 66MB | ⚠️ ~77k TS errors (runtime API gaps) |
| Risk of Rain | `game.unx` 34MB | ❌ YYC (empty CODE chunk) |
| Rocket Rats | `data.win` 2MB | ❌ YYC |
| Schism | `data.win` 77MB | ⚠️ 367 TS errors (2026-03-14, was 793) |
| Shelldiver | `data.win` 2MB | ❌ YYC |
| Soulknight Survivor | `data.win` 35MB | ❌ YYC |
| Undertale | `data.win` 5MB | ⚠️ 8 TS errors (6 unreachable + 2 game-author — all wontfix) |
| VA-11 HALL-A | `game.unx` 212MB | ⚠️ 5 TS errors (2026-03-15, was 56; −51 FS_* extension stubs fixed) |

---

## GameMaker — Remaining TS Error Categories (compiler-level)

After runtime function coverage is comprehensive, remaining errors fall into these categories.
These need compiler/emitter/IR fixes, not runtime additions.

### 1. GML auto-coercion (TS2345, TS2322) — partially fixed, ~20 remaining across games
GML silently coerces between types: `bool→number`, `number→string`, `string→number`,
`GMLObject→number` (instance ID). The emitted TypeScript has strict types, so these produce
TS2345 "argument not assignable" errors.

**Status**: Phase 4b in `GmlBoolArithCoerce` handles call-site coercion using both internal
(`callee_param_types`) and external (`external_function_sigs`) param types. Session 26 added
`peel_dynamic_coerce` to look through `coerce(val, dyn)` wrappers that hid the real arg type.
Also fixed `Cast(x, Int(64), Coerce)` emission — was falling through to passthrough instead
of emitting `Number(x)`.

**Remaining issues**:
- **GMLObject→number** (instance ID): `this.id` is now typed `GMLObject` (commit `91e327aa`).
  However, zero new numeric-id-misuse errors surfaced because numeric uses of `id` flow through
  `unknown`-typed intermediates — TypeScript never sees comparisons like `GMLObject === -4`.
  A dedicated `InstanceId` type is still needed for full static verification; see open threads.
- **Script calls with wrong arg types**: `SpriteFromName(rt, self, 531)` where param is `string`
  but caller passes number. ~6 errors in 10SecNinjaX.

**Key discoveries (session 26)**:
- **`coerce(val, dyn)` hides real types**: GML translator wraps most call args in
  `coerce(val, dyn)` before passing. The auto-coercion pass saw `dyn` and skipped.
  Fix: `peel_dynamic_coerce()` looks through the wrapper. This fixed ~21 errors across games.
- **`Int(64)` cast emission gap**: `Cast(x, Int(64), Coerce)` in the IR fell through to the
  catch-all passthrough in `ast_printer.rs` (only `Float(_)` and `Int(32)` had explicit arms).
  Added `Int(64) → Number(x)` arm.
- [x] **`action_if_variable` runtime signature wrong** (2026-03-15): Runtime took `name: string`
  and did `this._self[name]` lookup, but GML DnD compiles to push the variable VALUE directly.
  Fixed runtime to take `(variable: unknown, value: unknown, op: number)` with full 6-op switch
  (equal/less/greater/notEqual/lteq/gteq). 10SecNinjaX: ~60 → 35 errors (−25).

### 2. Dangling SSA references (TS2304 `v*` variables) — ~25 errors in Schism/MaxManos2
Variables like `v17`, `v23`, `v48` appear as TS2304 "Cannot find name". Investigation
(MaxManos2 `funcGAMEenemiesAndObjectsCreate`) shows these are **dangling SSA references**:
values used in block-terminator arguments (e.g. `switch v482, [..., "pentagonFloor" ->
block33(v48), ...]`) but with NO definition instruction anywhere in the IR function.
This is a GML frontend translation bug — the translator created a reference to a value
that was only defined in a different function's scope (or not at all).

**Fix**: Audit `translate_push_variable` / GML bytecode Br/BrIf/Switch emission in the
frontend to ensure all block-argument values have definitions in the current function.
The `v48` in MaxManos2 appears to be a `coerce(1, dyn)` that should be a direct constant,
suggesting the front end lost track of the value's origin when building block args.

### 3. Instance ID as number (TS7053) — ~32 errors in Schism, ~2 in Dead Estate
GML instance IDs are numbers that can be used to index into instance fields via
`getInstanceField`. When type inference narrows a variable to `number` (from an instance ID
return), using it as `arr[id]` produces TS7053 "can't index type Number".

**Fix**: Track instance ID types separately from plain numbers. Requires either a dedicated
`InstanceId` type in the IR or widening to `any` when a value flows through both
instance-ID and number contexts.

### 4. Duplicate identifiers (TS2300, TS1117) — ~2 errors in 12BetterThan6, ~2 in 10SecNinjaX
Object literal duplicate keys from name collisions in the emitter.

**TS2300 FIXED 2026-03-15**: `emit_class_file`, `generate_main`, and `emit_module_imports`
in the TypeScript backend now apply collision-suffix logic (_2, _3, …) when two GML objects
in the same namespace share the same sanitized name (e.g., a game OBJT chunk with duplicate
object names like `TOTCLeaderboard`×2 in 10SecNinjaX, `OFloor2`×2 in 12BetterThan6).

**TS1117 remaining**: `data/objects.ts` object literal duplicate keys — different root cause.
Fix: audit GML object-type data generation for duplicate keys.

### 5. Extension function auto-stubbing — FIXED 2026-03-15 (`ce7706a`)
Parsed EXTN chunk, extracted function names and signatures, generated throw-stubs as IR
free functions that call `extension_stubfunc_real/string` (which throw at runtime).
VA-11 HALL-A: 56 → 5 errors (−51 TS2304). Remaining 5 are pre-existing wontfix bugs.
Schism also benefits (NSP_* extension functions will now be stubbed).

### 6. Unresolved function pointers (TS2304 `func_ref_unknown_*`) — ~165 in Schism
Obfuscated games have function references that can't be resolved to known functions.
These are `func_ref_unknown_0x...` identifiers.

**Fix**: Better function pointer resolution in the GML translator, or generate stub
declarations for unresolved references.

### 7. Read-only property assignment (TS2540) — FIXED
Room setter added to GMLObject (commit `b825e00`).

---

## GameMaker — Runtime Platform Layer (HIGH PRIORITY)

The GameMaker runtime has several major API families that need platform-layer implementations:

### Audio (`platform/audio.ts`)
All `audio_*` functions are currently unimplemented and throw. Audio belongs in the platform
layer per the three-layer architecture. Needs:
- `platform/audio.ts` — Web Audio API implementation (AudioContext, AudioBufferSourceNode)
- Wire audio asset loading from `GameConfig` (asset table → audio buffer)
- `GameRuntime.audio_play_sound` → delegates to platform audio
- `GameRuntime.audio_is_playing` / `audio_stop_sound` / `audio_stop_all` / etc.

### Surfaces (`platform/surface.ts` or in draw layer)
`surface_*` / `draw_surface*` require off-screen rendering via WebGL or OffscreenCanvas.
Currently throw. Dead Estate uses surfaces extensively for post-processing effects.

### Shaders / GPU State
`shader_*` / `gpu_*` require WebGL shader compilation and state management. Currently throw.
Dead Estate uses shaders for visual effects (fog, color grading, etc.).

### Particle System
`part_*` require a real particle simulation system. Currently throw.
Dead Estate uses particles for effects (dust, sparks, etc.).

### GML Simulation Correctness Bugs (found 2026-03-09 audit)

**CRITICAL — `room_speed` completely ignored:**
The game loop calls `requestAnimationFrame` unconditionally every vsync frame. GML's `room_speed`
declares target steps/second (commonly 30, 60, 120). A game with `room_speed = 30` on a 60Hz
monitor runs 2× too fast. All per-step logic (alarms, physics, `xprevious`/`yprevious`) is
affected. Fix: accumulate elapsed time and step N times per frame to match `room_speed`.

- [ ] **Implement `room_speed`-based step accumulator in `_runFrame()`**

**CRITICAL — Built-in instance physics missing entirely:**
GML's built-in motion model (`speed`, `direction`, `hspeed`, `vspeed`, `friction`, `gravity`,
`gravity_direction`) is applied to `x`/`y` each step by the runner. None of these properties
exist on `GMLObject` and the step loop doesn't apply them. Games using built-in motion are
fully stationary.

- [ ] **Add `speed`, `direction`, `hspeed`, `vspeed`, `friction`, `gravity`, `gravity_direction`, `image_speed`, `image_angle` to `GMLObject`**
- [ ] **Apply built-in motion in the step loop** (before step event): apply gravity to speed along `gravity_direction`, apply friction to decelerate `speed`, decompose `speed`/`direction` into `hspeed`/`vspeed`, add to `x`/`y`

**CRITICAL — `_partUpdate`: system position offset applied as per-step velocity:**
`runtime.ts` line ~1174: `p.x += p.vx + s.pos[0]; p.y += p.vy + s.pos[1]` — `s.pos` is the
system origin offset, not a velocity. Adding it every step causes exponential drift. Fix:
`p.x += p.vx; p.y += p.vy;` during update; apply `s.pos` offset only at draw time.

- [ ] **Fix `_partUpdate` to apply `s.pos` at draw time only**

**WARN — `part_type_color_hsv` computes one color at type-definition time, not per-particle:**
GML's HSV range parameters are per-particle randomization at spawn time. Current impl calls
`randf` once and stores one color in `t.colors[0]` for all particles of the type.

- [ ] **Fix `part_type_color_hsv` to store HSV range params; randomize per spawn in `_spawnParticle`**

**WARN — `string_insert` off-by-one (GML is 1-based):**
`string.ts`: `s.slice(0, index) + sub + s.slice(index)` — at `index=1`, slices first character
instead of inserting before it. GML `string_insert(sub, s, 1)` inserts at the very start.
Fix: `s.slice(0, index - 1) + sub + s.slice(index - 1)`.

- [ ] **Fix `string_insert` 1-based indexing**

**WARN — `buffer_wrap` kind (2) silently drops writes past end:**
`buffer_write` grows for kinds 1/3 (grow/fast-grow), early-returns for kinds 0/4 (fixed/vbuffer),
but for kind 2 (wrap) it also silently drops without wrapping the position. GML `buffer_wrap`
semantics require position to wrap to `tell % size`.

- [ ] **Implement `buffer_wrap` wraparound in `buffer_write`/`buffer_read`**

**WARN — `clipboard_get_text()` returns stale internal cache:**
Only updates from `clipboard_set_text()` calls within the game. External clipboard content
(e.g. user copying a save code from elsewhere) is never read. `navigator.clipboard.readText()`
is async — needs a pre-fetch mechanism or async init path for games that use clipboard save codes.

- [ ] **Implement `clipboard_get_text()` with async clipboard read + cached result**

**WARN — `collision_circle` ignores `prec=true`:**
Uses AABB-vs-circle test regardless of the `prec` parameter. When `prec=true`, GML uses
pixel-precise sprite masks. The `_prec` parameter is currently accepted and ignored.

- [ ] **Track as known limitation; add `prec=true` TODO comment in source**

**WARN — `draw_rectangle_color` / `draw_triangle_color` use only first corner color:**
Three of four rect corner colors and two of three triangle vertex colors are ignored. Canvas 2D
doesn't natively support per-vertex colors, but a linear gradient approximation is feasible.

- [ ] **Implement gradient approximation for `draw_rectangle_color` and `draw_triangle_color`**

**WARN — `GetField` on `Union` type always returns `Dynamic`:**
`type_infer.rs` `infer_result_type` for `Op::GetField` handles `Struct(name)` and `ClassRef(name)`
but not `Union([...])`. Field access on any union-typed value yields `Dynamic`, losing type info
after any branch that produces different object types.

- [ ] **Add Union arm to `GetField` type inference: resolve field type for each union member, join results**

### Import side: free-function and asset references
`loadAnyaDataExt`, `AnyaSticker2A`, etc. appear as bare TS2304 names because the import
generator only adds imports for `Op::Call` callee positions — not for function references
used as values (via `@@pushref@@` / `GlobalRef`). Fix: scan all `GlobalRef` / `JsExpr::Var`
nodes in the emitted function body and add any that resolve to `_init.ts` exports or
asset-table names to the file's import set.

## GameMaker Frontend

### GML `with` Statement Bugs — FIXED (2026-02-22)

Discovered via Bounty reference comparison (2026-02-22). The GML `with(obj) { ... }` statement
had two distinct bugs in the frontend translator, both now fixed:

- [x] **`with` callback uses outer `self` instead of iterated instance** — Fixed by adding
  `_withSelf: any` parameter to the arrow function and replacing all `JsExpr::This` in the
  body with `JsExpr::Var("_withSelf")` via `replace_this_in_stmts` in `rewrites/gamemaker.rs`.
  Commit: 3ba5814. Note: field accesses on the iterated instance that went through `v0`
  (IR-level) are correctly replaced because `v0` (named "self") emits as TypeScript `this`,
  which is then caught by the replacement.

- [x] **Post-`with` code not captured** — Fixed by changing `PopEnv`'s loop-back case from
  `resolve_branch_target` (emits back-edge to body, leaving fall-through unreachable) to
  `resolve_fallthrough` (falls through to continuation). The GML iteration is handled entirely
  by `withInstances()` in the runtime — the IR doesn't need to model the loop. Both the
  loop-back case (sentinel >= 0) and the break-out case (sentinel < 0) now fall through.
  Commit: 7832b3a.

**Design debt — RESOLVED**: The `withBegin`/`withEnd` bracket anti-pattern turned out to already be fixed — the GML frontend already emitted `Op::MakeClosure` + `withInstances` directly (not `withBegin`/`withEnd`). `collapse_with_blocks` was dead code only exercised by its own tests. Deleted `collapse_with_blocks`, `try_extract_with_loop_body`, `make_with_instances`, `replace_this_in_stmts`, `replace_this_in_expr`, the `withBegin` canonicalization map entry, and all associated tests.

### GML Short-Circuit AND Condition Bug — FIXED (2026-02-22)

- [x] **`if (a && b && c)` emits ternary `a ? b : c` instead of conjunction** — Root cause was
  `GmlLogicalOpNormalize` incorrectly normalizing the shared else-block when multiple BrIf
  instructions share the same else target. The pass replaced `br merge(const_0)` with
  `br merge(cond_outer)`, making the condition `(cond_inner) ? (cond_innermost) : cond_outer`
  instead of the correct falsy short-circuit. Fix: skip normalization when the trivial block has
  more than one BrIf predecessor (commit 41671f2). Output is now semantically correct:
  `(pressed === 2) ? (locked === 0) : 0` (equivalent to `pressed===2 && locked===0`).
  The switch detection also fires correctly now, producing `switch(this.type)` blocks.

### GML 2D Array Compound-Assignment Bug — FIXED (commit b3db317)

Discovered via Bounty reference comparison (2026-02-22). Fixed 2026-02-22.

- [x] **2D array `+=` wrote `inventory[sum] = const` instead of `inventory[idx] = sum`** —
  Root cause: compound assignment uses the Dup pattern: `push dim2, push dim1, Dup, VARI-read,
  arithmetic, VARI-write`. After the read+arithmetic, the stack is `[dim2, dim1, new_value]`
  with new_value on top — opposite of simple assignment `[value, dim2, dim1]` with dim1 on top.
  Fix: added `compound_2d_pending` flag, set by `translate_push_variable` when originals remain
  after 2D VARI read (`stack.len() >= 2`). `translate_pop` uses reversed pop order when set.
  Output: `self.inventory[(int(i)*32000)+1] += argument1` (correct compound assignment).

### Other GML Logic Bugs Found in Bounty Comparison

- ~~**Missing `do_gangbang_encounter` function**~~ — Confirmed custom code added after the port
  (`~/git/bounty/scripts/main6.js`). Not a missing translation. N/A.

- [ ] **`save_game` missing INI writes** — The following fields are not written to the save file:
  `name`, all `a_*` appearance fields (`a_eye_color`, `a_skin_color`, `a_height`, `a_weight`,
  `a_hair_color`, `a_hair_length`, `a_hair_straightness`, `a_hair_style`, `a_other`, `a_racial`),
  `o_prostitution`, `o_self_defense`. Reference is `~/git/bounty/scripts/main.js:save_game`.

- [ ] **`roll_d6` hardcoded Y-coordinate** — Reference: `y = 480 - 20 * size` (scales with
  dice size). Emitted: hardcoded `460`. Dice render at wrong Y position when dice size ≠ 1.

- [ ] **Stats::create broken advantages/inventory loop** — The loop initializer in create()
  emits `i = i < 20` (assigns boolean comparison result to counter) instead of the loop
  running properly. All 20 advantages slots and inventory entries may not initialize correctly.
  Also missing: `negotiate_*` fields (6 bool fields), `o_prostitution`, `o_self_defense`,
  and all 12 `a_*` appearance fields. Also missing `user0()`/`user1()` events and debug
  `keypress70`/`keypress72`/`keypress76`/`keypress77` handlers.

- [ ] **Location::create scroll condition inverted** — Reference: `if (i > 440)` sets
  `scroll = true` and skips destroying LocationScroll; `else` destroys LocationScroll.
  Emitted: condition is `<= 440` (inverted) and `scroll` is set to 1 unconditionally after
  the branch. Also text width for scale computation is 380 instead of 390.

- [ ] **Location::step/draw debug check missing** — Reference: `if (instance_exists(obj_location_scroll) && obj_stats.debug)` before drawing debug overlay. Emitted: missing the `&& obj_stats.debug` guard.

- [ ] **Cross-object writes inside `with`-bodies emitted to `_self` instead of outer self** —
  When a `with(OtherObj)` body writes to the enclosing instance (e.g. `obj_race_reader.advantage = self.number`),
  the emitted code incorrectly assigns to `_self.advantage` (the iterated instance) instead of
  `outerSelf.advantage` (the captured outer self). Root cause: the with-body closure translator
  doesn't distinguish `InstanceType::Other` (outer self) from `InstanceType::Own/_self` (iterated
  instance). Fix: inside `translate_with_body`, capture the outer `self` parameter as an extra
  capture, and emit writes with `instance == other / outer-object-id` as `captured_self.field`
  rather than `_self.field`.
  Affects: `EquipReader::step` (`_self.type = _self.type` → should be `this.type = _self.type`),
  `RaceReader::step` (`_self.advantage = _self.number`), `OptionsReader::step` (`_self.type = _self.number`),
  `MainLocMain::step` (`_self.no_use = 1`).

- [ ] **GeneralGotoMain::step missing debug mode cleanup** — Reference sets `obj_stats.debug = false`
  and deletes `obj_stats.alarm[0]` when navigating from the debug room. Both ops are absent.
  Additionally TS has an extra `save_config()` call not in the reference.

- [ ] **`a && b` compiled as `a ? b : 0` ternary instead of logical AND** — When GML bytecode
  encodes `a && b` as a diamond CFG (eval `a`; if false skip; eval `b`; merge), the structurizer
  sometimes emits `a ? b : 0` instead of `a && b`. The two are semantically equivalent in a
  boolean `if`-condition context (both falsy when `a` is false), but the ternary form is harder
  to read and should ideally be normalized to `&&`. The `GmlLogicalOpNormalize` pass handles
  the simple 2-operand case but misses the 3+ operand case (two consecutive BrIf instructions
  where the second guard shares the else-target of the first). A post-structurize AST pass
  should detect `(cond ? inner_cond : 0)` / `(cond ? inner_cond : false)` and rewrite to
  `cond && inner_cond`. Affected: `StoreButton`, `TravelButton`, `TravelMain` in Bounty.
  See also: `GmlLogicalOpNormalize` in `rewrites/gamemaker.rs` commit 41671f2.

### Boolean / Short-Circuit Detection (open)

- [ ] **Numeric booleans: `=== 1` / `=== 0` instead of boolean tests** —
  GML compiles `if (self.active)` as `push self.active; pushi 1; cmp.eq; bf`.
  Requires heuristics to identify fields only assigned 0/1/true/false across
  all functions, then replace `=== 1` with bare test and `=== 0` with `!`.

- [ ] **Enum detection (string and numeric)** — Many GML games use string
  constants as enum values. Could extract into `const` objects during type
  inference. The reference code uses `Advantages.none`, `MouseButtons.pressed`,
  etc., showing these were originally named constants.

### Missing Runtime Functions

All previously listed functions have been implemented. Check `function_modules`
in runtime.json for any newly referenced but unimplemented functions.

### Runtime Signature Verification Against GML Docs (HIGH PRIORITY)

~200+ function signatures in `runtime.json` were added by guessing from decompiled call
sites instead of checking the GML manual (manual.gamemaker.io). Multiple signatures are
known to be wrong (e.g. `action_if_variable` translator passes value instead of name,
`psn_*` and `xboxone_*` param counts were guessed). Need a systematic way to verify all
signatures en masse.

**Approach**: Codegen `runtime.json` function signatures from the GML manual source.

The manual is on GitHub: https://github.com/YoYoGames/GameMaker-Manual
HTML files have consistent, parseable structure:
- `<h4>Syntax:</h4>` → `<p class="code">func_name(param1, param2, ...)</p>`
- Parameter table: `Argument | Type | Description` with `data-keyref="Type_*"` attributes
- `<h4>Returns:</h4>` → `<span data-keyref="Type_Real">` etc.

`data-keyref` type mapping needed: `Type_Real` → `number`, `Type_String` → `string`,
`Type_Bool` → `boolean`, `Type_Void` → `void`, `Type_ID_*` → `number`, etc.

Script should:
1. Clone/fetch the manual repo
2. Walk `Manual/contents/GameMaker_Language/GML_Reference/` for .htm files
3. Parse each function page: extract name, params (names + types), return type
4. Output a verified `runtime.json` `function_signatures` section
5. Diff against current `runtime.json` and flag discrepancies

Platform functions (psn_*, xboxone_*, uwp_*) may not have public docs — flag those
separately for manual triage.

## Eliminate `any` from Emitted Code and Runtime (HIGH PRIORITY)

`any` is unacceptable in emitted TypeScript and runtime code per CLAUDE.md.
Current `any` usage is extensive tech debt. Key areas to address:

### Runtime (`runtime.ts`, `object.ts`, etc.)
- `GMLObject.[key: string]: any` — dynamic field access index signature
- Many function params/returns typed `any` (especially `action_*` functions)
- `runtime.json` `function_signatures` entries using `"any"` param types
- `global` object typed with `any`
- Event handler return types (`create(): any`, `step(): any`)

### Emitted Code
- GML script params default to `any` when type inference can't narrow
- `argument0: any = 0.0` patterns
- Cross-object field access via `(this as any)`
- `withInstances` callback return type

### Strategy
Replace `any` with:
- Specific types where inference can determine them
- `unknown` where the value truly is unknown (requires narrowing at use sites)
- Union types for known finite sets of types (e.g. `number | string | boolean`)
- Generics for container patterns (e.g. `ds_map`, `ds_list`)

## Custom Type Checker

Currently `reincarnate check` shells out to `tsc`. We want a high-quality, general-purpose type checker that operates on the IR (not emitted code), since TypeScript is not our only backend:
- Catch arity mismatches, missing functions, and type errors before emission
- Backend-agnostic — validates IR-level types, works regardless of target language
- Engine-agnostic — engine-specific coercion belongs in IR transforms, not the checker
- Already have half the story (type inference in IR passes); need the checking/validation half

Prior art: [crescent](https://github.com/rhi-zone/crescent) (`~/git/rhizone/crescent/`) — our own Lua type checker (useful reference for type-checking algorithms and infrastructure).

## IR Architecture

### Class Representation Audit (HIGH PRIORITY)

The IR's class-level types have accumulated design debt. A dedicated audit session should
review these holistically rather than fixing them piecemeal:

- **`StaticField.default: Option<Constant>`** — Only represents compile-time literals.
  Non-constant initializers (constructor calls, method calls) live in a separate cinit
  `Function` body. The emitter should at minimum inline simple cinit assignments as field
  initializers (`static HUMAN = new CockTypesEnum(...)`) instead of emitting a `static { }`
  block. Question: should the IR carry richer initializer info, or is this purely an emitter
  pattern-match?
- **Instance fields on `StructDef`, static fields on `ClassDef`** — Why split? Both describe
  class members. The split forces the emitter to cross-reference `struct_index`.
- **`abstract_members: Vec<(String, Type, Vec<Type>, MethodKind)>`** — Unreadable tuple,
  same problem `static_fields` had before the `StaticField` struct refactor.
- **Flash-specific fields on engine-agnostic `ClassDef`** — `is_interface`, `interfaces`,
  `zero_initialized`, `needs_index_signature` are Flash concepts on a core type. Law 2
  violation? Or are these general enough to justify?
- **cinit as `Function` with `MethodKind::StaticInit`** — Is a method body the right
  representation for class-level initialization, or should it be a distinct concept?

### IR Invariant Violations (found 2026-03-09 audit)

- [ ] **`FunctionBuilder::br()` / `br_if()` don't validate arg count vs target block param count.**
  A mismatch is silently accepted at construction time and causes index-out-of-bounds panics or
  silent wrong-value assignments when `branch_assigns()` zips params with args during structurization.
  Fix: add a debug-mode assertion (or always-on check) in `br()` / `br_if()` that
  `args.len() == func.blocks[target].params.len()`. Add an IR verifier pass for catching this class
  of error during testing.

- [ ] **`ValueId`s are unscoped — a value from function A can be silently embedded in function B.**
  `ValueId` is a plain `u32` newtype with no function-scope binding. If a frontend accidentally
  passes a foreign `ValueId` into `fb.br(target, &[foreign_id])`, it silently accesses the wrong
  value (or an in-range-by-coincidence value). Consider a debug-mode `FuncId`-tagged `ValueId`
  wrapper, or at minimum document this invariant as caller responsibility and add a verifier check.

- [ ] **`null_sentinel_values` is `#[serde(skip)]` — lost across IR serialization.**
  If IR is ever serialized post-Mem2Reg (e.g. for incremental builds, caching, or `print-ir`
  roundtrip), sentinel information is lost and the emitter would emit spurious null-initialization
  assignments. Not a current bug (no serialization roundtrip of transformed IR exists), but
  the field should either be serialized or the sentinel logic should be reconstructable from
  the IR itself.

- [ ] **Closure support with variable capture** — The IR has
  `MethodKind::Closure` for marking closures, and the backend can inline them
  as `JsExpr::ArrowFunction`, but there's no mechanism for closures to capture
  variables from the enclosing scope. Currently Twine arrows work because all
  state is runtime-managed (`State.get`/`State.set`), so there's nothing to
  capture. Proper closure support would need:
  - `Op::MakeClosure { func_name, captures: Vec<ValueId> }` — creates a
    closure binding captured values from the current scope
  - `Function.captures: Vec<CaptureInfo>` — records which parent values
    each closure parameter maps to
  - Backend: when inlining, strip captured params and use parent variable
    names directly (JS native closures handle the rest)
  - Touches every transform pass (new Op variant requires match arms)

## Twine Frontend

- [x] **Use a proper HTML parser for extraction** — Replaced `extract_tagged_blocks` and `extract_format_css` with `extract_script_style_blocks`, a tokenizer-based implementation using `html5ever`. Returns `TokenSinkResult::RawData(ScriptData/Rawtext)` on script/style start tags, so the tokenizer handles raw content exactly as a browser would — no manual closing-tag search, no cross-element contamination.

- [ ] **Passage rendering strategy** — Implement `passage_rendering`
  manifest option (`auto`/`compiled`/`wikifier`). In `wikifier` mode,
  Rust emitter emits passage source as string constants instead of compiled
  functions. `auto` mode scans scripts for `Wikifier.Parser` references.

### SugarCube Runtime Errors (DOL)

Two runtime errors block DOL (Degrees of Lewdity) from running:

- [ ] **User script eval failure** — The emitted `__user_script_0` is ~45k
  lines compiled into a single `new Function(...)` call. If this eval fails
  (SyntaxError or runtime error), all subsequent `window.X = X` assignments
  inside it are lost. `resolve("allClothesSetup")` returns `undefined` because
  `allClothesSetup` was defined inside the eval but the eval died before
  reaching the `window.allClothesSetup = allClothesSetup` assignment. The
  improved error reporting in `evalCode()` (split SyntaxError vs runtime
  errors, logged to console) should surface the root cause when run in the
  browser — **needs testing**.
  - Cascading failure: `get("NPCNameList")` returns `undefined` in
    `widget_npcPregnancyUpdater` because StoryInit didn't complete.
  - Potential fix directions: split user scripts into smaller eval chunks,
    or identify the specific code pattern that causes the eval to fail.

- [x] **Un-parenthesized single-param arrow parsing** — Fixed in `06a0dce`.
  The parser now handles `x => expr` in addition to `(x) => expr`. DOL:
  596 → 817 inline arrows (+221 previously broken).

### SugarCube Translator Bugs

- [ ] **CRITICAL: eliminate per-function runtime service aliases** — Every exported function
  currently emits 7 `const SugarCube_X = _rt.X;` aliasing lines at the top (DOM, Engine,
  Input, Navigation, Output, State, Widget). These aliases are unnecessary — all call sites
  should use `_rt.State`, `_rt.Output`, etc. directly. The aliases add noise to every function,
  make diffs harder to read, and cause TypeScript to allocate extra locals for every call frame.
  Fix: emit `_rt.State.get(...)`, `_rt.Output.break()`, etc. at every call site; drop the
  alias-emission block entirely. This is a backend emitter change (TypeScript backend, `emit.rs`
  or the SugarCube-specific emitter).

- [ ] **`_rt` parameter threading in trc (1935 errors)** — Passage functions are emitted as
  `(_rt: SugarCubeRuntime) => void` but some callback sites (e.g. link targets, widget calls)
  pass them without the `_rt` argument, producing TS2345 "not assignable to `() => void`".
  Root cause: the translator threads `_rt` through passage closures but the call-site types
  don't reflect it. Either passage callbacks must always accept `_rt` consistently, or the
  closure form must be `() => void` with `_rt` captured from outer scope.

- [x] **`radiobutton` emits 1 arg instead of 2 in trc** — Fixed in `3079adb`. Input macros
  (`textbox`, `textarea`, `numberbox`, `checkbox`, `radiobutton`) now use `split_case_values()`
  → `MacroArgs::CaseValues` so each arg is a discrete token. Also fixed `checkbox` arg order
  (was `checkedValue, uncheckedValue`; SugarCube is `uncheckedValue, checkedValue`) and added
  `...flags` rest param to handle `checked`/`autocheck` bareword flags.

### SugarCube Type Inference (Root Cause of TS2571/TS18046)

- [x] **Phase 1: GlobalStore/ResolveGlobalType cross-passage inference** (commit `184c090`)
  — Registers `SugarCube.State` get/set as `GlobalStore`/`ResolveGlobalType` rules so
  `TypeInference::build_global_types` builds a per-variable type map from all `State.set`
  call sites across all passages. Emitter injects `as number`/`as string`/`as boolean`/
  `as StructName` casts via `LoweringConfig::cast_narrowed_syscall_results_for`.
  Results: TRC TS2571 27392→9635 (−65%), DoL ~96K→~53K errors.
- [x] **Phase 2: setup inference + multi-pass global inference + use-site inference**
  - `SugarCube.Setup` get/set as GlobalStore/ResolveGlobalType (setup.X → typed)
  - Multi-pass Phase 3 (up to 4 iterations): propagates array element types, e.g.
    `Naked = Struct("Object")` → `OutfitList = Array(Struct("Object"))` in 2 passes
  - `Array(T)` cast injection in `syscall_cast_kind` so `as T[]` is emitted
  - `Engine.resolve()` TS overloads for JS globals + SugarCube builtins
  - **Use-site inference**: when `State.get("x")` result is used with array
    methods (`indexOf`, `length`, `push`, …) → infer `x: Array(Dynamic)` → `unknown[]`.
    When used in `CallIndirect(v, args)` → infer as `Function(…) → unknown`.
  Results: TRC 0 TS2571 (was 9635), DoL 21,709 TS2571 (was ~42K).
- [ ] **Phase 3: Struct use-site inference** (DoL 21,709 TS2571 remaining)
  — All remaining TS2571 in DoL come from state variables that are struct-typed objects
  (e.g. `$navigation`, `$worn`) with nested property chains like `.stack.last()`.
  These have no `State.set` call sites visible to the IR (set in user scripts via old SC1
  `state.active.variables.x = {...}` API).
  **Correct fix**: infer these as `Record<string, unknown>` from use-site GetField
  accesses on non-array properties, then add further inference to narrow field types.
  **Blocker**: `Record<string, unknown>` field accesses return `unknown`, pushing the
  error from TS2571 to TS18046 on every subsequent property/method use.  Requires
  multi-level struct field inference or explicit per-variable schema declarations to
  fully eliminate.  Cannot use `Record<string, any>` (violates Law 4).

### SugarCube Bare Identifier Call Resolution

- [ ] **Bare identifier calls go through Engine.resolve instead of direct dispatch** — In
  SugarCube scripts, any bare function call (`foo(args)`) is translated as
  `Engine.resolve("foo")(args)` via `CallIndirect`.  This is wrong in two cases:

  1. **SugarCube stdlib** (`visited`, `random`, `either`, `tags`, etc.) — These should be
     translated at parse time to typed `SystemCall`s directly.  The translator already has
     all the information it needs; it just doesn't specialize on known identifier callees.

  2. **Compiled game-author functions** (from `:: JavaScript` passages, `<<widget>>`
     definitions) — These are IR functions with inferred signatures.  At link time (or in a
     transform pass), any `SystemCall("SugarCube.Engine", "resolve", [const_string(name)])`
     whose result feeds a `CallIndirect` should be replaced with `Op::Call(func_id, args)`
     when `name` resolves to a known IR function.

  **Current workaround**: `LoweringConfig::cast_unknown_indirect_callee` wraps the
  `Engine.resolve` result in a function-type cast at emit time.  This suppresses TS2571 but
  loses all argument and return type information — exactly the kind of emit-level cast that
  masks a real inference gap.

  **Correct fix order**:
  - Phase A: recognize stdlib identifiers in `lower_oxc_expr` → `Identifier` arm and emit
    typed `SystemCall`s (no IR changes needed). **Attempted and reverted** (commit b24f88d,
    reverted 8f2f055, same day) — reason not recorded. Needs re-investigation before retry.
  - Phase B: add a link/transform pass that resolves `Engine.resolve(name) → CallIndirect`
    to `Op::Call` for all names present in the module's function table. **Blocked on Phase A.**
  - Phase C: remove `cast_unknown_indirect_callee` once Phase A+B cover all cases in DoL/TRC;
    remaining genuine unknowns (`globalThis` functions not in IR) are the only legitimate
    remaining use of Engine.resolve.

### SugarCube Type Emission Bugs (DoL)

- **`never[]` errors in DoL (game author errors)** — `[][_namecontroller] = x` (writing
  to a throw-away empty array literal), `[].pushUnique(...)`, `[].pluck(...)`, and
  `traits: never[]` from TypeScript's union inference on inline push args with empty
  `traits: []` in objects with inconsistent shapes. All are game author bugs in the original
  SugarCube source — reincarnate faithfully reproduces them. No fix warranted.

- **`Property '0'/'1' does not exist on type '{}'`** — `Record<string, unknown>`
  object literal annotation makes field reads return `unknown`; null-check narrowing then
  produces `{}`, and `{}[numeric]` is TS7053. Root cause: same as TS2571 below (no type
  inference for story variables). Will be resolved by the type inference pass.

- [x] **jQuery `@types` missing in DoL runtime** — Already in `dev_dependencies` in `runtime.json`; was a stale entry.

### SugarCube Remaining Stubs

- [ ] **Scripting.parse()** — Returns code unchanged (identity function).
- [ ] **L10n.get()** — Returns key as-is. Low impact.
- [ ] **SimpleAudio.select()** — AudioRunner returned is a no-op stub.
- [ ] **Engine.forward()** — No-op (deprecated in SugarCube v2).
- [x] **SCEngine.clone()** — Fixed in `8e17415`. `clone(x)` now rewrites to standalone pure function import; was emitted as `SCEngine.clone(x)` method call. Eliminated 643 TS2339 errors.
- [x] **SCEngine.iterate() / iterator_has_next() / iterator_next_value() / iterator_next_key()** — Fixed in `8e17415`. Same fix; standalone pure function imports. Eliminated ~969 TS2339 errors.
- **TS2447 `|` on booleans** (1610 errors in DoL) — DoL game authors use `|` where they meant `||`: `(v === "Kylar") | (v === "Bailey")`. This is a **game author error** in the original source. Reincarnate correctly emits the game's code. These errors are expected and no fix is appropriate — changing `|` to `||` would alter semantics, and any other suppression would hide a real bug in the game.

### SugarCube oxc Parse Errors

Status: DoL 290→4, TRC 974→0. Fixed: default-to-Raw, LinkTarget, preprocessor context
(identifiers, property names, object literals), HTML entity decoding (html-escape crate),
UTF-8 preservation in preprocessor, template literal `${...}` preprocessing, `<<run>>`
statement parsing, trailing comma stripping in case values.

- [ ] **`settings.x` / `setup.x` in `<<case>>` args** — SugarCube's `parseArgs()`
  evaluates `settings.x` and `setup.x` barewords via `evalTwineScript()`, not as string
  constants. Currently falls through to `Bareword` in `classify_case_token`.

- [ ] **`[[...]]` SquareBracket token in `<<case>>` args** — SugarCube converts `[[text|passage]]`
  in arg position to a link object. Currently not handled by `classify_case_token` at all.
  Rare in practice as a case value (object identity comparison with `===` would never match).

- [ ] **`parseArgs()` semantics for media/audio macros** — When `<<audio>>`, `<<playlist>>`,
  `<<cacheaudio>>`, `<<track>>` etc. are eventually lowered to the platform audio layer, their
  args must use `parseArgs()` token semantics (discrete values: track ID, command string, volume)
  not the full-expression path. Currently fall to `Raw` which is safe but means audio is not
  preserved in output.

- [x] **CRITICAL: Extract `Macro.add()` from JavaScript passages** — Implemented in
  `sugarcube/custom_macros.rs`. Scanner extracts block/self-closing kind (`tags:` property)
  and `skipArgs: true` semantics. Registry built from user scripts before passage parsing.
  Custom entries shadow built-ins (DoL redefines `button`, `link`). Dynamic registrations
  (variable names) skipped silently. No `<<switch>>` override exists in DoL.

- [x] **CRITICAL: `assets/styles/user_0.css` for DoL contains JavaScript** — Fixed in `57e02a9`.
  `extract_tagged_blocks` preferred `</script>` unconditionally over `</style>`, so the user
  stylesheet block captured 1.5 MB of JS. Fix: pick whichever closing tag appears first.

### Harlowe Correctness Bugs

- [ ] **`(sorted: via lambda)` — `$dm's (it)` inside via lambda** — e.g.
  `(sorted: via $players's (it), ...(dm-names: $players))` uses `(it)` as a
  property key inside a `via` lambda. Our translator lowers `(it)` as a variable
  reference. Need to verify `$dm's (it)` translates correctly in lambda context
  (should produce `get_property(dm, it)`).

- [x] **`(sorted:)` via-lambda `its X` and implicit any (rogue-time-agent, 1 error)** —
  Two bugs: (1) `its year` was parsed as `Ident("its")` → `const_string("its")` instead
  of `Possessive(It, "year")`. Fixed in `expr.rs`: `its` desugars to `it's`. (2) `sorted`
  was in `is_predicate_op` causing `infer_param_types: true` on the via-lambda, but
  `Collections.sorted` takes `...args: unknown[]` so TS can't infer the item type → TS7006.
  Fixed by removing `sorted` from `is_predicate_op`. rogue-time-agent: 1 → 0 errors.

- [ ] **Unreachable code after `(goto:)`/`(stop:)` (equivalent-exchange, 8 errors)** —
  Code emitted after `goto`/`stop` IR returns is flagged as TS7027 unreachable by
  TypeScript. The structurizer or emitter should suppress statements following a
  guaranteed-terminating call. Alternatively, the frontend should mark blocks after
  unconditional goto/stop as dead and DCE should prune them.

- [ ] **TS2304 `vNNNN` across Dispatch case boundaries (DoL, 1501 errors)** —
  Values (e.g., `iterate()` results) are declared in one `case {}` block of a
  Dispatch/switch and used in a sibling `case {}`. TypeScript sees them as
  undeclared because each case is a separate block scope. Repro: `v5242` declared in
  case 742 (`const v5242 = iterate(...)`) used in case 745 (`iterator_next_value(v5242)`).
  Fix: during Dispatch emission, detect values defined in one case but referenced in
  another, and hoist their declarations (without init, with `Assign` at definition site)
  to before the switch statement — same approach as `block_params_preamble()`.

- [ ] **Unresolved temp vars / used-before-assigned (equivalent-exchange, DoL)** —
  Two related TS errors from temp var scoping gaps:
  - **TS2304 "Cannot find name `_x`"** — `_enemycockz`/`_cockz` (equivalent-exchange),
    `_swarmamounts`/`_arrayClothes`/`_clothing` (DoL): temp vars referenced in passage
    bodies but not declared in scope at the reference site. Likely a closure capture
    ordering problem — the lambda captures the var before its alloc is visible.
    Also: `_slot`/`_tentacleColour`/`_ii`/`_outfit`/`_kylar`/`__part` etc. (21 errors).
  - **Root cause for `_args = State.get("_args")` without `let` (TS2304)** —
    When a single-store alloc has 2+ loads (e.g. `_args` loaded in both a display
    expression and an if condition), after single-store promotion the stored value (v1)
    gets use_count >= 2. `emit_or_inline(v1, count≥2)` takes the `Assign` path +
    adds to `referenced_block_params`. But v1 is NOT a block param, so
    `collect_block_param_decls` never emits `let _args;`. Fix: in `emit_or_inline`,
    when count >= 2 AND value has a name AND is not yet in referenced_block_params,
    emit `VarDecl { name, init: Some(expr) }` (not bare Assign). Track that the
    first emission was a VarDecl so subsequent uses just reference the name.
  - **TS2454 "Variable `_x` used before being assigned"** — `_hooks`/`_them` (DoL):
    var IS declared (hoisted by `hoist_allocs()`) but has no initializer, and a code
    path reaches the use before the `(set:)` assignment. Fix: initialize hoisted allocs
    to `undefined` (or the Harlowe undefined sentinel) so no path is "unassigned".

- **TS2872 in artifact (game author error)** — `(set: $cond to (cond: ...))` where
  `(cond:)` is used as a value macro. This is a game author misuse of `(cond:)` —
  Harlowe's `(cond:)` is a value macro that picks between two values, not a
  condition object. Reincarnate correctly emits the call; the TS error reflects the
  type mismatch in the source. No fix warranted.

- **TS2363/TS2367 game author comparison errors** — Boolean or type-mismatched operand
  in a comparison that TypeScript rejects. Observed in the-national-pokedexxx (TS2363)
  and equivalent-exchange line 44703 (TS2367: `boolean` vs `number`). Game author errors
  in the original source. No fix warranted.

- [x] **Temp variable block-scope leak** — Fixed in `62bcd79`. Harlowe temp vars (`_var`) have
  passage-level scope, but `Op::Alloc` was placed inside nested blocks producing block-scoped `let`.
  Fix: `Function::hoist_allocs()` moves all allocs to the entry block before structurize. Resolved
  `_twelve` (artifact), `_sex`/`_victory` (equivalent-exchange).

- [x] **Backtick verbatim spans not handled in parser** — Fixed in `cfb1796`.
  Parser now handles backtick-delimited verbatim spans; `]` inside backticks
  no longer closes hooks. arceus-garden: 203 → 3 unknown_macro calls.

- [x] **`is ... or ...` shorthand not expanded** — Fixed in `d4bcf29`.
  `maybe_distribute_comparison()` wraps bare values in matching comparison
  when `or`/`and` follows `is`/`is not`.

- [x] **Changer `+` composition emits JS `+`** — Fixed in `155af06`.
  All Harlowe `+` routed through `Harlowe.Engine.plus()` runtime call
  which dispatches by type (changers, arrays, datamaps, numbers).

- [x] **`it` in `(set:)` context** — Fixed in `5223050`. Confirmed via
  Harlowe source (`setIt()` calls `.get()` on target VarRef): `it` inside
  `(set:)` refers to the target variable's current value. `set_target` field
  on TranslateCtx substitutes `It` with a read of the target variable.
  arceus-garden: 244 `get_it()` calls → 0.

- [x] **Missing macros** — `(obviously)` was prose misparsed as macro (parser
  fix: require colon). `(forget-undos:)` implemented in engine.ts + state.ts.

### Harlowe Output Quality

- [x] **Text coalescing** — Fixed in `66ade45`. New `coalesce_text_calls`
  AST pass merges adjacent string-literal text() calls into a single call.
  arceus-garden: 2,974 → 1,874 text() calls (-37%), 16,329 → 15,429 lines.

### Harlowe Performance

- [x] **O(n²) AST pass regression (12x → 2.4x)** — The declarative content
  tree refactor created more AST statements, exposing O(n²) behavior in
  `fold_single_use_consts` and `narrow_var_scope`. Both used a
  one-at-a-time loop pattern (scan body per candidate). Fixed with batch
  passes that precompute variable reference counts in one O(n) pass.
  arceus-garden: 1.1s → 0.22s (release). Remaining 2.4x vs baseline is
  from higher statement count (content-as-values), not algorithmic.

### Harlowe DOM Fidelity — Missing `tw-*` Custom Elements

Format CSS is now extracted from `<style title="Twine CSS">` in the story HTML
and emitted as `assets/styles/format_harlowe.css`. Scaffold uses `<tw-story>`
root for Harlowe (SugarCube keeps `<div id="passages">`).

**Structural (done):**
- [x] `<tw-story>` — root container (scaffold)
- [x] `<tw-passage>` — wraps current passage (navigation.ts, `tags` attribute)
- [x] `<tw-sidebar>` — sidebar with undo/redo (navigation.ts)
- [x] `<tw-icon>` — clickable sidebar icons (navigation.ts)

**Content (done):**
- [x] `<tw-link>` — clickable links (context.ts)
- [x] `<tw-broken-link>` — links to nonexistent passages (context.ts)
- [x] `<tw-hook>` — changer-styled content wrapper (context.ts `styled()`)
- [x] `<tw-expression>` — macro output wrapper (context.ts `live()`)
- [x] `<tw-collapsed>` — collapsed whitespace sections (context.ts `collapse()`)
- [x] `<tw-align>` — aligned content (context.ts `align()`)
- [x] `<tw-consecutive-br>` — consecutive line break normalization (context.ts `br()`)
- [x] `<tw-include>` — embedded passage content via `(display:)` (context.ts)

**Content (done — macro support added, untested against real games):**
- [x] `<tw-verbatim>` — raw/verbatim text via `(verbatim:)` changer (context.ts)
- [x] `<tw-enchantment>` — enchanted element wrapper via `(enchant:)`/`(enchant-in:)` (engine.ts)
- [x] `<tw-columns>` / `<tw-column>` — column layout via `(columns:)`/`(column:)` (engine.ts, context.ts)
- [x] `<tw-meter>` — progress meter via `(meter:)` (engine.ts)
- [x] `<tw-colour>` — color swatch display in `printVal` (context.ts)

**Dialog system (done — untested against real games):**
- [x] `<tw-dialog>` — modal dialog container via `(dialog:)` (engine.ts)
- [x] `<tw-backdrop>` — dialog backdrop overlay (engine.ts)
- [x] `<tw-dialog-links>` — dialog close link container (engine.ts)

**Content (done — transition wrapping):**
- [x] `<tw-transition-container>` — wraps hook children when `(transition:)` changer is applied (context.ts)

**Testing:** None of the 21 current test games use these macros. To verify
correctness, find a Harlowe game on IFDB/itch.io that exercises these macros
and add it to `~/reincarnate/twine/`.

**CSS animation keyframes:** Now extracted from format CSS (no runtime injection).

**Error/debug (skip):** `tw-error`, `tw-debugger`, `tw-eval-*`, etc.

**Data (extraction input only):** `tw-storydata`, `tw-passagedata`, `tw-tag`

---

## Codebase-Wide Correctness Audit — 2026-03-15

Five parallel audit tracks run against the full codebase. Findings documented below; fixes come after the full picture is clear.

---

### Track 1: Silent Stubs — New Findings (62 undocumented) — partial fix in 00dffe5

All functions below silently return `0`, `false`, `""`, or `-1` without throwing, hiding missing
functionality. Per CLAUDE.md: "implement fully or throw — never stub silently."

**Layer system** (`runtime.ts`):
- [ ] `layer_get_x`, `layer_get_y` — return `0` (no layer state tracking)
- [ ] `layer_depth` — returns `0` (getter side; setter side exists)
- [ ] `layer_exists` — always `false`
- [ ] `layer_vspeed` — returns `0` for query
- [ ] `layer_background_sprite` — returns `-1` for query
- [ ] `layer_get_depth` — returns `0`
- [ ] `layer_get_visible` — always `true`
- [ ] `layer_x`, `layer_y` — return `0` for query

**Tile system** (`runtime.ts`):
- [ ] `tile_layer_find` — returns `-1` (no tile layer query)
- [ ] `tile_get_x`, `tile_get_y` — return `0`
- [ ] `tile_exists` — always `false`
- [ ] `tile_add` — returns `-1` (tile creation not implemented)

**Camera system** (`runtime.ts`):
- [ ] `camera_get_view_angle` — returns `0`
- [ ] `camera_get_view_speed_x`, `camera_get_view_speed_y` — return `-1`
- [ ] `camera_get_view_target` — returns `-4` (GML noone sentinel, not the actual target)
- [ ] `view_get_surface_id` — always `-1`

**Joystick input** (`runtime.ts`):
- [ ] `joystick_check_button` — always `false`
- [ ] `joystick_xpos`, `joystick_ypos` — return `0` (wrong; axes should return −1..1)
- [ ] `joystick_exists` — always `false`
- [ ] `joystick_direction` — returns `0`
- [ ] `joystick_pov` — returns `-1`
- [ ] `joystick_has_pov` — always `false`

**Path system** (`runtime.ts`):
- [ ] `path_exists` — always `false` (contradicts `path_add` which creates paths)
- [ ] `path_end` — empty no-op (path system nonfunctional)

**Particles** (`runtime.ts`):
- [ ] `part_particles_count` — returns `0` (see also Track 5 for arity mismatch)

**Background / sprite / font info** (`runtime.ts`):
- [ ] `background_get_width`, `background_get_height` — return `0`
- [ ] `os_get_info` — returns `{}` (empty object)
- [ ] `font_get_info` — returns `{}` (empty object)
- [ ] `sprite_get_info` — returns `{}` (empty object)
- [ ] `font_get_texture` — returns `-1`
- [ ] `sprite_get_speed_type` — returns `0`

**Command-line parameters** (`runtime.ts`):
- [ ] `parameter_count` — always `0` (no URL/launch arg parsing)
- [ ] `parameter_string` — always `""` (see also Track 5 for arity mismatch)

**Skeleton / Spine** (`runtime.ts`):
- [ ] `skeleton_animation_get` — returns `""` (Spine animation system not implemented)

**Sprite creation** (`runtime.ts`):
- [ ] `sprite_add` — returns `-1` silently (sprite loading from URL not implemented);
  callers that use the returned sprite ID will silently operate on an invalid handle

**Extension stubs** (`runtime.ts`):
- [ ] `extension_stubfunc_real`, `extension_stubfunc_string` — generic fallbacks that silently
  return `0`/`""` for ALL missing extension functions; callers cannot detect the stub fired

---

### Track 2: Type Looseness — `any` Is Never Acceptable — draw.ts/fontLookups/colorFontCache fixed in 00dffe5

Per CLAUDE.md: "`any` in emitted TypeScript or runtime code is never acceptable — use specific types,
`unknown`, union types, or generics." Every `any` below is either a bug to fix or requires a
documented design decision about why `unknown` + narrowing is genuinely unworkable.

**GML `draw.ts` — inference gaps:**
- [ ] `tex: any` in `getCachedColorFont()` → define `TextureInfo` interface
  `{ src: { x: number; y: number; w: number; h: number }; dest: { w: number; h: number } }`
- [ ] `color as any` for color-keyed array indexing in `colorFontCache` →
  use `Map<number, ImageBitmap>` to eliminate the cast
- [ ] `(tc as any).width` / `(sheet as any).width` → use a type guard or explicit union

**GML `draw.ts` / `runtime.ts` — fixed in 00dffe5 / 34f5824:**
- [x] `fontLookups: Map<number, any>[]` → `Map<number, FontGlyph>` (`FontGlyph` interface defined)
- [x] `colorFontCache: ImageBitmap[][]` → `Map<number, ImageBitmap>[]`
- [x] `(inst as any)[field]` → `(inst as unknown as Record<string, unknown>)[field]` (internal casts)
- [x] `(obj as any)[id]()` event dispatch → typed intermediate variable

**GML `runtime.ts` `getInstanceField`/`getAllField` — blocked on emitter:**
- [ ] `getInstanceField(cls, field): any` → should be `getInstanceField<T = unknown>(cls, field): T`
  Requires the GML backend emitter to generate `rt.getInstanceField<FieldType>(cls, field)`
  or `rt.getInstanceField(cls, field) as FieldType` at each call site, using the IR type
  of the assigned value. Without this, changing to `unknown` causes ~1110 TS18046 in
  emitted Dead Estate code (every `const x = rt.getInstanceField(...)` → `x.field` fails).
  Current state: returns `any` with a TODO comment; internal implementation uses `Record<string, unknown>`.
- [ ] `getAllField(field): any` — same emitter gap.

**GML `instance.ts`:**
- [x] `getInstanceField(…): any`, `setInstanceField(…, value: any)` → `unknown`
- [x] `withInstances<T>(…, callback: (inst: T) => any): any` → callback return `unknown`, outer `unknown`

**Flash `amf.ts` — serialization:**
- [x] `readValue(ba: ByteArray): any` → `unknown`; `objects: any[]` → `unknown[]`

**Flash `xml.ts` — E4X:**
- [x] `escapeAttribute(value: any)` → `unknown`; `checkFilter(value: any): any` → `(value: unknown): unknown`

**Flash `events.ts` — covariance workaround:**
- [~] `listener: (event: any) => void` — `any` is REQUIRED here. TypeScript's strict function
  type checking (`strictFunctionTypes`) makes parameter types contravariant for function literals.
  `(event: MouseEvent) => void` is NOT assignable to `(event: Event) => void` or
  `(event: unknown) => void`. The only option that accepts typed AS3 handlers without TS2345 is
  `any`. Verified experimentally: changing to `unknown` causes ~191 new errors in Flash CC.
  Fix requires either generics (loses ability to store heterogeneous listeners in a Map) or
  living with `any` on the parameter. The comment in events.ts now explains this correctly.

**Flash `utils.ts`:**
- [x] `asType<T>(value, type): T | null` — changed from `any` return to generic `{ prototype: T }`
  constraint + `T | null` return. Now surfaces ~186 AS3 null-safety bugs in Flash CC (game-author
  bugs: unguarded `as` casts that could NPE at runtime — 103 TS2531, 69 TS2322, 17 TS2345).
  Flash CC error count: 18 → 204.
- [ ] `cachedBind<T extends (...args: any[]) => any>` — `any[]` is the accepted TypeScript idiom
  for generic higher-order function constraints; return type could be improved to `Parameters<T>`.

**Harlowe `engine.ts` / `context.ts` — story variables:**
- [x] `get(name: string): any` / `set(name: string, value: any)` → `unknown`
- [x] `Changer.args: any[]` → `unknown[]`
- [x] `printVal(v: any): Node` → `v: unknown`
- [x] `plus(a: any, b: any): any` / `minus(a: any, b: any): any` → `(a: unknown, b: unknown): unknown`

**SugarCube `extensions.ts` — prototype extensions:**
- [x] `Array.prototype.delete(...items: any[])` → `...items: unknown[]`

**SugarCube `state.ts` / `engine.ts` / `wikifier.ts`:**
- [x] All `any` types replaced: storyVars/tempVars → `Record<string, unknown>`, clone/iterate/ushr/
  instanceof_/evalJavaScript/resolve params and returns → `unknown`, parseMacroArgs → `unknown[]`

**Flash `iterator.ts` / `class.ts` / `object.ts` / `scope.ts` / `globals.d.ts`:**
- [x] hasNext/nextValue/getSuper/callSuper/construct/deleteProperty etc. obj/params → `unknown`
- [x] getOuterScope(): any → `typeof globalThis`; newActivation value: any → `unknown`
- [x] trace ...args: any[] → `unknown[]`

**Flash `net.ts` / `text.ts` / `media.ts` / `ui.ts` / `desktop.ts` / `text/ime.ts`:**
- [x] URLLoader._data: string|ArrayBuffer|null; SharedObject._data: Record<string,unknown>
- [x] IDynamic* params: unknown; FileReference.save data: unknown
- [x] registerFont(fontClass: new()=>Font); Sound/Video params: unknown
- [x] Keyboard index: number; registerCursor: unknown; IFilePromise.reportError: unknown
- [x] updateComposition attributes: unknown[]

**Flash remaining `any` — documented design decisions:**
- [~] `DrawCommand.args: any[]` — renderer reads per-command-kind at specific indices;
  proper fix is a tagged union per command kind. Not done.
- [~] `Sprite/MovieClip [key: string]: any` — AS3 dynamic classes; TypeScript index signatures
  with `unknown` interact poorly with declared properties; not changed.
- [~] `platform/input.ts` listener `(e: any) => void` — same covariance as events.ts.
  `(e: MouseEvent) => void` NOT assignable to `(e: Event) => void` in strict mode.

Flash CC error count: 18 → 209 (186 from asType null-safety, ~5 from net/ui/iterator changes).
All new errors are game-author type violations (unguarded `as` casts, untyped property access).

---

### Track 3: Platform Bypass — Refined Counts

Already documented in the "Platform Interface Redesign" section above. Precise counts from 2026-03-15 audit:
- localStorage: **28** sites (25 in `runtime.ts`, 3 in `storage.ts`) — previously estimated 15+
- `navigator.*`: **11** sites — previously estimated 6+
  (includes `navigator.language`, `navigator.onLine`, `navigator.clipboard`)
- `document.*`: **12** sites — previously estimated 20+
  (title, fullscreen, createElement×4, body.appendChild, hasFocus)
- `performance.now()` / `Date.now()`: **6** sites — previously 3
  (also `Date.now()` in randomize + `get_timer`)
- `fetch()`: **1** site (`buffer_load_async`)
- `new Image()`: **1** site (module-level `loadImage`)
- `setTimeout()`: **1** site (`buffer_load_async`) — not previously counted separately

No new bypass categories found. Root causes:
1. `storage.ts` uses localStorage in module-level functions — needs `GameRuntime` ref plumbed through
2. Gamepad state queried live from `navigator` instead of reading cached `InputState`
3. No `WindowState` platform abstraction exists yet for title/fullscreen/video/download operations

---

### Track 4: Law Violations — New Finding — fixed in 00dffe5

**`FrontendOutput.extra_passes` — Law 1 design tension (not previously in TODO.md)**

- [ ] `FrontendOutput.extra_passes: Vec<Box<dyn Transform>>` in
  `crates/reincarnate-core/src/pipeline/frontend.rs:36` creates a tension with Law 1.
  Frontends inject engine-specific IR transform passes that run inside the pipeline — the
  pipeline's behavior is controlled by the frontend pass list, not solely by IR content.

  This is a real design tradeoff: frontends legitimately need to define engine-specific IR
  rewrites (e.g. `GmlLogicalOpNormalize`, `GmlBoolArithCoerce`) that only make sense given
  source-language semantics. The alternative — encoding all pass-selection logic via
  `LoweringConfig` flags in core — gives core knowledge of every frontend-specific pass,
  which also violates Law 2.

  Current injected passes only read/write IR ops and carry no non-IR state, so the practical
  violation is limited. But the mechanism allows future passes to introduce real side-channels.

  **Fix:** Add a `PureIrPass` marker trait bound on `extra_passes` entries: stateless,
  no external I/O, IR in → IR out. Enforced at compile time. This turns the implicit
  assumption into a compile-time invariant — frontends can inject passes, but only passes
  that are provably IR-only. Also rename field to `frontend_passes` for clarity.

---

### Track 5: Signature Arity Bugs — fixed in 00dffe5 / verified

From `bun scripts/gml-manual-sigs.ts --diff` vs GameMaker Manual.

**Fixed:**
- [x] `buffer_load` — was conflating with `buffer_load_ext` params; now 1 param (`string`)
- [x] `draw_roundrect` — was using `draw_roundrect_ext` signature; now 5 params
- [x] `rectangle_in_rectangle` — spurious extra `number` removed; now 8 params
- [x] `tag_get_assets` — removed spurious `kind` param; now 1 param
- [x] `vertex_position` — was including `z` (belongs to `vertex_position_3d`); now 3 params
- [x] `part_particles_count` — now 1 param
- [x] `layer_sequence_x`, `layer_sequence_y` — now setters `(seqElId, val): void`
- [x] `max`, `min`, `mean`, `median` — variadic (no fixed params)

**Also fixed (session 29, commit 029e44a):**
- [x] `shader_set_uniform_f` — was 1 param (handle only); now 2 (handle + value1)
- [x] `shader_set_uniform_i` — same fix
- [x] `ds_map_is_map` — was 1 param; now 2 (map_id + key string) per manual
- [x] `audio_play_sound_at` — was 7 params; now 9 matching runtime.ts impl

**Verified correct (audit was wrong):**
- `audio_emitter_velocity` — manual says `(emitter, vx, vy, vz)` — 4 params ✓
- `parameter_string` — manual says `parameter_string(n)` — 1 param ✓

**Variadic functions treated as fixed-arity:**
- [ ] `choose(v1, v2, ...)` → `<T>(...vals: T[]): T`
- [x] `max(v1, v2, ...)`, `min(v1, v2, ...)` — folded to binary `max_f64`/`min_f64` chain at GML call site; TS backend flattens chains to flat `Math.max`/`Math.min`. No runtime stub.
- [x] `mean(...)` — folded to `add_f64` chain + `div_f64` at GML call site. `mean(a,b,c)` → `(a+b+c)/3`. No runtime stub.
- [ ] `median(...)` — deferred; requires sorting, not expressible as a fold.
- [ ] `script_execute(script, arg1, arg2, ...)` → `(script: number, ...args: unknown[]): unknown`

**22 `any` entries in `runtime.json`** (honest representation violation):
- [ ] `draw_text` and all 6 `draw_text_*` variants — third param `any` → `string`
- [x] `array_contains`, `array_get_index` — params `any` → `unknown[]` + `unknown` (2026-05-25)
- [x] `array_height_2d` — first param `any` → `unknown[]` (IR body done, 2026-05-25)
- [ ] `array_resize` — first param `any` → `unknown[]`
- [ ] `ptr` — `(any): any` → `(number): number`
- [x] `buffer_write` — third param `any` → `unknown` (2026-05-25)
- [ ] `get_string` — second param `any` → `string` (default value)
- [x] `ini_close` — return `any` → `string` (2026-05-25)
- [x] `string_trim` — second param `any` → `string[]` (2026-05-25)
- [x] `variable_instance_set`, `texture_prefetch`, `layer_get_visible`,
  `buffer_async_group_option`, `mp_linear_step_object`, `mp_potential_step_object` (2026-05-25)
- [x] DS container value params: `ds_grid_add`, `ds_grid_clear`, `ds_list_insert`,
  `ds_list_replace`, `ds_list_set`, `ds_map_set`, `ds_map_set_post`, `ds_priority_add`,
  `ds_priority_delete_value`, `ds_queue_enqueue`, `ds_stack_push` → `unknown` (2026-05-25)
- [x] Object-ID params: `action_create_object`, `action_create_object_motion`,
  `action_create_object_random`, `action_change_object`, `action_if_collision`,
  `action_if_object` → `number` (2026-05-25)
- [x] Polymorphic params: `__throw__`, `_instanceof`, `action_if`, `action_execute_script`,
  `arrayLocalSet`, `execute_string`, `setAllField`, `setInstanceField`,
  `setInstanceFieldIndex`, `setOtherField`, `steam_activate_overlay`,
  `steam_user_request_encrypted_app_ticket`, `struct_set`, `variable_struct_remove`,
  `weak_ref_alive`, `weak_ref_create`, `withInstances`, `xboxone_fire_event`,
  `xboxone_set_rich_presence` (2026-05-25)
- [ ] **Phase 7 (deferred):** `__NewGMLObject__`, `__NullObject__`, `__This__`, `global`,
  `other`, `getInstances`, `getInstancesOf` — return `any` pending instance/scope type
  resolution. These 7 functions (8 `any` occurrences) require Phase 7 instance types before
  concrete return types can be assigned.

**Partial-implementation stubs (behavioral gap — no TODO entry = implicit correctness claim):**
- [ ] `array_shuffle(arr, start, count)` — `start`/`count` params now present in signature (reconciled with builtins_generated.rs), but the IR body still ignores them and always shuffles the entire array. GML spec: only elements `arr[start..start+count)` should be shuffled. Behavioral gap remains; entry stays open until the IR body is implemented.

**Notable return type mismatches:**
- [x] `ds_map_add`, `ds_map_replace` — already return `boolean` in runtime.json
- [x] `file_delete`, `file_rename` — already return `boolean` in runtime.json
- [ ] `display_reset`, `surface_reset_target` — return `void`, manual: `number`/`boolean` (needs GML manual verification before changing — skip for now)
- [ ] `buffer_peek`, `buffer_read` — return type depends on `buffer_type` runtime param;
  known limitation requiring dependent types to express precisely — document as such
- [ ] Layer name vs ID: several layer functions accept `string` (layer name) per manual but
  our sigs show `*` or `number` — layer names and IDs are distinct; audit which is which

---

### Harlowe Phase 2 (Advanced Features)

- [x] **`(for: each _item, ...$arr)[hook]`** — Loop lowering (done)
- [x] **`(live: Ns)[hook]` + `(stop:)`** — Timed interval (IR lowering, runtime `live()`/`stopLive`, navigation cleanup all done; regression test present)
- [x] **`(click: ?hook)[hook]`** — Event handler targeting named hooks (done — `click_macro` in engine.ts handles selector + hook callback)
- [x] **Collection constructors** — `(a:)`, `(dm:)`, `(ds:)` (done — runtime + frontend complete)
- [x] **Collection operators** — `contains`, `is in`, `'s`, `of` with full Harlowe semantics (done)
- [x] **Lambda expressions in collection ops** — `each _x where _x > 5` as predicate callback for
  `(find:)`, `(some-pass:)`, `(all-pass:)`, `(none-pass:)` etc. (done — `build_lambda_callback`)
- [x] **Lambda `via` expressions + `(sorted-by:)` + fold lambdas** — `via expr` tokenized and lowered
  as `ViaLambda`; `each _x making _acc via expr` parsed as `FoldLambda` for `(folded:)`;
  `(sorted-by:)` added to frontend + runtime; `interlaced`, `repeated`, `folded` runtime functions added.
- [x] **Dynamic macro calls** — `($varName: args)` expression-position calls; `(macro: type _p, [body])`
  closures; `ExprKind::DynCall`; `ExprKind::MacroDef`; `extract_macro_definition()` in lexer;
  `parse_macro_definition_args()` in parser; `build_macro_closure()` in translator.
- [x] **Changer composition with `+`** — `(color: red) + (text-style: "bold")` (fixed in `155af06`)
- [x] **`(save-game:)` / `(load-game:)`** — Save integration (basic runtime done; already implemented)
- [x] **`(replace:)`, `(show:)`, `(hide:)`** — DOM manipulation hooks (done)
- [x] **`(meter:)`, `(dialog:)`, `(dropdown:)`, `(checkbox:)`, `(input-box:)`** — UI macros (implemented)
- [x] **`(verbatim:)[...]`** — Raw text pass-through via `<tw-verbatim>` element
- [x] **`(enchant:)` / `(enchant-in:)`** — Apply changers to matching elements via `<tw-enchantment>`
- [x] **Named hooks** — `|name>[hook content]` and `?name` hook references (done)
- [x] **Complex `'s` possessive chains** — `$obj's (str-nth: $idx)` nested macro in possessive
  (already handled: `parse_prefix` falls to `try_parse_inline_macro` after `'s`)





## Future Backends (POST-PHASE-9 BACKLOG)

After all rewrite phases are complete and the IR is clean.

- **`reincarnate-backend-love2d`** — Lua + Love2D target. Validates cross-language IR design (different calling conventions, no classes, `for` loops over iterables). Shared Lua AST crate (`reincarnate-ast-lua`) if other Lua targets follow (PICO-8, plain LuaJIT).
- **`reincarnate-backend-bevy`** — Rust + Bevy target. Strongest validation of "no TypeScript assumptions in IR" — Rust's type system will reject any emit-form leakage immediately.
- **`reincarnate-backend-godot`** — GDScript or C# target via Godot. Natural GML migration path given similar architecture (nodes/objects, signals, game loop).
- **`reincarnate-backend-android`** — Kotlin/Java target for native Android. Eliminates WebView dependency for JoiPlay-class deployments; pairs naturally with Love2D (Android is a first-class Love2D target).

- [x] add Subagent Prompts discipline section — see github-io for canonical version

---

## Open threads (session 31 close)

*Open threads from a previous session. Treat as starting context, not instructions — verify relevance before acting.*

### `unknown`-inference gap (likely the highest-leverage thread)

Dead Estate still has ~1877 TS2345 and ~31 TS2416 errors (as of session 31 close). The dominant remaining cluster appears to be values typed `unknown` where a concrete source-language type should be inferable.

**Why it matters:** Per Law 4, `Type::Unknown` (formerly `Type::Var` before the rename in commit `0bb06399`; now `Type::InferVar` for solver-internal inference vars) is an inference failure, not a legitimate type. It is also the shared root behind a second observation: when `id` was retyped as `GMLObject` (commit `91e327aa`), zero new numeric-id-misuse errors surfaced — because `id`'s numeric uses flow through `unknown`-typed intermediates, so TypeScript never sees comparisons like `GMLObject === -4`. Improving `unknown`-inference may both shrink the TS2345 cluster AND unlock the static verification benefit the `id` retyping was meant to provide.

**Open question:** Where the `unknown`s originate (frontend translation gap? a specific transform abstaining? builtin signatures registered as `Unknown`?) — needs investigation before any approach is chosen. The per-commit work this session (`RedundantInheritedFieldPrune` pass, `z` field fix, `id` retyping) removed band-aids and made the genuine inference gaps visible; the error counts now reflect real gaps.

### Possible future API tightening (low priority, noted, not urgent)

The runtime instance-handle APIs (`instance_exists`, `instance_destroy`, `getInstanceField`, `withInstances`, `collision_*`) accept a `number` arm for the object-index (class-as-integer) case. With instance `id` now `GMLObject`, that `number` arm could potentially be narrowed to only the object-index type for stronger static verification. Uncertain whether worth it; flagged because the `id`-as-`GMLObject` direction makes it conceivable. No action warranted until the `unknown`-inference gap is substantially closed.

---

## Ad-hoc dispatch findings (2026-05-29)

From an ecosystem-wide investigation of ad-hoc dispatch architecture (2026-05-29). The recurring anti-pattern: N parallel dispatch tables keyed on a closed name/enum set where one registry/trait/visitor belongs — strongest tell is DRIFT (parallel tables disagreeing). Each finding names the general mechanism it should have been.

- **I1 — `EngineOrigin`→string match duplicates trait identity.** `reincarnate-cli/src/main.rs:509-514` (`resolve_runtime`) maps `EngineOrigin::{Flash,GameMaker,Twine}` to dir-name strings; the `Frontend` trait has no `runtime_name()`/`name()` method (the `Backend` half correctly uses `backend.name()`). SHOULD BE: a display/dir-name method on `Frontend`/`EngineOrigin`.

- **I2 — `disasm` bypasses the Frontend→IR→Backend pipeline.** `main.rs:1876` guards `if manifest.engine != EngineOrigin::GameMaker` then calls `datawin::DataWin::parse` directly; no `disassemble()`/`bytecode_reader()` on `Frontend`. Judgment note: disasm operates on pre-IR binary, so whether it belongs on the trait is a design call — but the manual `EngineOrigin` guard in CLI is the structural signal.

- **NOT a smell (do not "fix" this):** `find_frontend`/`find_backend`/`find_checker` (`main.rs` ~462, 553, 1033) are feature-gated registry constructors — the correct place for per-type match arms.

---

## Dead Estate inference work — corrected baseline & measurement hygiene (2026-06-05)

*Open threads from a previous session. Treat as starting context, not instructions — verify relevance before acting.*

1. **Authoritative Dead Estate error baseline: TS2345 = 2101, total = 25446 errors** (0 warnings). Reproduced 3× — with a clean `cargo build`, a forced emit cache MISS, and two independent ways of obtaining the total (the `--filter-code TS2345` run header "Showing 2101 of 25446 diagnostics", and an unfiltered `check --examples=0` → "Total: 25446"). This SUPERSEDES other figures used earlier this session: the "~1,877 TS2345" and a transient "763 / 87,279" measurement were both contaminated (stale binary / broken-emit state). Use 2101 / 25446 as ground truth.

2. **Measurement hygiene (the cause of this session's wasted effort):** numbers were repeatedly contaminated by running a STALE BINARY — hand-copied binaries, or `cargo build` no-op'ing while an old binary got run. Durable rules:
   - (a) Always invoke via `cargo run -p reincarnate-cli -- …` so it rebuilds-then-runs; never hand-copy/run `target/debug/reincarnate-cli`.
   - (b) Use `--examples=-1` (the space form `--examples -1` fails clap parsing — `-1` is read as a flag — and exits with empty stdout, which can be misread as "zero errors").
   - (c) `emit` cleans `output_dir` (`fs::remove_dir_all`) only on a cache MISS; a cache HIT returns early and skips cleanup, so leftover/foreign files in `out/` survive a cached emit.
   - (d) `check --no-emit` type-checks whatever is on disk and does no cleanup — sharpest footgun for reading stale/foreign files.

3. **Emit cache change (commit `0376151c`):** `emit_cache_key` now incorporates the running binary's mtime instead of a content hash. NOTE: this is a conservative policy (any relink invalidates the cache) but does NOT address the real contamination vector — running a stale binary — since any cache key derived from `current_exe()` describes whichever binary is actually executing. The procedural rules in (2) are the real safeguard. The cache-hit-skips-cleanup hazard in (2c) also remains.

4. **Diagnosis of the `unknown → number` flood** (against the true 2101 baseline; approximate proportions — RE-MEASURE before relying):
   - Dominant sources are unknown-typed script params (`argumentN: unknown = 0.0`, TODO U4) and a smaller slice of genuinely-Unknown builtin returns.
   - A separate large bucket — `this → SpecificType` receiver mismatches (~26% of TS2345) — is a distinct method-dispatch problem, not an Unknown gap.
   - ADR-004's Template/parametric-builtins feature addresses only a small slice (~5–10%); it is NOT the top lever.

5. **Phase 1 done (committed, `fe19f330`):** fixed the gen-gml-builtins HTML parser to match `<h4>Returns</h4>` (no colon); 31 builtins flipped Unknown→concrete; also fixed generator drift (missing `param_lower_bounds` emission). NET Dead Estate impact: ~zero (none of the affected builtins are exercised in ways that surfaced the Unknown on this corpus). Correct fix regardless; helps other corpora.

6. **Phase 2 (U4 param inference) — IMPLEMENTED (correct, but low payoff on this corpus).** The redesign landed in two commits:
   - **Phase 1 (`5ab379d4`, refactor(gamemaker)):** removed the default-literal TYPE FLOOR in `default_arg.rs`. `recover_defaults` no longer overwrites a param's type from its default literal, and `set_variadic_defaults` no longer widens non-scalar arg params to Unknown. The pass now records only the default VALUE; the param type is left as a fresh inference Var. The floor was a "propagate types by guessing" monkeypatch (a default value does not constrain a param's type), so removing it is correct.
   - **Phase 2 (`e48b488c`, fix(core)):** added an evidence-completeness gate to the HM solver's call-site param inference (`constraint_solve_hm.rs`). `ParamEvidence` tracks per-param concrete call-site types, the default, and `saw_unknown`. A freed param narrows only when all contributing call sites are concrete and consistent; incomplete evidence (`saw_unknown`) or a call-site-free param stays free. The default is one piece of evidence, never a floor. Duplicated seeding bodies were extracted into one `seed_param_from_arg` helper (ratchet stayed green for that file).

   **Measured outcome on Dead Estate (pre = `1c5a3b75`; post = `e48b488c`):** TS2345 2101 → 2092 (−9); total 25446 → 25476 (+30). The change RELOCATED unknowns rather than closing gaps: the TS2345 −9 turned into TS18046 +28 / TS2322 +9 / TS2769 +2 — honest surfacing of inference failures, not new defects ("TS error counts are not a goal"). Both phases are correct under the rules; they just do not close the unknown gap on this corpus.

   **Why the payoff is bounded (verified):** of the ~1316 "unknown → X" TS2345 errors, only ~270 are freed `argumentN` params at all. Of the freed-param positions (178 across 133 functions), **~~66% (117)~~ have NO static call sites** — ~~library/dead/dynamically-dispatched functions that call-site inference can never type (honest Unknown)~~. The DOMINANT "unknown → X" source is `self` / `this.<field>` reads typed unknown (hundreds of positions), which U4 never touches. A small genuine residual (~8 positions with all-concrete-literal call sites, e.g. `wave/arg0..2`) is correctly bounded by the `saw_unknown` gate when other call sites pass unknown args.

   > **CORRECTED 2026-06-12 (re-derivation against Dead Estate corpus):** the subset figures above (133 functions / 178 positions) were restricted to TS2345-producing call positions. Full population: **645** `_init.ts` script functions carry `argumentN: unknown` params; **393 (60.9%)** have zero direct call sites in the emitted TS. Of those 393, only **~50 (~13%)** are genuinely dead: 10 embedded GIF-library functions (`gif_std_*`, `format_gif_*`, `read_anon_*`, entry point `sprite_add_gif`) and ~40 dead game scripts (PS5/PSN platform stubs, unused map-gen/draw/audio helpers) — confirmed dead in bytecode (no `Call.i`, no `pushref` at their SCPT indices). The remaining **~343 (87%) ARE called** via mechanisms the pipeline doesn't statically resolve: 258 GMS2.3 anon closures + 50 `___struct___` constructors + 7 named constructors (`ColorSwap`, `cutscene`, `InventorySection`, `OptionsSection`, `OutlineTextSurface`, `SpriteButton`, `Tunneler`) + 5 method()-stored functions + 23 `live_*` hot-reload-framework functions dispatched via `live_call()`. (`script_execute` has zero uses in this game's bytecode.) The "information doesn't exist" conclusion was wrong — the evidence exists in the bytecode and is destroyed or obscured before the solver sees it. Two root-cause defects are recorded below.

   **Corrected invariant (now law in CLAUDE.md, `ad31f37a`):** `Type::Unknown` is CONCRETE — a terminal type, not a free variable. `is_concrete(Type::Unknown) == true` is correct by design, and the `saw_unknown` gate correctly blocks narrowing a param when any call site supplies a genuinely-unknown-typed value. An earlier diagnosis this session mislabeled both as "solver bugs"; that framing was wrong — do not re-open it.

   **Deferred prerequisite (multi-call-site UNION narrowing):** a consistent 2+ call-site param emits `Equal(Union([...]), var)`, but `unify`'s `(Union, _) => Unknown` arm (`constraint_collect.rs:361`) collapses it to Unknown. Fixing that arm (its own ratchet check) also unblocks the self/this.field → SpecificType receiver bucket; it is the shared prerequisite.

   CONFIRMED facts to reuse:
   - Intentional-any params (select, `_any` builtins, runtime-library sigs) are STRUCTURALLY EXEMPT because `Module::register_runtime` builds them with EMPTY entry-block params, so the seeding loops (which key off entry-block param ValueIds) never reach them.

   **Next lever:** receiver-typing arc LANDED (item 9 below). Successor lever: TS2339 authored-field inference on now-narrowed receivers (item 10 below).

7. **`@@NewGMLObject@@` constructor erasure — Law 3 behavioral correctness bug (2026-06-12, HIGH PRIORITY).**

   The TS backend rewrites `@@NewGMLObject@@` to `new GMLObject()` unconditionally (`crates/backends/reincarnate-backend-typescript/src/rewrites/gamemaker.rs` ~line 843), discarding the constructor reference entirely. The IR registration (`crates/frontends/reincarnate-frontend-gamemaker/src/lib.rs` ~line 200) registers `@@NewGMLObject@@` with empty params, so the pushref'd constructor FuncId is never modeled in IR at all.

   **Verified emitted consequence:** `out/Dead_Estate/objects/OEnterDream.ts` contains `[new GMLObject(), new GMLObject()]` where the source constructs `new ColorSwap(color, baseColor, threshold, amount)`. Every GMS2.3+ constructor invocation in every game emits a bare `GMLObject` — silently wrong behavior, a Law 3 violation. 57 constructor functions affected in Dead Estate alone (all 7 named constructors + all 50 `___struct___` constructors).

   **Fix direction:** model the constructor reference as an operand of `@@NewGMLObject@@` in IR (frontend); emit `new <Ctor>(args)` (backend). This also restores call-site type evidence for all constructor params, directly unblocking the U4 residual for the 57 affected functions.

8. **Indirect-call evidence loss for GMS2.3 closures and method-vars (2026-06-12, HIGH PRIORITY).**

   `CallV` lowers to `call_indirect` with an opaque callee value. `Break.i pushref` resolves the function name (`asset_ref_names` includes SCPT), but the callee value's connection to the concrete FuncId/signature is not carried through IR, so: (a) call-site arg types never reach the callee's params in the HM solver — the `saw_unknown` gate is starved of call-site evidence; and (b) where the callee ref is a constant pushref, emit performs an indirect call (`_rt.callScript(...)` or similar) where a direct typed call is possible — the emit-quality defect class named at the top of CLAUDE.md.

   **Scope in Dead Estate:** 258 GMS2.3 anon closures + 5 method()-stored functions. The 23 `live_*` hot-reload-framework functions dispatched via `live_call()` are a separate dynamic-dispatch case (runtime string dispatch, not a constant pushref).

   **Fix direction:** when a `CallV` callee value is provably a constant pushref to a known FuncId (detectable at IR construction or in a dedicated lowering pass), lower it to a direct `Call` to that FuncId. This makes the call visible to call-site inference and eliminates the indirect-call emit. Non-constant `CallV` remains indirect.

9. **Receiver-typing inference arc — LANDED (commit chain on master, 2026-06-12).**

   **RESOLVED.** Five commits closed the self/this.field receiver-typing lever, which was the
   dominant `unknown`→X source. The Equal-link propagation leak (item 10) and the LCA-blocked
   framing (item 9 prior) are both resolved by `4de16db9`'s post-fixpoint join design. Items 10
   and 11 are superseded by this arc.

   Commit chain:
   - **`7db18235`** — ownerless-script `self` re-spelled as a fresh `InferVar` (was mis-spelled
     `Type::Value` after the Unknown→Value rename; `is_concrete(Value)==true` caused the solver to
     skip call-site seeding entirely). Restored call-site seeding + GMLObject lower-bound fallback;
     `self: unknown` 979 → 92.
   - **`b369cecc`** — evidence gate enforces true caller completeness. `ParamEvidence.incomplete`
     now fires on ALL dropped-caller paths (silent `usage_suppressed` early-return and the
     non-concrete-arg link path were both bugs). Single-caller and union/LCA narrowing require
     complete caller enumeration. Fixed −22 unsound `this`-not-assignable TS2345.
   - **`864cddf7`** — address-taken analysis bounds indirect-call incompleteness. `compute_address_taken`
     enumerates all FuncId-escape routes (`MakeClosure`, `CoroutineCreate`, `GlobalRef`→SCPT). A
     function is marked `incomplete` only when address-taken AND an opaque `CallIndirect` exists (or
     name collision). Non-address-taken funcs stay sound. Unit-tested; no metric moved (sound
     foundation only).
   - **`4de16db9` — THE root fix:** call-site param inference is a JOIN (LCA over the object-parent
     hierarchy) of the complete caller set, not equality unification. Three parts: (1) completeness
     distinguishes genuinely-dynamic callers (`Type::Value`/opaque → `incomplete`) from not-yet-
     resolved linked param-vars (recorded as directional lower bounds, not poison — removes the
     Equal-link propagation leak); (2) post-fixpoint join over the complete lower-bound set, iterated
     to a fixpoint; (3) the sound call-site join takes precedence over the HasField single-owner
     heuristic (a fallback for evidence-less values only). Result: `getPlayerX.self → GMLObject`
     (supertype of all 319 callers incl. `BossKey`, not the heuristic's wrong `Enemy`);
     `drawSelfNormally → WorldObject`; `hurt`/`sayOtherExt.self → concrete instance`. `this`-not-
     assignable 1451 → 236; `this`-not-assignable-to-`number` 112 → 0.
   - **`950f5e4a`** — instance-reference builtin params (`instance_destroy`/`instance_exists`/…
     "Object Instance or Object Asset") were mistyped `Int(32)` by the signature generator (VM
     storage format leaking into source type, Law 4); now `Type::Value`. Fixed in the frontend
     generator (`tools/gen-gml-builtins`, Law 2). This eliminated the `Equal(self, Int(32))`
     constraint that was typing 16 ownerless scripts' `self` as `number`.

   **Session result:** `this`-not-assignable 1451 → 236; `this`-not-assignable-to-`number` 112 → 0;
   total ~22,445 (from pre-loud baseline ~25,446, while carrying the deliberate ~4,935 RC0006 loud
   backlog). Also landed in the same session: TS1243 declare-override 926→0, TS7053 stranded
   native-dispatch reconnected ~9k→~37, TS2322 void event-returns 727→0, TS2339 GML builtin
   instance vars/globals ~1,961 fixed, rest-param default syntax.

   **Follow-up (instance-creating builtins, emit quality):** `instance_create_depth` and family now
   return `Type::Value` rather than `Int(32)`. A human port would type the result as the created
   object's type. Recovering that requires flowing the object-asset argument's classref into the
   return type (frontend-local, GML `instance_create_*` family). Tracked; not yet done.

10. **Next levers — current landscape (post-arc, 2026-06-12).**

    - **TS2339 (~7,355) — authored fields on now-narrowed receivers.** Receiver narrowing honestly
      un-masked field accesses that `unknown` was swallowing: `self.<authored_field>` now needs the
      field to resolve on the receiver's concrete type. This is the per-object field-inference lever
      (ConstructorStructInfer / struct-field inference from all events, not just `create`). The
      largest remaining bucket; the natural next target now that receivers are typed.

    - **RC0006 (~4,935) — the deliberate loud inference backlog** (Call/GetField/closure results
      that genuinely don't resolve). Reducing it needs the closure/method-var FuncId threading (the
      call-graph-completion lever — items 7 and 8 above) and continued field/return inference.

    Key files: `crates/reincarnate-core/src/transforms/constraint_solve_hm.rs` (the join/drain/
    post-fixpoint machinery), `crates/frontends/reincarnate-frontend-gamemaker/src/transforms/
    constructor_struct_infer.rs` (field inference — next lever), `tools/gen-gml-builtins` (Law-2
    boundary for instance-ref builtin types).

11. **Constant-pushref `CallV`→direct `Op::Call` lowering — SUPERSEDED / NO VALID TARGETS IN DEAD ESTATE.**

    Investigated 2026-06-12: of 2269 `Op::CallIndirect` sites in Dead Estate, ZERO are constant-
    pushref-SCPT idioms (all are genuine struct method-vars or dynamic helpers). The canonical
    `getPlayerX` case was always reached via direct `Call.i` ops — its call graph was already
    complete. The Equal-link propagation leak that blocked LCA has been fixed by `4de16db9`'s
    directional lower-bound design (item 9). Phase-1 lowering remains correct in principle for
    corpora that DO emit constant-pushref-SCPT `CallV`, but is a no-op for Dead Estate. Re-evaluate
    against a corpus that actually contains the idiom before building.

## Adversarial audit findings (2026-06-12)

All items verified empirically or by direct code reading. Ordered by severity.

1. **CRITICAL — bare `getInstanceField`/`setInstanceField` identifiers emitted (TS2304,
   non-compiling output; regression triggered by the receiver-typing arc, item 9 above).**
   `crates/backends/reincarnate-backend-typescript/src/rewrites/gamemaker.rs` (~1720-1859)
   rewrites `GameMaker.Instance.getOn/setOn` calls to a bare unqualified
   `JsExpr::Var("getInstanceField")` instead of `_rt.getInstanceField(...)` — these are
   runtime-instance closures per Law 5, not free functions. The rewrite bug predates the arc
   (`e2d4645c`, 2026-05-23) but `950f5e4a` + `4de16db9` changed which call shapes route
   through it, triggering it at volume: TS2304 Dead Estate 18→3,899, Bounty 25→839, Undertale
   ~2,937→~28,072. Bounty total regressed 2,208→3,819 (+73%); Undertale 113,516→115,747.
   Emitted output does not compile (e.g. `Bounty/_init.ts:376`). **Dead Estate's "clean"
   22,445 baseline (item 9) silently contains ~3,899 of these.** Fix: qualify the callee with
   `_rt`. Root cause of the miss: only Dead Estate totals were measured after the arc, never
   per-code diffs or other corpora — cross-corpus + per-code measurement must accompany
   future inference arcs.

2. **HIGH — solver: collector `Equal(arg_var, InferVar(0))` path is defective and
   redundant.** `constraint_collect.rs` `Op::Call`/`MethodCall` arg handling (~866-877,
   ~948-957) emits `Equal(arg_var, param_ty.clone())` when the callee sig param is
   `InferVar` — but the sig placeholder is the raw `Type::fresh_var()` = `InferVar
   (TypeVarId(0))`, which the arena dereferences as REAL slot 0 (`module.globals[0]`).
   Verified on Dead Estate: 18,506 such constraints; effect today is mild (2 params degraded
   GMLObject→unknown: `gif_std_Std_stringify`, `gml_std_enum_toString`) only because slot 0
   happens to resolve to `Value` — if `globals[0]` were concrete this is a mass-aliasing
   hazard. The path also silently pre-empts the post-fixpoint join (item 9's `4de16db9`) for
   any param with ≥2 direct callers: binds first-caller, poisons to `Value` on disagreement,
   and the join then skips the now-non-free var — so the join currently fires mostly for
   self/receiver params (the one path that doesn't emit this `Equal`). Fix: remove the path
   (seeding/join is the sound interprocedural channel) and/or make `fresh_var` non-colliding;
   re-measure.

3. **MEDIUM — solver: join-precedence gate tests the wrong predicate for `incomplete`
   params (active, currently benign).** The HasField single-owner gate and the Step-4.5 gate
   test `join_param_vars` membership; `incomplete` params are excluded from `join_param_vars`
   by construction, so they PASS the gates and can be heuristic-bound despite a known-
   incomplete caller set. Verified: 3 incomplete params bound on Dead Estate
   (`increaseAnyaSpeed`→`OAnya` etc.) — all currently body-consistent, zero wrong emitted
   code today, but unsound in principle. Fix: gate the guessers on `!incomplete` (or an
   explicit has-any-caller-evidence check); decide policy for the 3 affected.

4. **MEDIUM — solver structural debts (code-verified):**
   - `unify`'s `(Union, _) => Value` catch-all destroys all unions — blocks union support
     structurally (same arm already flagged as the multi-call-site UNION narrowing
     prerequisite above).
   - `MAX_JOIN_PASSES = 3` uses `assert!` — panics in release on deep join chains; should
     degrade to fallback, not crash.
   - Occurs-check failure silently binds `Value` — a loud-posture violation (CLAUDE.md); should
     diagnose instead.
   - The `ConstraintCollect` `Transform` is dead relative to the solver (the solver
     re-collects itself; the `Transform`'s `RefCell` output is never read), and the two
     collection paths diverge on `runtime_registry` handling — remove or reconcile.

   **Prognosis:** the equality-arena + outside-join accretion (11 interacting mechanisms, no
   statable single invariant) points toward an eventual biunification-style solver (native
   lower/upper bound sets) as the long-term direction. Track, do not build yet.

5. **Flash CC corpus panics at HEAD (pre-existing, not this arc).** `builtin_type_suffix`'s
   unsupported-type panic, introduced by `1b7c0bf6` (a silent fallback replaced by a hard
   panic), makes Flash currently unmeasurable end-to-end. Needs its own fix before Flash can
   be used as a cross-corpus check for future arcs (see item 1's lesson).

6. **Cross-engine generalization debts (Law 1/2 design audit):**
   - Runtime-bodies M+N is ~3.5% real (40/1139 GML functions use `attach_runtime_body`;
     Flash's 9,968 LOC and Twine's 8,580 LOC handwritten TS use it zero times) — a second
     backend currently implies rewriting ~everything by hand, the exact M×N the design
     (CLAUDE.md, Hard Constraints) forbids.

     **Corrected reading of the 3.5% / zero-usage figures (git-dated, 2026-07-03):** Flash's
     runtime was written Feb 10–Mar 15 2026 and Twine's Feb 15–Mar 18 2026, both frozen since;
     the IR-bodies mechanism first existed Mar 29 (`b76028c3`, raw FunctionBuilder) and
     `attach_runtime_body` only landed May 23 (`e234d739`). So zero Flash/Twine usage is NOT a
     rejection of the design — the runtimes predate the mechanism, no migration was ever
     attempted (already tracked as blocked, separate scope), and the deeper blocker is that
     runtime bodies flow through the same inference/lowering/emit pipeline as game code, which
     is not yet capable of carrying runtime-grade code faithfully (see this same audit: item 1's
     non-compiling dispatch emission, the RC0006 backlog, item 4's union destruction). The 40
     functions that DID get bodied are the pure-logic slice the pipeline can already express —
     adoption tracks the capability boundary, not designer intent.

     **Sequencing decision (2026-07-03):** M+N is neither dropped nor defended — it is
     SEQUENCED BEHIND pipeline soundness. Unblock criterion: the pipeline can express and
     faithfully emit a representative slice of the GML runtime (state-touching + dispatch-
     touching functions, not just pure logic). Once that gate passes, evaluate the authoring
     format empirically: hand-authored IR bodies (`attach_runtime_body`) vs self-hosting
     (author runtimes in TS and translate them with Reincarnate itself via a TS-subset
     frontend — currently forbidden by the CLAUDE.md runtime-bodies constraint; that
     prohibition's rationale (IR churn) should be re-examined at gate time, not now). Until the
     gate passes, new-backend planning must assume per-pair handwritten runtimes (lazy
     materialization: only demanded (engine, backend) pairs, not the full matrix) — and the
     platform interface is the load-bearing portability boundary under EVERY branch of this
     decision, which raises the priority of item 7's codify-and-enforce fix.
   - `TypeDecl::Object.parent: Option<TypeId>` (single parent) cannot represent AS3
     interfaces — neutrality is currently bought by discarding source info (Law 2/4). Honest
     form is multi-parent; join/LCA (item 9's `4de16db9`) must generalize to a
     common-ancestor-*set*.
   - `param_lower_bounds` has exactly one producer (GML `self`) — general-in-name-only until
     a second engine populates it.
   - The completeness/address-taken machinery (`864cddf7`, item 9) assumes closed-world
     enumerable callers. This holds for narrative engines (Twine by-name passages, Ren'Py
     labels, HyperCard message sends) ONLY IF their frontends emit those dispatch forms as
     opaque indirect calls — nothing in core enforces that frontend discipline today.

7. **Platform interface substrate (autopsy, docs/architecture.md:358-473).** The documented
   handles/primitives/enums design is sound and Love2D-implementable, but: no codified
   contract exists (loose module functions, no interface/trait, no build gate — bypasses are
   free); DOM types leak through realized signatures (`ImageBitmap`, `Document = document`
   defaults, `HTMLCanvasElement`, `CanvasRenderingContext2D`, `DOMRect` — un-portable by
   construction); graphics migration never happened (52 raw ctx sites in
   `gamemaker/runtime.ts` alone; frozen since 2026-03-15); `setColorTransform` is a silent
   no-op (fail-loud violation; correct impl already specified at architecture.md:455);
   consumer call sites bypass the abstraction and bind page-global `document`/`window`
   directly (breaks Law 5 multi-instance). Fix order: codify the contract as real
   interfaces + a build-failing bypass lint FIRST, then purge DOM types from signatures, then
   migrate graphics.


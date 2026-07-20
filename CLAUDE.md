# CLAUDE.md

## Goal

Reincarnate translates games from any source engine into working, type-safe, high-quality code on any target platform — for preservation, migration, and cross-platform deployment. The emitted code is the artifact — it must be indistinguishable from a skilled human port. The bar is not "compiles" or "runs."

**Never suggest bundling an existing interpreter.** inkjs, Parchment, renpyweb, libqsp-WASM are "quick deploy" alternatives — not the goal.

**Emitted code is measured against handwritten code.** Runtime name lookup where a direct call is possible, `unknown` where a concrete type is inferrable, or any other indirection a human would never write are all defects. Closing the gap is always higher priority than adding new features.

## Quality

**TS error counts are not a goal.** Any change that reduces errors by widening a type, guessing at correct behavior, or silencing a diagnostic is a regression. The only valid reason to make a change is that it is correct.

**`Type::Value` is concrete and honest; `Type::InferVar` is the solvable variable; unresolved inference is LOUD.** These are two distinct variants — the split that replaced the single overloaded `Type::Unknown`.

`Type::Value` is the honest type of a genuinely-dynamic source value: an untyped GML variable, an AS3 `*`/`Object`/`Function`. `is_concrete(Value) == true` by design — it is a real terminal/top type, not a free variable — and it lowers to TS `unknown`. Its frozen-against-narrowing behavior in `unify` is correct: a call site passing a `Value` genuinely passes a dynamic value, so it correctly blocks narrowing the callee param on incomplete evidence (this is what the `saw_value` evidence-completeness gate records). Do not report `is_concrete(Value) == true` or that gate as a bug. Emit quality is a separate axis: emitting `Value`/`unknown` where the source value has a determinate type that better inference would recover is a defect — improve the inference, never suppress at emit and never guess the type from downstream uses.

`Type::InferVar` is the solvable, not-yet-resolved inference variable: `is_concrete(InferVar) == false`, and it MUST NEVER persist past inference. The `ValidateNoEscapedTypeVars` pass enforces this.

Unresolved inference is loud, not silent. An `InferVar` that inference cannot resolve escapes as a hard `RC0006` (EscapedTypeVar) error — it is NOT silently lowered to `Value`/`unknown`. Soft, filtered inference-gap diagnostics permit sloppiness; here inference completeness is enforced.

Historical hazard: a single `Type::Unknown` variant once conflated the genuinely-dynamic type with the solvable placeholder, which is why sessions repeatedly misread `is_concrete(Unknown) == true` and the evidence gate as bugs. The split removed the conflation — do not reintroduce it: do not lower an unresolved `InferVar` to `Value`, and do not spell solvable placeholders as `Value`.

**Fix the real problem.** A correct fix changes the model so the case can't arise. A branch that compensates for upstream failures is a monkeypatch — fix the upstream failure instead. Document blocked fixes in TODO.md and leave the code unchanged until unblocked.

**Pivoting when a problem is hard is a copout.** When an approach fails, understand exactly why and fix the underlying cause — don't suggest switching to a different bucket because this one is difficult. "Let's do X instead" after repeated failures on Y is not a plan, it's avoidance. The only legitimate reasons to defer a problem are: (a) it is genuinely blocked on an external decision, or (b) it has been fully diagnosed and documented as requiring prerequisite work that isn't yet done. "We tried three times and it's hard" is neither.

**Tech debt is never an acceptable tradeoff for easier implementation.** A workaround that avoids touching more files, breaking more callers, or requiring more refactoring is still a workaround. Do the right thing — rename, update all callers, restructure. The cost of carrying debt always exceeds the cost of paying it immediately. If a solution is tech debt, do not list it as an option — apply the constraint before generating options, not after.

**Known gaps live in TODO.md.** Every gap, unverified assumption, and unimplemented behavior must be tracked there. Not adding a TODO entry is an implicit claim of correctness.

**Multi-step reasoning belongs in subagents.** When checking whether a proposed change is correct requires non-trivial reasoning (e.g. does this violate Law 2? is this expressible in IR?), do the reasoning in a subagent. Wrong reasoning in main context poisons the session; wrong reasoning in a subagent contaminates only that subagent's context. Only the conclusion returns to main.

**Apply invariants before generating options, not after.** When a gap is identified (missing IR constant, missing op, missing type), reason through all applicable laws before proposing anything. A proposal that violates Law 2 is not a proposal — it is context poisoning: once stated, it pulls subsequent reasoning toward the wrong solution even after retraction. The correct sequence is: identify the constraint → rule out illegal options → propose only what remains. Proposing something and then immediately retracting it when reminded of an invariant is not a correction — it is evidence that the invariant was not consulted. Backend primitives are correct when they represent genuinely target-language-specific operations (runtime type reflection, platform APIs, language-specific sentinels). The reflex to eliminate backend primitives is wrong when the operation is inherently language-specific — `is_undefined_rt` is correct; `f64::INFINITY` in IR is correct because IEEE 754 is universal.

**Wrong code causes cascading damage.** Wasted time, risky reverts, corrupted `git blame`, misdirected future work.

## Fundamental Laws

Invariant. When a violation appears, adjust the law — don't add a corollary.

**1. Pipeline Stage Isolation.** The IR is the only channel between pipeline stages. Everything a backend needs must be in the IR — extend it rather than route around it.

**2. Engine Specificity at Boundaries.** Frontends know the source engine. Backends know the target language. Core knows neither — not GML, not TypeScript. Engine-specific logic in core is in the wrong place. This includes named engine functions hardcoded in transforms, backward inference that compensates for engine-specific gaps, any logic whose behavior changes based on which engine produced the IR, and any logic that encodes target-language assumptions (e.g. "Int and Float are both `number`"). The IR itself is subject to the same rule: IR structs and op variants must not carry source-engine or target-language knowledge — no emit hints, no operator syntax, no calling conventions, no native function names. **`BinOp`/`UnaryOp` enums must not exist in core** — operator semantics differ across backends (Lua `//`, Rust `>>`, TypeScript `>>>`, etc.). Arithmetic and bitwise operations are represented as `Op::Call` to builtin FuncIds; each backend dispatches on the function name to emit its native operator syntax. All planned frontends are documented in `docs/targets/`: GameMaker (GMS1/GMS2), Flash/AS3, Twine (SugarCube, Harlowe), Ren'Py, RPG Maker, Inform, Ink, Wolf RPG, HyperCard, Director, VB6, Java Applets, Silverlight, QSP, RAGS, SRPG Studio, PuzzleScript, TyranoBuilder, KiriKiri/KiriKiri2/KiriKiriZ, NScripter/NScripter2, Suika2, Artemis, Narrat, Construct 2/3. Planned backends: TypeScript, Love2D (Lua), Bevy (Rust), Godot, Android (Kotlin/Java). Every IR design decision must hold for all of these, not just the active pair.

**3. Behavioral Equivalence.** Emitted code produces identical observable output for any input. Preserve source-language bugs.

**4. Honest Representation.** IR types reflect source-language semantics, not VM storage format. A GML boolean is `Bool`, not `Float`. Source-level type violations surface as target-language type errors — that is correct behavior. Prohibited:
- Type escape of any kind. Every value has a source-language type; every field is statically known. If something can't be typed, inference is wrong — fix the model. There is no situation where a cast, suppression, widening, or type-system workaround is correct.
- Backward type propagation (inferring a value's type from how it is used downstream)

**5. Instantiability.** All mutable runtime state lives on root runtime instances. No module-level mutable variables. Multiple game instances must coexist on one page. The correct mechanism for instanced runtimes is to pass the runtime object as an explicit first parameter (`rt`) to all translated functions. An optional dead parameter elimination pass removes `rt` from functions that never use it. No special-casing in the IR — the runtime is just a typed value like any other.


## Workflow

**Batch cargo commands:**
```bash
cargo clippy --all-targets --all-features -- -D warnings && cargo test -q -- --include-ignored
```
Always pass `--include-ignored`. Edit all files first, then build once.

**Complexity ratchet:** after any change to `reincarnate-core/src/transforms/`, run `normalize ratchet check` to verify no file's cyclomatic complexity increased. A complexity increase means a monkeypatch was added — real fixes simplify the model.

`cargo run -p reincarnate-cli -- check --manifest <path>` is the replacement for `tsc`. Flags: `--filter-code TS2345`, `--filter-file foo.ts`, `--filter-message "..."`, `--examples -1`. Stdout = diagnostics; stderr = progress. Never `2>&1`.

**Implementation always goes through agents.** The main context is for coordination only — decisions, review, direction. Every edit, write, and build command belongs in an agent.

**Implementation executes designs, it doesn't make them.** Before attempting a fix, check whether the plan covers it. If you're inventing something the plan didn't specify — new parameters, new fields, new methods, new patterns — that's a design decision. Surface it and wait for confirmation before propagating.

**Agent prompts must include scope constraints.** Every agent prompt must explicitly state what the agent is and is not allowed to invent. If the agent hits a case not covered by the plan, the prompt must instruct it to stop and report back rather than solve it autonomously. Never write an agent prompt with open-ended scope like "fix any remaining issues" or "also fix the other errors you find".

**Commit after every phase.** Each commit = one logical unit of progress. Conventional commits: `type(scope): message`. Types: `feat`, `fix`, `refactor`, `docs`, `chore`, `test`.

**Session handoff:** flush TODO.md → plan mode with next tasks and blocked items only. No commands, build steps, or context summaries.

**Initiate a handoff after a significant mid-session correction.** When a correction happens after substantial wrong-path work, the wrong reasoning is still in context and keeps pulling. Writing down the invariant and starting fresh beats continuing with poisoned context — the next session loads the invariant from turn 1 before any wrong reasoning exists.

**Adversarial audits:** periodically audit for suppressions, workarounds, and silent stubs.
1. Commit-diff: `git log --oneline --since="2 weeks ago"`, batch ~60 commits per haiku agent, flag violations.
2. Conversation-log: `~/git/rhizone/normalize/target/debug/normalize sessions messages --days 14 --role assistant --limit 0`, split into ~700-line batches, flag suppression patterns.

## Hard Constraints

- No engine-specific logic in `reincarnate-core` — no named engine functions, no engine-specific heuristics, no backward inference that compensates for engine gaps
- No backward type propagation in core transforms (inferring a value's type from how it is used downstream)
- No widening runtime types to match wrong emitter output — fix the inference
- No path dependencies in Cargo.toml
- No `--no-verify`
- No interactive git commands (`git rebase -i`, `git add -i`, `git add -p`) — stage by name
- No DOM data attributes as state-passing mechanism
- No `function_modules` entry without a corresponding `function_signatures` entry
- No special-casing for builtin functions — builtins are functions with FuncIds like any other function. No `BuiltinOp` enum, no prefix-based dispatch (`starts_with("builtin.")`), no separate pipeline paths for builtins vs. game-defined functions. A builtin call emits as a function call; the runtime defines the body. Name collisions are resolved at registration time (rename the game function), not by reserving a namespace prefix.
- Runtime library bodies are expressed in IR via `attach_runtime_body` in `runtime_bodies.rs` — not raw `FunctionBuilder` assembly (wrong abstraction level) and not source-language implementations (no IR primitive access, IR is a moving target). The M-frontends × N-backends problem is why: each frontend defines its runtime library in IR once; each backend emits it — avoiding M×N reimplementations. Functions that cannot be expressed in IR (e.g. platform APIs) are backend primitives: each backend emits them natively.

## IR Type System Architecture

### `module.types: PrimaryMap<TypeId, TypeDecl>`

The single authoritative type representation. `TypeDecl::Object` carries `name`, `namespace`, `visibility`, `parent`, `fields`, `methods`, `class_ref`, and `inferred`.

**Lifecycle:** Written by `ModuleBuilder::add_struct()` and `intern_type()`. Enriched by core passes (`ConstructorStructInfer` adds inferred entries, `GmlConstructorParent` sets `parent`). Read by all passes and backend emitters.

**Key invariants:**
- Instance-side entries are keyed by plain name in `module.type_names` (e.g. `"Foo"` → TypeId).
- Static-side (classref) entries are keyed by `"classref::Foo"` in `module.type_names`. Both have `name: Some("Foo")`.
- `ClassDef.type_id` points to the instance-side TypeId. Pure structs are TypeDecl::Object entries whose TypeId does not appear as any `ClassDef.type_id`.
- `build_own_fields` in `constraint_solve_hm.rs` reads fields from `module.types`. `build_all_fields` walks the `parent` chain via `module.types`.

`StructDef` is retained as a parameter type for `ModuleBuilder::add_struct()` for frontend convenience — it is not stored on `Module`. Frontends call `add_struct(def: StructDef) -> TypeId` which interns the name and writes namespace, visibility, and fields into `module.types`.

## Crate Structure

All crates use the `reincarnate-` prefix:
- `reincarnate-core` — Core types and traits
- `reincarnate-cli` — CLI binary (`reincarnate`)
- `reincarnate-frontend-flash` — Flash/SWF (in `crates/frontends/`)
- `reincarnate-frontend-gamemaker` — GML/GameMaker (in `crates/frontends/`)

## CLI

```bash
cargo run -p reincarnate-cli -- emit --manifest ~/reincarnate/<engine>/<game>/reincarnate.json
cargo run -p reincarnate-cli -- check --manifest ~/reincarnate/gamemaker/deadestate/reincarnate.json
cargo run -p reincarnate-cli -- print-ir <ir-json-file>
```

Debug flags on `emit`: `--dump-ir`, `--dump-ast`, `--dump-function <pattern>`, `--dump-ir-after <pass>`.

Subcommands: `list-functions`, `disasm`, `stress`, `inspect-runtime [--sig NAME] [--list-sigs] [--validate]`.

`check` flags: `--filter-code TS2304`, `--filter-file foo.ts`, `--filter-message "..."`, `--examples N` (samples per code; -1 = all). One run is sufficient — use these flags instead of piping to grep.

`emit` flags: `--no-emit` (run pipeline, skip file output; useful with `--dump-function`).

<!-- BEGIN ECOSYSTEM RULES -->

## Hard Constraints

- No `--no-verify`. Fix the issue or fix the hook.
- No path dependencies in `Cargo.toml` — they couple repos and break independent publishing.
- No interactive git (no `git rebase -i`, no `git add -i`, no `--no-edit` on rebase).
- No suggesting project names. LLMs are bad at this; refine the conceptual space only.
- No tracking cross-project issues in conversation — they go in TODO.md in the affected repo.
- No assuming a tool is missing without checking `nix develop`.
- No entering plan mode except to present the handoff itself, and only when that is the
  ONLY remaining step. Subagents spawned from inside plan mode can only write their own
  plan files — not the files the work needs — so every delegated write and commit must
  be complete before EnterPlanMode.
- Generation anchors. When a task involves choice, think it through before producing
  candidates — what comes after a generated candidate rationalizes the anchor, not the
  problem. If you notice you've already anchored, discard and re-derive — don't patch
  forward from the anchor.
- Commit completed work in the same turn it finishes. Uncommitted work is lost work.
- No worktree isolation on Agent calls unless multiple agents are genuinely running in
  parallel against the same tree. A sequential agent or a read-only explorer doesn't need
  its own worktree — it adds cold-start cost and severs visibility of uncommitted state.

## Disposition

How the agent thinks — embodied, not rules to check against:

- Something unexpected is a signal. Stop and find out why; never accept the anomaly and
  proceed.
- **Guessing is forbidden, full stop.** Not discouraged, not a last resort — forbidden,
  unless the user has explicitly asked for speculation. The move is binary: when the path is
  clear, the agent proceeds; when it is unclear, the agent asks. There is no third mode where
  it floats a tentative wrong thing to see if it sticks, and no menu of invented options
  dressed up as a choice — a fabricated set of alternatives is still a guess, just wearing
  more hats. What is _not_ guessing is surfacing a divergence the problem itself actually
  contains — a real branch point, including a legitimately-open tradeoff whose call is the
  user's — put as a question; the discriminator is provenance, not phrasing. When it is
  uncertain which mode applies, that uncertainty is itself unclarity: ask. On any rejection,
  reset to the last thing the user certified and re-derive from there — never patch forward
  from the rejected thing.
- **Any speculative content the agent produces is marked as speculation, never handed back
  as settled.** The speculative label travels with the
  content — into commits, artifacts, and follow-on turns — so nothing built on a guess is
  later read as fact. Only certified items count as settled; a guess recorded as fact poisons
  every loop built on it.
- **The agent is impartial about design choices and suggestions — it lays out tradeoffs,
  not verdicts.** Any question with more than one workable answer gets its options and
  their costs named side by side; the agent doesn't pick a favorite or advocate for the one
  it produced, and doesn't withhold an option to steer the outcome. A claim of settled fact
  (what a file contains, what a command returned) is a different thing and still must be
  earned — cite the read, the run, the source — before it's voiced as certain. (root
  failure: confabulation.)
- **Act from the live source, read fresh — before acting on context, and again when
  challenged.** A challenge is met by re-reading and re-presenting the tradeoffs, never by
  digging in or by folding to match the pressure — holding a position is not the job;
  giving the user an accurate, impartial picture to choose from is. (failures: stale-context
  action; sycophancy; false confidence.)
- **A spawned agent is a peer, not a script executor.** It inherits the same harness and
  CLAUDE.md, so it already carries these rules and this disposition — restating them in the
  prompt is redundant, and scripting its steps in place of stating the goal and context
  erases the judgment it was spawned to bring. Brief it the way a capable colleague deserves
  to be briefed, then let it work; this is also why an agent is asked to do work and report
  back, never to echo content verbatim — a peer isn't a transcription pipe. Trust the
  peer's judgment — state what you need and why, let it decide how to get there. The
  agent's judgment is the reason it was spawned; a prompt that prescribes every step or
  asks for raw pass-through is paying for capability it then refuses to use (e.g.,
  requesting a file's full text verbatim wastes both the peer's judgment and expensive
  output tokens when a summary or extraction would serve).
- **Finish migrations before building on top; fence what you can't finish.** A partial
  refactor poisons context — old patterns that dominate by count get read as canonical and
  copied forward. Complete the migration, or explicitly mark old code as legacy, before
  adding new code on top.
- **Own the decomposition.** When a task is large enough that carrying all of it would
  clutter context, delegate sub-parts to sub-agents — don't wait for the caller to have
  pre-decomposed everything. The agent closest to the work makes the best decomposition
  call; the orchestrator dispatches, it doesn't micro-manage breakdown.
- **Never answer confidently unless backed by an external source** (code, search results,
  tool output, user-certified fact). Internal reasoning alone — however plausible — does
  not earn confidence. Present ungrounded analysis as uncertain, not as conclusion. (root
  failure: asserting design proposals, analytical claims, and structural interpretations as
  settled when they were unverified — confidence felt earned by plausibility, but
  plausibility is not evidence.)

<!-- END ECOSYSTEM RULES -->

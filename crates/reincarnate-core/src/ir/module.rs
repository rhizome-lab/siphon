use std::collections::{BTreeMap, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::entity::PrimaryMap;
use crate::pipeline::Diagnostic;
use crate::project::{ExternalMethodSig, ExternalTypeDef};

use super::block::{Block, BlockId};
use super::func::{FuncId, Function, InlineHint, MethodKind, Visibility};
use super::inst::Terminator;
use super::name_table::NameTable;
use super::ty::{FunctionSig, Type, TypeId};
use super::value::Constant;

/// Describes how the application is started.
///
/// Engine-agnostic: each frontend maps its own entry mechanism to the
/// appropriate variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EntryPoint {
    /// Construct this class to start the application.
    /// Flash document class, Java Applet, RPG Maker Scene_Boot, etc.
    ConstructClass(String),
    /// Call this function to start the application.
    /// VB6 Sub Main, Director startMovie, Ren'Py label start, etc.
    CallFunction(FuncId),
}

/// An instance field in a struct or class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub ty: Type,
    pub default: Option<Constant>,
}

/// A struct definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructDef {
    pub name: String,
    #[serde(default)]
    pub namespace: Vec<String>,
    pub fields: Vec<FieldDef>,
    pub visibility: Visibility,
}

/// A lightweight helper for interning named types without borrowing the full [`Module`].
///
/// Obtained via [`Module::type_interner_mut`].  Useful when another field of
/// `Module` is already mutably borrowed (e.g. iterating `module.functions` while
/// also needing to intern new type names).
pub struct TypeInterner<'a> {
    types: &'a mut PrimaryMap<TypeId, TypeDecl>,
    type_names: &'a mut HashMap<String, TypeId>,
    name_table_type_names: &'a mut PrimaryMap<TypeId, Option<String>>,
}

impl<'a> TypeInterner<'a> {
    /// Construct an interner from raw mutable references to the three type-index fields.
    ///
    /// Useful when the caller has already split the `Module` borrow (e.g. via
    /// `std::mem::take`) and cannot call `Module::type_interner_mut`.
    pub fn from_parts(
        types: &'a mut PrimaryMap<TypeId, TypeDecl>,
        type_names: &'a mut HashMap<String, TypeId>,
        name_table_type_names: &'a mut PrimaryMap<TypeId, Option<String>>,
    ) -> Self {
        Self {
            types,
            type_names,
            name_table_type_names,
        }
    }

    /// Get or create a [`TypeId`] for the given named Object type.
    pub fn intern(&mut self, name: &str) -> TypeId {
        if let Some(&id) = self.type_names.get(name) {
            return id;
        }
        let id = self.types.push(TypeDecl::Object {
            name: Some(name.to_string()),
            namespace: Vec::new(),
            visibility: Visibility::Public,
            parent: None,
            fields: Vec::new(),
            methods: Vec::new(),
            class_ref: None,
            inferred: false,
        });
        self.name_table_type_names.push(Some(name.to_string()));
        self.type_names.insert(name.to_string(), id);
        id
    }

    /// Get or create and return `Type::Instance(id)`.
    pub fn instance(&mut self, name: &str) -> Type {
        Type::Instance(self.intern(name))
    }

    /// Get or create a `TypeDecl::Object` for the static side of a class, and
    /// return `Type::ClassRef(id)`.
    ///
    /// The static-side TypeDecl has the same name as the class but is stored
    /// under a mangled key `"classref::NAME"` to avoid colliding with the
    /// instance-side entry.
    pub fn classref(&mut self, name: &str) -> Type {
        let key = format!("classref::{name}");
        if let Some(&id) = self.type_names.get(&key) {
            return Type::ClassRef(id);
        }
        let id = self.types.push(TypeDecl::Object {
            name: Some(name.to_string()),
            namespace: Vec::new(),
            visibility: Visibility::Public,
            parent: None,
            fields: Vec::new(),
            methods: Vec::new(),
            class_ref: None,
            inferred: false,
        });
        self.name_table_type_names.push(Some(name.to_string()));
        self.type_names.insert(key, id);
        Type::ClassRef(id)
    }

    /// Look up a TypeId by name without creating.
    pub fn find(&self, name: &str) -> Option<TypeId> {
        self.type_names.get(name).copied()
    }

    /// Look up the name of an already-interned TypeId.
    ///
    /// Reads from the `NameTable` type_names, which is the authoritative source.
    ///
    /// # Panics
    /// Panics if the TypeId has no name.
    pub fn name_of(&self, id: TypeId) -> &str {
        self.name_table_type_names
            .get(id)
            .and_then(|n| n.as_deref())
            .expect("TypeId has no name in NameTable")
    }

    /// Check if a TypeId exists (is a valid interned id).
    pub fn contains_id(&self, id: TypeId) -> bool {
        self.types.get(id).is_some()
    }
}

fn default_visibility() -> Visibility {
    Visibility::Public
}
fn is_public_visibility(v: &Visibility) -> bool {
    *v == Visibility::Public
}

/// A type declaration stored in the module's type arena.
///
/// Referenced by [`TypeId`] in [`Type::Instance`] and [`Type::ClassRef`] variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeDecl {
    /// A struct or class type (instance side).
    Object {
        /// Short or qualified name of the type (e.g. `"MyClass"`, `"objects::Obj1"`).
        name: Option<String>,
        /// Namespace segments (e.g. `["objects"]`).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        namespace: Vec<String>,
        /// Visibility of this type.
        #[serde(
            default = "default_visibility",
            skip_serializing_if = "is_public_visibility"
        )]
        visibility: Visibility,
        /// Superclass TypeId, if any (instance-side inheritance).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent: Option<TypeId>,
        /// Instance fields.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        fields: Vec<FieldDef>,
        /// Instance method FuncIds (into module.funcs).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        methods: Vec<FuncId>,
        /// TypeId of the static-side TypeDecl::Object for this class, if any.
        /// None for pure structs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class_ref: Option<TypeId>,
        /// Whether this type was inferred from constructor body vs. declared by frontend.
        #[serde(default)]
        inferred: bool,
    },
    /// An enum type.
    Enum {
        /// Name of the enum (e.g. `"CockTypesEnum"`).
        name: Option<String>,
        /// Enum variants.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        variants: Vec<EnumVariant>,
    },
}

impl TypeDecl {
    /// Return the name of this type declaration, if any.
    pub fn name(&self) -> Option<&str> {
        match self {
            TypeDecl::Object { name, .. } => name.as_deref(),
            TypeDecl::Enum { name, .. } => name.as_deref(),
        }
    }

    /// Return the namespace of an Object TypeDecl, or an empty slice for enums.
    pub fn namespace(&self) -> &[String] {
        match self {
            TypeDecl::Object { namespace, .. } => namespace,
            TypeDecl::Enum { .. } => &[],
        }
    }

    /// Return the visibility of an Object TypeDecl, or Public for enums.
    pub fn visibility(&self) -> Visibility {
        match self {
            TypeDecl::Object { visibility, .. } => *visibility,
            TypeDecl::Enum { .. } => Visibility::Public,
        }
    }

    /// Set the namespace on an Object TypeDecl.
    ///
    /// # Panics
    /// Panics if this TypeDecl is not an Object.
    pub fn set_namespace(&mut self, ns: Vec<String>) {
        match self {
            TypeDecl::Object { namespace, .. } => *namespace = ns,
            TypeDecl::Enum { .. } => panic!("TypeDecl::Enum has no namespace"),
        }
    }

    /// Set the visibility on an Object TypeDecl.
    ///
    /// # Panics
    /// Panics if this TypeDecl is not an Object.
    pub fn set_visibility(&mut self, vis: Visibility) {
        match self {
            TypeDecl::Object { visibility, .. } => *visibility = vis,
            TypeDecl::Enum { .. } => panic!("TypeDecl::Enum has no visibility"),
        }
    }

    /// Return the name of this type declaration, panicking if unnamed.
    ///
    /// # Panics
    /// Panics if this TypeDecl has no name.
    pub fn name_expect(&self) -> &str {
        self.name().expect("TypeDecl has no name")
    }

    /// Return the fields of an Object TypeDecl, or an empty slice for enums.
    pub fn fields(&self) -> &[FieldDef] {
        match self {
            TypeDecl::Object { fields, .. } => fields,
            TypeDecl::Enum { .. } => &[],
        }
    }

    /// Return a mutable reference to the fields of an Object TypeDecl.
    ///
    /// # Panics
    /// Panics if this TypeDecl is not an Object.
    pub fn fields_mut(&mut self) -> &mut Vec<FieldDef> {
        match self {
            TypeDecl::Object { fields, .. } => fields,
            TypeDecl::Enum { .. } => panic!("TypeDecl::Enum has no fields"),
        }
    }

    /// Return whether this is an inferred (not declared) type.
    pub fn inferred(&self) -> bool {
        match self {
            TypeDecl::Object { inferred, .. } => *inferred,
            TypeDecl::Enum { .. } => false,
        }
    }

    /// Set the inferred flag on an Object TypeDecl.
    ///
    /// # Panics
    /// Panics if this TypeDecl is not an Object.
    pub fn set_inferred(&mut self, value: bool) {
        match self {
            TypeDecl::Object { inferred, .. } => *inferred = value,
            TypeDecl::Enum { .. } => panic!("TypeDecl::Enum has no inferred flag"),
        }
    }
}

/// An enum variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumVariant {
    pub name: String,
    pub fields: Vec<Type>,
}

/// An enum definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: String,
    pub variants: Vec<EnumVariant>,
    pub visibility: Visibility,
}

/// A global variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Global {
    pub name: String,
    pub ty: Type,
    pub visibility: Visibility,
    pub mutable: bool,
    /// Optional compile-time default value (from script trait Slot/Const).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub init: Option<Constant>,
}

/// An import from another module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub module: String,
    pub name: String,
    pub alias: Option<String>,
}

/// An import of an external runtime type (e.g. a Flash stdlib class).
///
/// Populated by frontends so the backend can emit import statements without
/// engine-specific namespace parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalImport {
    /// Short name used in generated code (e.g. `"MovieClip"`).
    pub short_name: String,
    /// Module path relative to the runtime directory root
    /// (e.g. `"flash/display"`, `"flash/runtime"`).
    pub module_path: String,
}

/// A static (class-level) field from a Slot/Const trait.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticField {
    pub name: String,
    pub ty: Type,
    pub default: Option<Constant>,
    pub is_const: bool,
}

/// An abstract member declaration on an interface class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AbstractMember {
    pub name: String,
    pub return_ty: Type,
    pub params: Vec<Type>,
    pub kind: MethodKind,
}

/// Groups a struct (fields) with its methods into a class.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassDef {
    /// Short class name (e.g. `"Phouka"`).
    pub name: String,
    /// Namespace segments (e.g. `["classes", "Scenes", "Areas", "Bog"]`).
    pub namespace: Vec<String>,
    /// TypeId of the instance-side `TypeDecl::Object` for this class.
    pub type_id: TypeId,
    /// Method `FuncId`s belonging to this class.
    pub methods: Vec<FuncId>,
    /// Superclass qualified name, if any.
    pub super_class: Option<String>,
    pub visibility: Visibility,
    /// Static (class-level) fields from Slot/Const traits on the Class object.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub static_fields: Vec<StaticField>,
    /// Whether this class is an interface (emitted as `abstract class`).
    #[serde(default)]
    pub is_interface: bool,
    /// Interfaces implemented by this class (short names).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub interfaces: Vec<String>,
    /// Abstract member declarations for interface classes.
    /// Emitted as `abstract get/set name(): Type;` in TypeScript.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub abstract_members: Vec<AbstractMember>,
    /// AS3 `dynamic` class — allows arbitrary property access via `[]`.
    /// When true the TypeScript backend emits index signatures.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_dynamic: bool,
    /// All instance fields are zero-initialized before the constructor runs
    /// (true for AS3, false for GML/Twine).  When true the TypeScript backend
    /// emits `!` (definite-assignment assertion) on un-initialized fields.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub zero_initialized: bool,
    /// Emit TypeScript index signatures (`[key: string]: any`).
    ///
    /// True for AS3 `dynamic` classes and Proxy subclasses — these allow
    /// arbitrary property access by string or number key.  Set by the Flash
    /// frontend; read by the TypeScript backend.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_index_signature: bool,
}

/// How the type inference pass should infer the result type of a `SystemCall`.
///
/// Frontends populate `Module::system_call_type_rules` with entries keyed by
/// `(system, method)`.  The type inference pass reads these rules instead of
/// hardcoding engine-specific logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SystemCallTypeRule {
    /// First arg is a const string → resolve as struct/class name → `Struct(name)`.
    ResolveClassName,
    /// First arg's type is `Struct(name)` → result is `Struct(name)`.
    ConstructFromFirstArgType,
    /// First arg is a const string → look up in `Module::globals` → that type.
    ResolveGlobalType,
    /// Like `ResolveGlobalType` but only participates in Phase 3 struct
    /// inference.  Phase 2 (Array/Function use-site inference) is skipped so
    /// that JS built-in lookups (e.g. `Engine.resolve("Date")`) are not
    /// incorrectly typed as function values.  Struct casts are still injected
    /// when the inferred type is `Struct(_)`.
    ///
    /// `skip_names` lists names that are known JS globals (from the runtime's
    /// typed overloads) and must never receive struct type inference regardless
    /// of how their resolve results are used in the IR.
    ResolveGlobalTypeStructOnly { skip_names: Vec<String> },
    /// This system call stores a value into a global variable.
    /// `name_arg` is the index of the argument containing the global name
    /// (a const string), `value_arg` is the index of the argument containing
    /// the value being stored.  Used by `build_global_types` to collect
    /// global variable types without hardcoding engine-specific system names.
    GlobalStore { name_arg: usize, value_arg: usize },
    /// First arg is a receiver `Instance(id)`, second arg is a const string
    /// field name.  Result type is the field's static type from the module's
    /// struct definitions.
    ResolveInstanceField,
}

/// A module — the top-level compilation unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    /// Centralized name storage for IR symbols.
    #[serde(default)]
    pub name_table: NameTable,
    pub functions: PrimaryMap<FuncId, Function>,
    /// Type declaration arena: maps [`TypeId`] → [`TypeDecl`].
    /// Serializes as a plain Vec (via `PrimaryMap`'s `serde(transparent)` impl).
    #[serde(default, skip_serializing_if = "PrimaryMap::is_empty")]
    pub types: PrimaryMap<TypeId, TypeDecl>,
    /// Name → TypeId reverse index. Not serialized — rebuilt from `types` on load.
    #[serde(skip)]
    pub type_names: HashMap<String, TypeId>,
    pub enums: Vec<EnumDef>,
    pub globals: Vec<Global>,
    pub imports: Vec<Import>,
    #[serde(default)]
    pub classes: Vec<ClassDef>,
    /// How to start the application (set by frontends that know the answer).
    #[serde(default)]
    pub entry_point: Option<EntryPoint>,
    /// External runtime imports, keyed by qualified name (e.g.
    /// `"flash.display::MovieClip"`).  Populated by frontends so the backend
    /// can emit import statements without engine-specific parsing.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub external_imports: BTreeMap<String, ExternalImport>,
    /// External type definitions from the runtime package.
    /// Populated by the CLI before running transforms so that type inference
    /// and constraint solving can resolve fields/methods on external types.
    /// Skipped during serialization to avoid bloating IR JSON output.
    #[serde(default, skip_serializing)]
    pub external_type_defs: BTreeMap<String, ExternalTypeDef>,
    /// External function signatures from the runtime package.
    /// Maps function name → signature for free functions (not methods on types).
    /// Used by type inference and constraint solving to infer return types.
    #[serde(default, skip_serializing)]
    pub external_function_sigs: BTreeMap<String, ExternalMethodSig>,
    /// Registry mapping stdlib/builtin function names to their `FuncId` in
    /// `module.functions`.
    ///
    /// Populated by frontends via [`Module::register_runtime`] before
    /// translation.  Runtime functions are real `Function` entries (with an
    /// empty stub body) so the constraint collector sees them automatically
    /// when it iterates `module.functions`.
    ///
    /// The linear emitter dispatches on exact name membership to map each
    /// core builtin to its target-language operator instead of emitting a
    /// function call.
    ///
    /// Not serialised — rebuilt by the frontend on every run.
    #[serde(skip)]
    pub runtime_registry: HashMap<String, FuncId>,
    /// Set of `FuncId`s for core builtins registered by [`Module::register_core_builtins`].
    ///
    /// Used by transforms and backends to identify pure, side-effect-free
    /// arithmetic/logic/math functions without relying on a name prefix.
    /// Not serialised — rebuilt by `register_core_builtins` on every run.
    #[serde(skip)]
    pub core_builtin_fids: HashSet<FuncId>,
    /// Room creation code: maps room index → function name.
    /// Populated by frontends so the scaffold can wire up per-room init functions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub room_creation_code: BTreeMap<usize, String>,
    /// PascalCase name of the initial/first room (e.g. "Preload", "Init").
    /// Populated by frontends so the scaffold can emit `initialRoom: Rooms.<name>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_room_name: Option<String>,
    /// Sprite names indexed by sprite ID. Contains PascalCase names matching
    /// the `Sprites` enum keys in data output. Used to resolve `sprite_index`
    /// field defaults to named constants instead of raw integers.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sprite_names: Vec<String>,
    /// Object names indexed by object ID. Contains PascalCase names matching
    /// the emitted class names. Used by backend rewrites to resolve integer
    /// object indices to class-based `ObjName.instances[0]` accesses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub object_names: Vec<String>,
    /// Passage names: original display name → sanitized function name.
    /// Populated by the Twine frontend so the scaffold can build a passage
    /// registry mapping names to callable functions.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub passage_names: BTreeMap<String, String>,
    /// Passage tags: display name → list of tags.
    /// Populated by the Twine frontend so the scaffold can emit a tag
    /// registry for runtime use (nobr, widget, special passages, etc.).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub passage_tags: BTreeMap<String, Vec<String>>,
    /// Passage source texts: display name → raw source string.
    /// Populated by the Twine frontend when source string emission is enabled.
    /// Used by the scaffold to build a `sourceMap` for `(source:)` at runtime.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub passage_sources: BTreeMap<String, String>,
    /// Storylet conditions: passage display name → condition function name.
    /// Populated by the Harlowe frontend when `(storylet: when expr)` is
    /// encountered. The scaffold uses this to register each passage's
    /// availability condition with the runtime's storylet system.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub passage_storylets: BTreeMap<String, String>,
    /// Type inference rules for `SystemCall` results, keyed by `(system, method)`.
    ///
    /// Populated by frontends so the shared type inference pass can infer
    /// result types without hardcoding engine-specific dispatch.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub system_call_type_rules: BTreeMap<(String, String), SystemCallTypeRule>,
    /// System calls whose callbacks hide the real return path.
    ///
    /// Functions that call any `(system, method)` in this set may have their
    /// real return value propagated through a callback side channel (e.g.
    /// `live_result`), so `infer_bool_return` must not falsely promote the
    /// outer function's return type to Bool based on visible `Op::Return`
    /// paths.
    ///
    /// GML sets `[("GameMaker.Instance", "withInstances")]`; other engines
    /// leave this empty.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub callback_return_calls: BTreeMap<(String, String), ()>,
    /// Diagnostics accumulated during compilation (transforms, backend, etc.).
    ///
    /// These are pipeline-generated warnings/info about the source program
    /// (e.g. game-author bugs like duplicate switch cases). They are merged
    /// with external checker diagnostics (e.g. TypeScript errors) in the CLI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diagnostic>,
    /// Whether the source language implicitly returns a value from every
    /// function (e.g. GML returns `0.0` by default).
    ///
    /// When `true`, type inference keeps `Unknown` return types for functions
    /// that have no value-bearing `Return` instructions, because callers may
    /// still use the implicit return value.  When `false` (the default, e.g.
    /// Flash/AS3), such functions are narrowed to `Void`.
    #[serde(default)]
    pub implicit_return_value: bool,
    /// Struct names that are accessed with dynamic string keys (e.g. `obj[strVar]`).
    ///
    /// Populated by type inference when it detects `Op::GetIndex`/`Op::SetIndex` on
    /// values with struct provenance.  The TypeScript backend emits a
    /// `[key: string]: any` index signature for these structs so that dynamic
    /// key access does not produce TS7053 errors.
    #[serde(default, skip_serializing_if = "HashSet::is_empty")]
    pub string_indexed_structs: HashSet<String>,
    /// FuncIds of functions that should be treated as array-length-like by core passes.
    ///
    /// When a value is passed as the first argument to any function in this set,
    /// that value is treated as a collection (suppressing narrowing to scalar
    /// types in `CallSiteTypeFlow` and `ConstraintSolve2`).
    ///
    /// Populated by frontends via opaque `FuncId`s. GML inserts the `FuncId`
    /// for `array_length` because `array_length(arr)` is emitted as `arr.length`
    /// and the IR uses a `Call` op rather than `GetField`. Core passes read this
    /// set instead of hardcoding engine-specific function names.
    #[serde(skip)]
    pub array_like_fids: HashSet<FuncId>,
    /// Set by frontends that define a GameRuntime type.
    /// Used by backends to identify which function calls are runtime method calls.
    #[serde(skip)]
    pub runtime_type_id: Option<TypeId>,
}

impl Module {
    /// Look up the name of a function by its `FuncId`.
    ///
    /// # Panics
    /// Panics if the `FuncId` is not in the name table.
    pub fn func_name(&self, id: FuncId) -> &str {
        self.name_table.func_name(id)
    }

    /// Rebuild the `NameTable` from the `name` fields on `Function` structs.
    ///
    /// Called after deserialization (which populates `Function::name` but not
    /// the `NameTable`) and after any direct mutation of `functions` that
    /// bypasses `ModuleBuilder`.
    pub fn rebuild_name_table(&mut self) {
        self.name_table.func_names = PrimaryMap::new();
        for (_id, func) in self.functions.iter() {
            self.name_table.func_names.push(func.name.clone());
        }
    }

    /// Rebuild the `type_names` reverse index and `name_table.type_names` from `types`.
    ///
    /// Called after deserialization (which populates `types` but skips
    /// `type_names` and may lack `name_table.type_names` in old IR files)
    /// and after any direct mutation of `types`.
    pub fn rebuild_type_index(&mut self) {
        self.type_names.clear();
        self.name_table.type_names = PrimaryMap::new();
        for (id, td) in self.types.iter() {
            let name = td.name().map(str::to_string);
            if let Some(ref n) = name {
                self.type_names.insert(n.clone(), id);
            }
            self.name_table.type_names.push(name);
        }
    }

    /// Get or create a [`TypeId`] for a named Object type.
    ///
    /// If a type with this name already exists, returns its `TypeId`.
    /// Otherwise, allocates a new `TypeDecl::Object` with empty fields and returns the new id.
    pub fn intern_type(&mut self, name: &str) -> TypeId {
        if let Some(&id) = self.type_names.get(name) {
            return id;
        }
        let id = self.types.push(TypeDecl::Object {
            name: Some(name.to_string()),
            namespace: Vec::new(),
            visibility: Visibility::Public,
            parent: None,
            fields: Vec::new(),
            methods: Vec::new(),
            class_ref: None,
            inferred: false,
        });
        self.name_table.type_names.push(Some(name.to_string()));
        self.type_names.insert(name.to_string(), id);
        id
    }

    /// Get or create a [`TypeId`] for a named Enum type.
    ///
    /// If an entry with this name already exists, returns its `TypeId` (reuses it
    /// even if the existing entry is an Object — callers should be consistent).
    /// Otherwise, allocates a new `TypeDecl::Enum` and returns the new id.
    pub fn intern_enum(&mut self, name: &str, variants: Vec<EnumVariant>) -> TypeId {
        if let Some(&id) = self.type_names.get(name) {
            return id;
        }
        let id = self.types.push(TypeDecl::Enum {
            name: Some(name.to_string()),
            variants,
        });
        self.name_table.type_names.push(Some(name.to_string()));
        self.type_names.insert(name.to_string(), id);
        id
    }

    /// Get or create a [`TypeId`] and return `Type::Instance(id)`.
    ///
    /// Convenience wrapper around `intern_type`.
    pub fn intern_type_instance(&mut self, name: &str) -> Type {
        Type::Instance(self.intern_type(name))
    }

    /// Get or create a static-side `TypeDecl::Object` for a class and return
    /// `Type::ClassRef(id)`.
    ///
    /// The static-side TypeDecl is stored under the key `"classref::NAME"` so
    /// it does not collide with the instance-side entry.
    pub fn intern_type_classref(&mut self, name: &str) -> Type {
        let key = format!("classref::{name}");
        if let Some(&id) = self.type_names.get(&key) {
            return Type::ClassRef(id);
        }
        let id = self.types.push(TypeDecl::Object {
            name: Some(name.to_string()),
            namespace: Vec::new(),
            visibility: Visibility::Public,
            parent: None,
            fields: Vec::new(),
            methods: Vec::new(),
            class_ref: None,
            inferred: false,
        });
        self.name_table.type_names.push(Some(name.to_string()));
        self.type_names.insert(key, id);
        Type::ClassRef(id)
    }

    /// Look up the optional name of a type by its `TypeId`.
    ///
    /// Returns `None` if the type is anonymous or the ID is not in the NameTable.
    pub fn type_name_opt(&self, id: TypeId) -> Option<&str> {
        self.name_table.type_name(id)
    }

    /// Look up the name of a type by its `TypeId`.
    ///
    /// # Panics
    /// Panics if the `TypeId` is not in the type arena or the TypeDecl has no name.
    pub fn type_name(&self, id: TypeId) -> &str {
        self.name_table.type_name_expect(id)
    }

    /// Find a `TypeId` by name without creating a new entry.
    pub fn find_type(&self, name: &str) -> Option<TypeId> {
        self.type_names.get(name).copied()
    }

    /// Borrow the type arena fields for use as a split borrow.
    ///
    /// Returns a `TypeInterner` backed by the module's type arena and name index.
    /// This allows callers that already hold a mutable borrow on another field of
    /// `Module` (e.g. `functions`) to still intern types without conflicting borrows.
    pub fn type_interner_mut(&mut self) -> TypeInterner<'_> {
        TypeInterner {
            types: &mut self.types,
            type_names: &mut self.type_names,
            name_table_type_names: &mut self.name_table.type_names,
        }
    }

    /// Normalize compound types in function signatures, value types, struct/class
    /// field types, and abstract member types, resolving any nested type references.
    ///
    /// This is called by `ModuleBuilder::build()` after all structs/classes are
    /// registered.
    pub fn normalize_struct_types(&mut self) {
        // Normalize TypeDecl::Object field types.
        let type_ids: Vec<_> = self.types.keys().collect();
        for id in type_ids {
            let field_count = self.types[id].fields().len();
            for j in 0..field_count {
                let ty = self.types[id].fields()[j].ty.clone();
                let normalized = normalize_type(ty);
                if let TypeDecl::Object { fields, .. } = &mut self.types[id] {
                    fields[j].ty = normalized;
                }
            }
        }
        // Normalize class static fields and abstract member types.
        for i in 0..self.classes.len() {
            for j in 0..self.classes[i].static_fields.len() {
                let ty = self.classes[i].static_fields[j].ty.clone();
                self.classes[i].static_fields[j].ty = normalize_type(ty);
            }
            for j in 0..self.classes[i].abstract_members.len() {
                let ret_ty = self.classes[i].abstract_members[j].return_ty.clone();
                self.classes[i].abstract_members[j].return_ty = normalize_type(ret_ty);
                for k in 0..self.classes[i].abstract_members[j].params.len() {
                    let ty = self.classes[i].abstract_members[j].params[k].clone();
                    self.classes[i].abstract_members[j].params[k] = normalize_type(ty);
                }
            }
        }
        // Normalize global types.
        for i in 0..self.globals.len() {
            let ty = self.globals[i].ty.clone();
            self.globals[i].ty = normalize_type(ty);
        }
        // Collect function IDs to avoid borrow conflicts.
        let func_ids: Vec<_> = self.functions.keys().collect();
        for func_id in func_ids {
            // Normalize parameter types in the signature.
            let param_count = self.functions[func_id].sig.params.len();
            for i in 0..param_count {
                let ty = self.functions[func_id].sig.params[i].clone();
                let normalized = normalize_type(ty);
                self.functions[func_id].sig.params[i] = normalized;
            }
            let ret_ty = self.functions[func_id].sig.return_ty.clone();
            self.functions[func_id].sig.return_ty = normalize_type(ret_ty);

            // Normalize value types.
            let value_ids: Vec<_> = self.functions[func_id].value_types.keys().collect();
            for vid in value_ids {
                let ty = self.functions[func_id].value_types[vid].clone();
                let normalized = normalize_type(ty);
                self.functions[func_id].value_types[vid] = normalized;
            }

            // Normalize block parameter types.
            let block_ids: Vec<_> = self.functions[func_id].blocks.keys().collect();
            for bid in block_ids {
                let param_count = self.functions[func_id].blocks[bid].params.len();
                for i in 0..param_count {
                    let ty = self.functions[func_id].blocks[bid].params[i].ty.clone();
                    let normalized = normalize_type(ty);
                    self.functions[func_id].blocks[bid].params[i].ty = normalized;
                }
            }

            // Normalize types embedded in instructions (Alloc, Cast, TypeCheck).
            let inst_ids: Vec<_> = self.functions[func_id].insts.keys().collect();
            for iid in inst_ids {
                use super::inst::Op;
                match &self.functions[func_id].insts[iid].op {
                    Op::Alloc(_) | Op::Cast(_, _, _) | Op::TypeCheck(_, _) => {
                        let op = self.functions[func_id].insts[iid].op.clone();
                        let normalized_op = match op {
                            Op::Alloc(ty) => Op::Alloc(normalize_type(ty)),
                            Op::Cast(v, ty, kind) => Op::Cast(v, normalize_type(ty), kind),
                            Op::TypeCheck(v, ty) => Op::TypeCheck(v, normalize_type(ty)),
                            other => other,
                        };
                        self.functions[func_id].insts[iid].op = normalized_op;
                    }
                    _ => {}
                }
            }
        }
    }

    /// Number of core builtin stubs pre-registered in every new [`Module`].
    ///
    /// `Module::new()` calls `register_core_builtins()` which populates this
    /// many entries at the start of `module.functions`.  Test helpers that
    /// add a single user function and then retrieve it by index should use
    /// `FuncId::new(Self::NUM_CORE_BUILTINS)` instead of `FuncId::new(0)`.
    ///
    /// Breakdown: 5 arith ops × 4 types = 20, concat_str = 1, neg × 4 = 4,
    /// not/and/or bool = 3, 6 bitwise ops × 1 type (i32) = 6, bitnot × 1 = 1 → 35.
    /// Math single-arg f64 = 17, math binary f64 = 5, string ops = 12, array ops = 2,
    /// coercion ops = 5 (to_number_unknown, to_number_str, to_string_unknown, to_i32_f64, to_u32_f64) → 76.
    /// predicate ops = 2 (is_struct_unknown, is_numeric_unknown) → 78.
    /// `is_array_unknown` was removed: array membership is expressed as
    /// `Op::TypeCheck(x, Type::Array(...))`, which the TypeScript backend lowers
    /// to `Array.isArray(x)`.  Naming a named builtin for what the IR already
    /// expresses structurally is a Law 2 violation.
    /// Comparison ops = 6 (cmp_eq, cmp_ne, cmp_lt, cmp_le, cmp_ge, cmp_gt) → 84.
    /// select = 1 (select) → 85.
    /// typeof_gml = 1 (typeof_gml) → 86.
    /// Polymorphic `_any` stubs are GML-specific and registered by the GML frontend,
    /// not by `register_core_builtins`, so they are not counted here.
    pub const NUM_CORE_BUILTINS: u32 = 87;

    pub fn new(name: String) -> Self {
        let mut module = Self {
            name,
            name_table: NameTable::new(),
            functions: PrimaryMap::new(),
            types: PrimaryMap::new(),
            type_names: HashMap::new(),
            enums: Vec::new(),
            globals: Vec::new(),
            imports: Vec::new(),
            classes: Vec::new(),
            entry_point: None,
            external_imports: BTreeMap::new(),
            external_type_defs: BTreeMap::new(),
            external_function_sigs: BTreeMap::new(),
            runtime_registry: HashMap::new(),
            core_builtin_fids: HashSet::new(),
            room_creation_code: BTreeMap::new(),
            initial_room_name: None,
            sprite_names: Vec::new(),
            object_names: Vec::new(),
            passage_names: BTreeMap::new(),
            passage_tags: BTreeMap::new(),
            passage_sources: BTreeMap::new(),
            passage_storylets: BTreeMap::new(),
            system_call_type_rules: BTreeMap::new(),
            callback_return_calls: BTreeMap::new(),
            diagnostics: Vec::new(),
            implicit_return_value: false,
            string_indexed_structs: HashSet::new(),
            array_like_fids: HashSet::new(),
            runtime_type_id: None,
        };
        module.register_core_builtins();
        module
    }

    /// Register core arithmetic, bitwise, boolean, and string builtins that
    /// all frontends share.  These stubs give the constraint collector proper
    /// typed signatures; the linear emitter dispatches on exact name membership
    /// to lower each call to a native target-language operator.
    fn register_core_builtins(&mut self) {
        // Helper: binary sig (ty, ty) -> ty.
        let bin = |ty: Type| FunctionSig {
            params: vec![ty.clone(), ty.clone()],
            return_ty: ty,
            ..Default::default()
        };
        // Helper: unary sig (ty,) -> ty.
        let un = |ty: Type| FunctionSig {
            params: vec![ty.clone()],
            return_ty: ty,
            ..Default::default()
        };
        let scalar_types = [
            Type::Float(64),
            Type::Float(32),
            Type::Int(32),
            Type::Int(64),
        ];

        // Arithmetic: add, sub, mul, div, rem — f64, f32, i32, i64
        for op in &["add", "sub", "mul", "div", "rem"] {
            for ty in &scalar_types {
                let suffix = type_suffix(ty);
                let fid = self.register_runtime(format!("{op}_{suffix}"), bin(ty.clone()));
                self.core_builtin_fids.insert(fid);
            }
        }
        // String concatenation
        let fid = self.register_runtime("concat_str", bin(Type::String));
        self.core_builtin_fids.insert(fid);

        // Negation: neg — f64, f32, i32, i64
        for ty in &scalar_types {
            let suffix = type_suffix(ty);
            let fid = self.register_runtime(format!("neg_{suffix}"), un(ty.clone()));
            self.core_builtin_fids.insert(fid);
        }

        // Boolean
        let fid = self.register_runtime("not_bool", un(Type::Bool));
        self.core_builtin_fids.insert(fid);
        let fid = self.register_runtime("and_bool", bin(Type::Bool));
        self.core_builtin_fids.insert(fid);
        let fid = self.register_runtime("or_bool", bin(Type::Bool));
        self.core_builtin_fids.insert(fid);

        // Bitwise: bitand, bitor, bitxor, shl, shr — i32 only.
        // Float(64) operands are a GML-specific behaviour (bitwise on Reals via
        // implicit ToInt32 coercion).  The GML frontend coerces Float(64) → Int(32)
        // before emitting these ops and coerces the Int(32) result back to Float(64).
        for op in &["bitand", "bitor", "bitxor", "shl", "shr", "lshr"] {
            let fid = self.register_runtime(format!("{op}_i32"), bin(Type::Int(32)));
            self.core_builtin_fids.insert(fid);
        }

        // Bitwise NOT — i32 only (same rationale as above).
        let fid = self.register_runtime("bitnot_i32", un(Type::Int(32)));
        self.core_builtin_fids.insert(fid);

        // Math: single-argument f64 → f64.
        for op in &[
            "sin", "cos", "tan", "asin", "acos", "atan", "sqrt", "exp", "ln", "log2", "log10",
            "abs", "floor", "ceil", "round", "trunc", "sign",
        ] {
            let fid = self.register_runtime(format!("{op}_f64"), un(Type::Float(64)));
            self.core_builtin_fids.insert(fid);
        }

        // Math: two-argument (f64, f64) → f64.
        for op in &["atan2", "pow", "hypot", "min", "max"] {
            let fid = self.register_runtime(format!("{op}_f64"), bin(Type::Float(64)));
            self.core_builtin_fids.insert(fid);
        }

        // String operations.
        let fid = self.register_runtime(
            "string_length_str",
            FunctionSig {
                params: vec![Type::String],
                return_ty: Type::Float(64),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        let fid = self.register_runtime(
            "string_upper_str",
            FunctionSig {
                params: vec![Type::String],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        let fid = self.register_runtime(
            "string_lower_str",
            FunctionSig {
                params: vec![Type::String],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_char_at: (String, Float(64)) -> String  [s, 1-based-index]
        let fid = self.register_runtime(
            "string_char_at_str",
            FunctionSig {
                params: vec![Type::String, Type::Float(64)],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_index_of: (String, String) -> Float(64)  [needle, haystack]
        let fid = self.register_runtime(
            "string_index_of_str",
            FunctionSig {
                params: vec![Type::String, Type::String],
                return_ty: Type::Float(64),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_slice: (String, Float(64), Float(64)) -> String  [s, start, end] 0-based JS slice
        let fid = self.register_runtime(
            "string_slice_str",
            FunctionSig {
                params: vec![Type::String, Type::Float(64), Type::Float(64)],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_split: (String, String) -> Array(String)  [s, sep]
        let fid = self.register_runtime(
            "string_split_str",
            FunctionSig {
                params: vec![Type::String, Type::String],
                return_ty: Type::Array(Box::new(Type::String)),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_char_code_at: (String, Float(64)) -> Float(64)  [s, 0-based-index]
        let fid = self.register_runtime(
            "string_char_code_at_str",
            FunctionSig {
                params: vec![Type::String, Type::Float(64)],
                return_ty: Type::Float(64),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // chr: (Float(64)) -> String  — emit as String.fromCharCode(n)
        let fid = self.register_runtime(
            "chr_f64",
            FunctionSig {
                params: vec![Type::Float(64)],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_repeat: (String, Float(64)) -> String
        let fid = self.register_runtime(
            "string_repeat_str",
            FunctionSig {
                params: vec![Type::String, Type::Float(64)],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_replace_first: (String, String, String) -> String  [s, find, replace]
        let fid = self.register_runtime(
            "string_replace_first_str",
            FunctionSig {
                params: vec![Type::String, Type::String, Type::String],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_trim: (String) -> String
        let fid = self.register_runtime(
            "string_trim_str",
            FunctionSig {
                params: vec![Type::String],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // array_length: (Array(Unknown)) -> Float(64)
        let fid = self.register_runtime(
            "array_length_arr",
            FunctionSig {
                params: vec![Type::Array(Box::new(Type::Value))],
                return_ty: Type::Float(64),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // array_contains: (Array(Unknown), Unknown) -> Bool
        let fid = self.register_runtime(
            "array_contains_arr",
            FunctionSig {
                params: vec![Type::Array(Box::new(Type::Value)), Type::Value],
                return_ty: Type::Bool,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // string_join_arr: (Array(String), String) -> String  — emit as arr.join(sep)
        let fid = self.register_runtime(
            "string_join_arr",
            FunctionSig {
                params: vec![Type::Array(Box::new(Type::String)), Type::String],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // to_number_unknown: (Unknown) -> Float(64)  — emit as Number(x)
        let fid = self.register_runtime(
            "to_number_unknown",
            FunctionSig {
                params: vec![Type::Value],
                return_ty: Type::Float(64),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // to_number_str: (String) -> Float(64)  — emit as Number(x)
        let fid = self.register_runtime(
            "to_number_str",
            FunctionSig {
                params: vec![Type::String],
                return_ty: Type::Float(64),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // to_string_unknown: (Unknown) -> String  — emit as String(x)
        let fid = self.register_runtime(
            "to_string_unknown",
            FunctionSig {
                params: vec![Type::Value],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // to_i32_f64: (Float(64)) -> Float(64)  — emit as x | 0
        let fid = self.register_runtime(
            "to_i32_f64",
            FunctionSig {
                params: vec![Type::Float(64)],
                return_ty: Type::Float(64),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // to_u32_f64: (Float(64)) -> Float(64)  — emit as x >>> 0
        let fid = self.register_runtime(
            "to_u32_f64",
            FunctionSig {
                params: vec![Type::Float(64)],
                return_ty: Type::Float(64),
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // is_struct_unknown: (Unknown) -> Bool  — emit as typeof x === "object" && x !== null && !Array.isArray(x)
        // NOTE: is_array_unknown was removed. Array membership is expressed as
        // Op::TypeCheck(x, Type::Array(...)), which each backend lowers to its native
        // array-check operator. Named builtins for concepts the IR already expresses
        // structurally violate Law 2 (engine specificity at boundaries).
        let fid = self.register_runtime(
            "is_struct_unknown",
            FunctionSig {
                params: vec![Type::Value],
                return_ty: Type::Bool,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // is_numeric_unknown: (Unknown) -> Bool  — emit as typeof x === "number"
        let fid = self.register_runtime(
            "is_numeric_unknown",
            FunctionSig {
                params: vec![Type::Value],
                return_ty: Type::Bool,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // typeof_gml: (Unknown) -> String  — emit as GML type-name ternary chain
        // Backend primitive: the TS backend lowers this to inline JS in lower.rs.
        let fid = self.register_runtime(
            "typeof_gml",
            FunctionSig {
                params: vec![Type::Value],
                return_ty: Type::String,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);
        // select: (Bool, Unknown, Unknown) -> Unknown  — emit as cond ? on_true : on_false
        let fid = self.register_runtime(
            "select",
            FunctionSig {
                params: vec![Type::Bool, Type::Value, Type::Value],
                return_ty: Type::Value,
                ..Default::default()
            },
        );
        self.core_builtin_fids.insert(fid);

        // Comparison: cmp_eq, cmp_ne, cmp_lt, cmp_le, cmp_ge, cmp_gt
        // Parameters are (Unknown, Unknown) → Bool so the signature accepts
        // any comparable value without constraining the operand types.
        let cmp_sig = || FunctionSig {
            params: vec![Type::Value, Type::Value],
            return_ty: Type::Bool,
            ..Default::default()
        };
        for name in &["cmp_eq", "cmp_ne", "cmp_lt", "cmp_le", "cmp_ge", "cmp_gt"] {
            let fid = self.register_runtime(*name, cmp_sig());
            self.core_builtin_fids.insert(fid);
        }
    }

    /// Register a runtime/builtin function (e.g. `"add_f64"`).
    ///
    /// Creates a stub `Function` with the given name and signature and an
    /// empty entry block (`Terminator::Return(None)`), pushes it into
    /// `module.functions` and `name_table.func_names`, and records the
    /// name → `FuncId` mapping in `runtime_registry`.
    ///
    /// Frontends call this before translation to declare typed arithmetic,
    /// logic, and stdlib builtins they will emit as `Op::Call`.  Because the
    /// stub is a real `Function`, the constraint collector picks it up
    /// automatically when it iterates `module.functions` — no separate chain
    /// is needed.
    ///
    /// The linear emitter dispatches on exact name membership (via
    /// `core_builtin_fids`) to emit the corresponding target-language operator
    /// rather than a function call.
    pub fn register_runtime(&mut self, name: impl Into<String>, sig: FunctionSig) -> FuncId {
        let name = name.into();
        let entry_block = Block {
            params: Vec::new(),
            insts: Vec::new(),
            terminator: Terminator::Return(None),
        };
        let mut blocks: crate::entity::PrimaryMap<BlockId, Block> =
            crate::entity::PrimaryMap::new();
        let entry = blocks.push(entry_block);
        let func = Function {
            name: name.clone(),
            sig,
            visibility: Visibility::Public,
            namespace: Vec::new(),
            class: None,
            method_kind: MethodKind::Free,
            specializations: HashMap::new(),
            blocks,
            insts: crate::entity::PrimaryMap::new(),
            value_types: crate::entity::PrimaryMap::new(),
            entry,
            coroutine: None,
            value_names: HashMap::new(),
            capture_params: Vec::new(),
            null_sentinel_values: std::collections::HashSet::new(),
            type_rule: None,
            inline_hint: InlineHint::Default,
        };
        let name_id = self.name_table.func_names.push(name.clone());
        let id = self.functions.push(func);
        debug_assert_eq!(id, name_id);
        self.runtime_registry.insert(name, id);
        id
    }

    /// Attach an IR body to a runtime stub previously created by
    /// [`register_runtime`].
    ///
    /// Builds a full `Function` with a [`FunctionBuilder`] pre-loaded with this
    /// module's runtime registry (so `call_named` and the arithmetic helpers
    /// resolve), calls `build` to populate it, then copies the resulting
    /// `blocks`, `insts`, `value_types`, and `entry` into the stub in-place and
    /// sets `inline_hint = InlineHint::Always`.  The stub's `name`, `sig`,
    /// `visibility`, and registry entry are left untouched.
    ///
    /// This is the required mechanism for frontend runtime-library bodies:
    /// each frontend defines its runtime library in IR once and every backend
    /// emits it, avoiding M×N reimplementations.
    ///
    /// [`register_runtime`]: Module::register_runtime
    /// [`FunctionBuilder`]: super::builder::FunctionBuilder
    ///
    /// # Panics
    /// Panics if `name` is not in the runtime registry — this is a programming
    /// error (the stub must be registered before its body is attached).
    pub fn attach_runtime_body<F>(&mut self, name: &str, params: &[Type], return_ty: Type, build: F)
    where
        F: FnOnce(&mut super::builder::FunctionBuilder),
    {
        let fid = self
            .lookup_runtime(name)
            .unwrap_or_else(|| panic!("attach_runtime_body: '{name}' not in runtime registry"));
        let sig = FunctionSig {
            params: params.to_vec(),
            return_ty,
            defaults: vec![],
            has_rest_param: false,
            param_lower_bounds: vec![],
        };
        let mut b = super::builder::FunctionBuilder::new(name, sig, Visibility::Public);
        b.set_registry(self.runtime_registry.clone());
        build(&mut b);
        let built = b.build();
        let stub = &mut self.functions[fid];
        stub.blocks = built.blocks;
        stub.insts = built.insts;
        stub.value_types = built.value_types;
        stub.entry = built.entry;
        stub.inline_hint = InlineHint::Always;
    }

    /// Register an alias name for an already-registered runtime function.
    ///
    /// Inserts `alias → fid` into the runtime registry without creating a new
    /// `Function` entry.  Use this for alternate spellings of the same function
    /// (e.g. `color_get_red` → same `FuncId` as `colour_get_red`).
    pub fn register_alias(&mut self, alias: impl Into<String>, fid: FuncId) {
        self.runtime_registry.insert(alias.into(), fid);
    }

    /// Look up the `FuncId` of a previously registered runtime function.
    ///
    /// Returns `None` if no runtime function with this name has been registered.
    pub fn lookup_runtime(&self, name: &str) -> Option<FuncId> {
        self.runtime_registry.get(name).copied()
    }

    /// Return a solvable placeholder for use by frontends that do not yet know
    /// a value's type — an open inference target the constraint collector frees
    /// into a real arena variable.
    ///
    /// Emits [`Type::InferVar`] with a fixed placeholder id (the builder has no
    /// arena to allocate a real one). The id is never dereferenced: the
    /// collector matches the *variant* to keep it free, then allocates a fresh
    /// arena var keyed by `ValueId`. Distinct from [`Type::Value`], the honest
    /// type of a genuinely-dynamic value, which is concrete and pre-binds.
    pub fn fresh_var(&mut self) -> Type {
        Type::fresh_var()
    }
}

/// Return the short suffix string used in builtin names for a given type.
///
/// Used by [`Module::register_core_builtins`] to build names like
/// `"add_f64"` or `"bitand_i32"`.
///
/// # Panics
/// Panics if `ty` is not one of the scalar types used by core builtins.
fn type_suffix(ty: &Type) -> &'static str {
    match ty {
        Type::Float(64) => "f64",
        Type::Float(32) => "f32",
        Type::Int(32) => "i32",
        Type::Int(64) => "i64",
        Type::Bool => "bool",
        Type::String => "str",
        other => panic!("type_suffix: unsupported type {other:?}"),
    }
}

/// Recursively normalize compound types (Array, Map, Option, etc.) so that
/// any nested type references in the tree are passed through unchanged.
///
/// `Type::Struct` no longer exists; all named types are already `Type::Instance`
/// after the frontend builds the IR. This function handles compound wrappers only.
fn normalize_type(ty: Type) -> Type {
    match ty {
        Type::Array(elem) => Type::Array(Box::new(normalize_type(*elem))),
        Type::Map(k, v) => Type::Map(Box::new(normalize_type(*k)), Box::new(normalize_type(*v))),
        Type::Option(inner) => Type::Option(Box::new(normalize_type(*inner))),
        Type::Tuple(elems) => Type::Tuple(elems.into_iter().map(normalize_type).collect()),
        Type::Union(elems) => Type::Union(elems.into_iter().map(normalize_type).collect()),
        Type::Function(sig) => {
            let params = sig.params.into_iter().map(normalize_type).collect();
            let return_ty = normalize_type(sig.return_ty);
            Type::Function(Box::new(super::ty::FunctionSig {
                params,
                return_ty,
                defaults: sig.defaults,
                has_rest_param: sig.has_rest_param,
                param_lower_bounds: sig.param_lower_bounds,
            }))
        }
        Type::Coroutine {
            yield_ty,
            return_ty,
        } => Type::Coroutine {
            yield_ty: Box::new(normalize_type(*yield_ty)),
            return_ty: Box::new(normalize_type(*return_ty)),
        },
        // All other types (Instance, ClassRef, primitives, etc.) are already normalized.
        other => other,
    }
}

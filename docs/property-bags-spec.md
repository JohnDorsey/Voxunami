# Phase 24 specification: property bags on block definitions

**Status: proposed 2026-10-18 for the owner's review. Nothing here is built.** It makes concrete revision 2 of `plan.md` ("Plugin shape
revisions") and requirement 5 of "Requirements pulled forward". The Rust is a sketch of shape and size. Read with `plugin-architecture-review.md`
section 2.1 (what property bags are and how they are built).

## 1. The problem, as it is in the code

[code, read 2026-10-18] `BlockDefinition` has fixed fields `emission`, `flammability` and `passable` next to the ones the engine reads for itself
(`color`, `see_through`, the shapes, the state colours). `BlockTable` copies them into `Entry` and answers `emission(block)`, `flammability(block)` and
`is_passable(block)`. Only Cave's rules ask: the light (`lighting.rs`, `multiscale_light.rs`, the light plugin's scan of edited bricks), fire
(`fire.rs`) and movement (`movement.rs`). The engine crate therefore carries three game concepts, and a rule that wants a fourth (conductivity, hardness)
means editing it.

## 2. The shape

A block definition carries **properties**: values, by namespaced key, that plugins declare. A key is declared once, with a type, a default and a doc
line; a definition sets the keys it cares about. The table turns them into one column per key, indexed by block id, and rules read through a
typed handle resolved once.

```rust
// block_models
pub struct PropertyKey<T: PropertyType> { id: &'static str, default: T, doc: &'static str, .. }   // const-constructible
pub trait PropertyType: Copy { /* bool, u8, u16, u32, i32 */ }
pub struct PropertyDeclaration { id: NamespacedId, kind: PropertyKind, default: i64, doc: String }
pub enum PropertyKind { Bool, Integer { minimum: i64, maximum: i64 } }
pub struct PropertySchema { .. }                       // the declared keys; the engine's own are always in it
pub struct PropertyHandle<T> { column: u32, .. }       // Copy; resolved once from a table

impl BlockDefinition { pub fn with_property<T: PropertyType>(self, key: &PropertyKey<T>, value: T) -> Self; }
impl BlockTable {
    pub fn try_from_definitions(schema: &PropertySchema, definitions: &[Option<&BlockDefinition>]) -> Result<Self, PropertyError>;
    pub fn handle<T: PropertyType>(&self, key: &PropertyKey<T>) -> Result<PropertyHandle<T>, PropertyError>;
    pub fn get<T: PropertyType>(&self, handle: PropertyHandle<T>, block: u32) -> T;   // an array index; an unknown block has the default
}
```

- **Kinds, for now:** bool and integers with a range. Nothing a rule reads is a float (determinism). Strings, registry ids and client-only floats
  join when something needs them (a tag is a bool property).
- **Errors are loud and early:** a definition that sets a key nobody declared, a value outside the declared range, or a table asked for a key as
  the wrong type is an error naming the key (and, from Phase 29, the file and path). `from_definitions` stays as the convenience for tests and
  panics with that message.
- **Canonical form:** definitions keep their properties in a `BTreeMap` by key and values are integers, so a definition has one byte form (the
  content package of Phase 28 hashes it).
- **Per block, not per state:** a property is the same for every state of a block (as emission is today). Per-state properties are a later
  extension if a game needs them.
- **Per-voxel data is not this:** that is a channel. Properties are definitions, are never saved in worlds, and are rebuilt from content.

## 3. Who declares what (the dependency rule: games -> plugins -> engine)

| Keys | Declared in | Why there |
|---|---|---|
| `engine:density` (u32, default 1000), `engine:placeable_on_moving_grids` (bool, default true) | `block_models` | The engine will read them (bodies' mass, Phase 32; the one flag a mod author sets for airships). Nothing reads them yet; they are declared and tested so the engine-owned path exists |
| `base_blocks:emission` (u8, 0), `base_blocks:flammability` (u8, 0), `base_blocks:passable` (bool, false) | `base_blocks` | The plugin whose blocks set them. Cave's rules (a game above the plugin) read them; the engine no longer knows them |

`BlockPropertiesExt` in `base_blocks` gives `with_emission`, `with_flammability` and `with_passable` on `BlockDefinition`, so the call sites keep
reading as they do; they set the properties underneath. A game declares the keys it uses in `PackageSpec` (`properties`), so the client builds the
same table.

## 4. What changes

- `block_models`: the three fields and `emission`, `flammability`, `is_passable` leave `BlockDefinition` and `BlockTable`; the property machinery
  is added; `BlockTable::from_definitions` keeps working for definitions with no properties.
- `base_blocks`: declares its three keys and the extension trait; sets them on lamp, grass, wood and fire as now.
- `game_shell::PackageSpec`: a `properties` list; `ContentPackage::block_table` passes the schema.
- Cave's rules resolve a handle when they are built (`Lighting`, `MultiScaleLighting`, `Fire`, `Movement`, the light plugin) and read through it.
- Nothing saved, sent or drawn changes. The golden sessions must pass unmodified.

## 5. Tests and checks

- Unit tests in `block_models`: a declared key is read back as set; an unset key gives the default; an unknown block gives the default; an undeclared
  key, a wrong type and an out-of-range value are errors that name the key; two definitions' properties give identical canonical bytes whatever
  order they were set in; the engine's keys are always in the schema.
- `base_blocks`: lamp, grass, wood and fire have the emission, flammability and passability they have today (the existing tests, through handles).
- The three golden sessions, the pinned generator digests and every existing test pass unmodified, on Windows and Linux.
- **Performance:** the multiscale rule benchmark (`multiscale_modes`, `rule_benchmark`), which reads emission for every voxel it looks into, was
  measured before this change: fill 770 to 830 ms (32 levels), 327 to 336 (16), 251 to 256 (8), 336 to 343 (4), three runs on the dev machine in a release
  build. The same after must be within the spread (about 5 percent); if not, the column read is made cheaper (a typed column rather than a
  conversion from `i64`) before this phase is called done.
- Mutations (with `PROPTEST_DISABLE_FAILURE_PERSISTENCE=1`): the unknown-block default, the range check, the kind check, and the schema check each broken once.

## 6. Steps

- **24a** The machinery in `block_models`, with its tests; the old fields still there.
- **24b** `base_blocks` declares and sets the properties (both ways in the definitions for the moment).
- **24c** Cave's rules, movement and the light plugin read through handles; the golden sessions and the benchmark after each rule.
- **24d** Remove the old fields and accessors; `PackageSpec.properties`; documentation; gates; commit.

## 7. Questions for the owner

1. **Who owns `passable`:** I put it in `base_blocks` with emission and flammability, since base_blocks' blocks set them and Cave's rules read them; the
   alternative is `voxunami_cave:passable`, which would make `base_blocks` depend on a game (not allowed). Is `base_blocks:passable` right?
2. **Kinds for now:** bool and integers only. OK?
3. **Per block or per state:** per block, as today. Does any block you plan (a lit and unlit lamp, a door open and shut) want different properties by state?

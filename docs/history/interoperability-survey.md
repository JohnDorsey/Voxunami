# Borrow Veloren's data, build on open standards

*A survey written on 2026-11-13 for the owner of Voxunami, who asked what could be taken from Veloren (GPL-3) and what else exists to let third-party avatars, accessories, builds, tools and control schemes appear in the game, with moddability and compatibility ahead of saving work. It is research, not legal advice: the licensing section is my reading, not a lawyer's. It describes other people's software and terms as of its date. Effort: S about a week, M a month, L a quarter, XL more.*

**How far to trust it.** Every non-trivial claim carries a tag. **[re-read]**: the page's own text (a raw file or markdown) was read a second time; where a re-read and an earlier read disagree, the re-read wins and I say so. **[read]**: read through a fetch tool that summarises each page with a small model, so numbers and wording may be the summary's. **[secondary]**: a second-hand source (search summary, news, vendor blog, forum). **[recalled]**: memory, unchecked; I say so wherever a recommendation leans on one. **[inference]**: my reasoning from cited facts. Three limits matter. Mojang's Minecraft end-user licence agreement (EULA) and Usage Guidelines could not be read (minecraft.net timed out on every attempt and the Internet Archive was blocked), so everything here about Mojang's terms, including skin use, rests on secondary summaries or recall; read them in a browser before any skin, texture or build work. The web-search allowance ran out in every line of inquiry, so a statement that something does not exist ("no project voxelises a skinned avatar") is weak evidence. Nothing here relies on Hytale's gated server source; none was read.

## The short version

**Veloren: the licence facts.** Veloren's code is **GPL-3.0-or-later**: the workspace metadata says so, and the LICENSE file is the plain GPLv3 text, with a notice that allows later versions, and no added exceptions ([Cargo.toml](https://gitlab.com/veloren/veloren/-/raw/master/Cargo.toml), [LICENSE](https://gitlab.com/veloren/veloren/-/raw/master/LICENSE)) [re-read]. GPLv3 section 13 lets a GPL work be combined with an AGPLv3 work and conveyed, the AGPL's network clause then applying to the combination [re-read], so Veloren's code can enter a GPL-3.0-or-later or an AGPL-3.0-or-later Voxunami [inference]. Its assets are not all GPL: `assets/credits.ron` has 41 entries, including eight music entries (seven tracks), each barred from use in other games except as parody, and three voxel models, all under **CC-BY-NC-SA 4.0**; entries with no licence field are assumed GPLv3 ([credits.ron](https://gitlab.com/veloren/veloren/-/raw/master/assets/credits.ron)) [re-read]. The contributing page has no contributor licence agreement ([CONTRIBUTING.md](https://gitlab.com/veloren/veloren/-/raw/master/CONTRIBUTING.md)) [re-read], so copied code stays owned by its many authors and can never be relicensed permissively [inference].

**As code: legally yes, practically almost nothing.** Everything from the `common` crate up is welded to the Specs entity-component system and a pinned nightly toolchain, and the crates are not on crates.io ([veloren-common](https://crates.io/api/v1/crates/veloren-common), [rust-toolchain](https://gitlab.com/veloren/veloren/-/raw/master/rust-toolchain)) [read; the pin, `nightly-2026-06-13`, is [re-read]]. Worth lifting: the separate `assets_manager` crate behind its layered asset directories (MIT OR Apache-2.0) ([crate](https://crates.io/api/v1/crates/assets_manager)) [read]; little else.

**As data and formats: yes, the prize.** Veloren's body model is the largest open, working precedent I found for the part system Voxunami lacks: a rig of at most 16 bones, each holding small MagicaVoxel `.vox` parts placed by a RON (Rust Object Notation) manifest (asset id, offset, model index), grey armour tinted in code, and palette indices that carry meaning ([load.rs](https://gitlab.com/veloren/veloren/-/raw/master/voxygen/src/scene/figure/load.rs), [cell.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/src/figure/cell.rs)) [read]. The library holds at least 100 creature directories, 34 armour sets and dozens of hair, beard and accessory models [read]. An import-only converter that bakes parts into Voxunami's own format is realistic (M); runtime support would chase manifests that change weekly [inference].

**As design: three ideas.** Content bundles announced by hash and fetched on demand ([server.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/net/src/msg/server.rs)) [read] fit your stream rule directly [inference]. The palette-index conventions and the "indices into manifest lists" character creator are the others. Veloren's WebAssembly plugin host is a pattern worth copying with limits added: as re-read, it turns on the component model and nothing else, so it sets no CPU or memory budget ([module.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/state/src/plugin/module.rs)) [re-read].

**Not worth touching.** Joining Veloren servers or syncing its components (I found no external protocol specification) [read; weak]; any `veloren-*` crate; its combat AI and character-creator code (hard-coded Rust); plugin-interface compatibility (one added function broke existing plugins ([MR 5105](https://gitlab.com/api/v4/projects/veloren%2Fveloren/merge_requests/5105))) [read]; its music and the three non-commercial models. Veloren also offers no precedent for showing off: its roadmap rules out vanity slots ([roadmap](https://book.veloren.net/players/roadmap.html)) [read]. Show-off avatars are greenfield.

**Beyond Veloren.** Minecraft (Java and Bedrock) appears to have the biggest pools of skins, builds and resource packs [inference], in documented or reverse-documented formats [read], but its terms were unreadable. Cubzh (apparently renamed Blip) is the only open-source, voxel-native avatar ecosystem I found (Apache-2.0) [read]. Hytale is a design reference and a tooling tie-in through its GPL-3.0 Blockbench plugin, not a content source [read]. Teardown's Workshop shows how large a `.vox` community gets, and Luanti has the cleanest content licensing [read].

### The ten best moves

Ranked by (a) how much existing third-party content or moddability each unlocks and (b) fit with the architecture (the client sees the world only through the server's message stream; determinism; hash-addressed content), after a hard licence filter: each move passes in the mode stated (read files the player supplies, bundle only bundleable licences, relay only what a work's own licence allows). Effort is information and did not set the order. Moves 1 and 6 are enabling layers that unlock nothing alone but gate the rest.

| Rank | Move | What it unlocks | Licence position | Effort |
|---|---|---|---|---|
| 1 | Appearance record plus hash-addressed blobs (the "phase that adds assets") | Delivery of every avatar, accessory, skin, ship and pack to other players and into replays; Veloren ships plugin bundles this way [read] | Design only | M |
| 2 | `.vox` as the common voxel-part door, with a socketed part format | Veloren's library; about 15,000 Teardown Workshop items [read]; hand-made accessories and ships; vengi lists about 50 voxel formats, direction per format unverified [read] | Spec repository and `dot_vox` are MIT [read]; each model keeps its author's licence | S to M |
| 3 | Blockbench plugin on a native box-tree format | A free tool for creators; Blockbench converts Minecraft Java, Bedrock and its own projects into the format inside the editor [inference]; Hytale's GPL-3.0 plugin is a template [read] | Blockbench is GPL-3.0-or-later; plugins may carry any licence [re-read] | S to M export-only; M to L full |
| 4 | Build interchange model with Minecraft schematic readers | The biggest build archives I found, with blocks about the size of your voxels [inference]; later hosts `.vox` ships, a mesh voxeliser, Space Engineers and LDraw importers | Formats readable from public descriptions; user-supplied files only; avoid PyMCTranslate data, which is proprietary [re-read] | M |
| 5 | Minecraft skin import (local PNG, optional fetch by name) | Probably the largest avatar pool [inference], in one fixed 64 by 64 layout [read] | Mojang terms unread: user-supplied only, bundle none | S |
| 6 | WebAssembly mod host configured to Wasmtime's determinism recipe, on a release line that gets security backports | Every community-written importer, ship computer, behaviour and brain | Wasmtime is Apache-2.0 [read] | M; L with a modding kit |
| 7 | One-shot Veloren importer (release 0.18 snapshot, filtered through `credits.ron`) | Ready-made creatures, armour, hair, accessories, airships and boats | GPL-3.0 by default; exclude CC-BY-NC-SA entries [re-read] | M |
| 8 | Minecraft resource packs as user-supplied picture packs (tier one) | 38,281 resource-pack projects on Modrinth [read] | User-supplied only; many packs are Mojang-derived or custom-licensed [inference] | S to M |
| 9 | VRM 1.0 and VRMA, with VRM-named bones as the canonical skeleton | The VRoid and VTuber avatar pool; free retargeting of VRMA and CC0 motion sets; later tracking over the Virtual Motion Capture protocol | Libraries MIT or Apache-2.0 [read]; per-model flags default to no redistribution, no modification [read] | M to L |
| 10 | Action system with input channels (SDL3, opentrack UDP, Open Sound Control, Elite Dangerous `.binds` importer) | Control schemes from existing hardware and games; accessibility met by design | SDL_GameControllerDB is zlib; opentrack is mostly ISC [read] | M; then S per channel |

Moves 4, 5 and 8 pass the licence filter only as "read the user's own files", and Mojang's terms are unread. The order of work differs from this ranking because of dependencies; see "Suggested order of first steps".

## Veloren: its parts library and delivery design are worth taking, its code is not

**The figures.** Veloren's artists use MagicaVoxel; a character is about 22 voxels tall, a landscape block 11 by 11 by 11 small voxels ([Book](https://book.veloren.net/contributors/artists/voxel-models.html)) [read]. The head manifest is keyed by species and body type and lists eyes, hair, beards and accessories ([manifest](https://gitlab.com/veloren/veloren/-/raw/master/assets/voxygen/voxel/humanoid_head_manifest.ron)) [read]. Palette indices carry meaning: 0 to 7 body materials, 8 to 12 shiny, 13 to 15 glowing, 16 hollow, 17 to 21 matte overrides ([figure/mod.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/src/figure/mod.rs)) [read]. Ships are fixed `.vox` models in one manifest (nine vehicles, airship to train) with a `custom_indices` map from palette slots to blocks such as a helm or lava ([ship manifest](https://gitlab.com/veloren/veloren/-/raw/master/assets/common/manifests/ship_manifest.ron)) [read]. Library counts are floors, taken from listings capped at 100 entries ([creatures](https://gitlab.com/api/v4/projects/veloren%2Fveloren/repository/tree?path=assets/voxygen/voxel/npc&per_page=100), [armour](https://gitlab.com/api/v4/projects/veloren%2Fveloren/repository/tree?path=assets/voxygen/voxel/armor&per_page=100)) [read]. Animation is Rust code, and the Book's animator page is empty ([run.rs](https://gitlab.com/veloren/veloren/-/raw/master/voxygen/anim/src/character/run.rs), [Book](https://book.veloren.net)) [read], so it cannot be imported; Voxunami would write its own gaits, driven by the game tick as your rules require for client cosmetics.

**What a player can create.** The humanoid creator is eight byte-sized indices (hair style, beard, eyes, accessory, hair colour, skin, eye colour, height) plus species and body type, every count a hard-coded `match` in Rust ([humanoid.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/src/comp/body/humanoid.rs)) [read]. A player cannot add a species, hairstyle or skin without editing the game; creators can add NPC bodies through a plugin, and items [inference].

**Plugins.** Since 0.16 (2024-03) plugins are WebAssembly components with WIT interfaces (the component model's interface description format) on wasmtime; assets-only plugins overlay and merge into base manifests ([CHANGELOG](https://gitlab.com/veloren/veloren/-/raw/master/CHANGELOG.md)) [read]. A `.plugin.tar` holds `plugin.toml`, modules and an asset tree, and its SHA-256 is its id ([plugin host](https://gitlab.com/veloren/veloren/-/raw/master/common/state/src/plugin/mod.rs)) [read]. The ecosystem is thin: the 1.0 tracking issue was closed on 2026-06-28 with 6 of 22 tasks done ([#908](https://gitlab.com/api/v4/projects/veloren%2Fveloren/issues/908)), and the Book's plugin guide describes crates that no longer exist ([guide](https://book.veloren.net/contributors/modders/writing-a-plugin.html)), so a review that relied on that page alone is out of date [read]. The host's module file, read whole a second time, builds its wasmtime configuration with the component model switched on and nothing else, and contains no fuel, epoch interruption, store limits or memory maximum [re-read]; a keyword search of its memory-manager file found none either [read]; so a client appears to run server-sent WebAssembly with no CPU or memory budget [inference]. Community plugins are a handful of examples (a canoe, GPL-3.0, four stars) ([canoe](https://github.com/cpetig/veloren-plugin-canoe)) [read], so the larger reusable body is the core team's own `.vox` library [inference].

**Mob AI and entity data.** Behaviour is Rust; configuration is RON. Entity configs, loot, recipes, trades and ability parameters are data, but ability behaviours are Rust character states and the combat AI's attack module alone is about 369,000 characters ([generation.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/src/generation.rs)) [read]. Worth re-implementing: the NPC simulation's resumable `Action` combinators and the habit of loading every entity config in a test ([rtsim](https://gitlab.com/veloren/veloren/-/raw/master/rtsim/src/ai/mod.rs)) [read].

**Health.** Alive but slowing: 55 commits on master from 2026-09-13 to 2026-11-10, release 0.18.0 on 2026-01-23 after about thirteen months (0.17.0 was 2024-12-27 UTC [re-read]), and a year of blog silence ended on 2026-11-07 ([project](https://gitlab.com/api/v4/projects/veloren%2Fveloren), [post](https://veloren.net/blog/update)) [read]. Depend on a pinned release snapshot, not the live tree [inference].

| Veloren part | How to take it | Licence | Effort | Verdict |
|---|---|---|---|---|
| `.vox` library (creatures, armour, hair, accessories, airships) | One-shot converter, or read the player's own install | GPL-3.0 by default; non-commercial items excluded [re-read] | S to read; M to convert | Take |
| Figure manifests, palette conventions, `custom_indices` | Read; implement in an importer | GPL-3.0 (design free) | M with your own rig | Take as design |
| Hash-announced plugin bundles | Copy the design | Idea free | S to M | Take |
| WIT plus wasmtime host | Copy the pattern, add limits | Interface file dual GPL-3 / CC-BY-SA-4.0 ([wit/LICENSE](https://gitlab.com/veloren/veloren/-/raw/master/plugin/wit/LICENSE)) [re-read] | M | Pattern only |
| Character creator, entity and ability RON, combat AI, NPC simulation, 220-line A* ([astar.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/src/astar.rs)) [read] | Design only; re-implement the combinators | GPL | S to M; XL to port | Design only |
| Wire protocol, `veloren-*` crates | None | GPL; nightly, Specs | XL, fragile | Do not touch |
| Music, three non-commercial models | None | CC-BY-NC-SA 4.0 [re-read] | None | Excluded |

## Avatars: adopt VRM's vocabulary, own a small voxel format, import the rest

**Open standards.** VRM 1.0 is the one open, complete and populated avatar format: a glTF binary with a humanoid map of 55 bones (15 required), 18 expression presets, spring bones, the MToon shading model, and a companion animation type, VRMA, whose rotations are in a normalised T-pose so one clip plays on any humanoid ([specification](https://github.com/vrm-c/vrm-specification/tree/master/specification), [humanoid](https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_vrm-1.0/humanoid.md), [VRMA](https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_vrm_animation-1.0/README.md)) [read]. Version 0.x is deprecated but circulates, faces the other way along Z and shares the `.vrm` extension, so loaders must detect the version ([vrm.dev](https://vrm.dev/en/vrm1/)) [read]. The pool is large: VRoid Hub passed one million models by May 2021 ([report](https://www.moguravr.com/?p=148493)) [secondary]. Libraries are permissive (UniVRM, three-vrm and VRM4U are MIT); the Rust crates are young, among them `vrm-runtime` (MIT OR Apache-2.0, VRMA retargeting, two commits) and the Bevy-bound `bevy_vrm1` ([vrm-runtime](https://github.com/nanowater/vrm-runtime-rs), [bevy_vrm1](https://github.com/not-elm/bevy_vrm1)) [read].

The licence problem is per model. VRM `meta` records permissions with restrictive defaults: use only by the author, personal non-profit use, credit required, **no redistribution, no modification** ([meta](https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_vrm-1.0/meta.md)) [re-read]. The VRM Public License binds anyone who operates a model and forbids extra restrictions and technical protection measures ([licence](https://vrm.dev/licenses/1.0/)) [re-read]. Making or sharing adapted copies is a right only where the author's licence settings grant it, and an adapter's licence must be the same or more restrictive; the text has no explicit wording on converting to another format, and its "technical modifications" item says only that modifications needed to use the licensed rights in other formats are not themselves adapted work [re-read]. Whether a voxelised avatar counts as adapted work is therefore unsettled. Voxelising is arguably modification and relaying to other players arguably redistribution [inference], so shared display must be gated on the flags. VRoid Hub has an OAuth 2.0 interface ([OAuth](https://developer.vroid.com/api/oauth-api.html)) [read] and a "multiplay" download licence that lets other players fetch the same model [secondary], but its terms returned HTTP 403, and an open-source client cannot hold a client secret, so Hub access would need a token-broker server [inference]. Make player-supplied `.vrm` files the primary path.

Khronos is absorbing VRM. The draft avatar extensions (`KHR_character` and companions) are still a pull request opened 2025-07-28 ([PR 2512](https://github.com/KhronosGroup/glTF/pull/2512)); the ratified list has no character, humanoid or spring-bone extension ([list](https://github.com/KhronosGroup/glTF/blob/main/extensions/README.md)); a July 2026 deck plans a "VRM 1.X", then a "VRM 2.0" ([deck](https://metaverse-standards.org/wp-content/uploads/MSF-3D-Assets-BOF-SIGGRAPH-Jul26.pdf)) [read]. Design on what VRM and the draft share, and write readers, not writers [inference]. The Open Metaverse Interoperability Group has ratified nothing, though two of its extensions suit ship parts ([OMI](https://github.com/omigroup/gltf-extensions)) [read].

**Hosted services.** Ready Player Me is the cautionary tale: Netflix announced on 2025-12-19 that it was acquiring it, the service including PlayerZero wound down on 2026-01-31, and about 20 people joined Netflix ([TechCrunch](https://techcrunch.com/2025/12/19/netflix-acquires-gaming-avatar-maker-ready-player-me/)) [re-read]. Accept files, never depend on a hosted account, and mirror any specification you rely on [inference]; replacements are priced for companies (Avaturn Pro lists $800 a month) ([pricing](https://avaturn.me/pricing)) [read].

**Other games' systems.** Minecraft, Bedrock, Hytale and Blockbench share one structure, a tree of nodes with pivots and rotations holding boxes with per-face texture coordinates plus a PNG [inference]. A Minecraft skin is a 64 by 64 PNG with inner and outer layers and classic (4 pixel) or slim (3 pixel) arms ([wiki](https://minecraft.wiki/w/Skin)) [read]; the box positions are in the MIT skinview3d source ([model.ts](https://raw.githubusercontent.com/bs-community/skinview3d/master/src/model.ts)) [read]; Mojang's lookup returns skin and cape addresses without a token, at about 200 requests per two minutes per address ([Mojang API](https://minecraft.wiki/w/Mojang_API)) [read]. Hytale characters use cubes and quads only, with grey textures tinted by gradient sets ([model guide](https://hytale.com/news/2025/12/an-introduction-to-making-models-for-hytale), [merger tool](https://github.com/hytale-tools/blockymodel-merger)) [read]; its EULA lets players share models and skins but bans distributing game files, and mods run only on servers, so live Hytale avatars cannot be shown ([EULA](https://hytale.com/eula), [strategy](https://hytale.com/news/2025/11/hytale-modding-strategy-and-status)) [read]. Cubzh has a documented `.3zh` voxel format with named points and an avatar module of twelve parts, but its build is not yet functional and its terms are unverified ([blip](https://github.com/bliporg/blip), [format](https://raw.githubusercontent.com/bliporg/blip/main/cubzh-file-format-3zh.txt), [avatar.lua](https://raw.githubusercontent.com/bliporg/blip/main/lua/modules/avatar.lua)) [read]. Roblox publishes the best accessory specification and is closed; Decentraland's design records are CC0 and show hide lists and tint-by-material-name ([Roblox](https://create.roblox.com/docs/avatar/rigid-accessories/specifications), [ADR-57](https://adr.decentraland.org/adr/ADR-57), [licence](https://raw.githubusercontent.com/decentraland/adr/main/LICENSE.md)) [read].

**Open-source creators.** MakeHuman is the most permissive parametric human: assets and exports are CC0, and its code is AGPL for the application [read] and GPL for the Blender add-on [secondary] ([licence page](https://static.makehumancommunity.org/about/license.html)). Its forum closed on 2026-11-08, so mirror the packs you need ([site](https://static.makehumancommunity.org)) [read; inference]. OxiHuman is a five-commit Rust generator exporting glTF, VRM 1.0, OBJ and STL (Apache-2.0) ([oxihuman](https://github.com/cool-japan/oxihuman)) [read]. Part assemblers exist for VRM (Character Studio, reported MIT) and in 2D (the LPC generator: GPL-3.0 code, art licensed per asset, with a credits export) ([Character Studio](https://github.com/M3-org/CharacterStudio), [LPC](https://github.com/LiberatedPixelCup/Universal-LPC-Spritesheet-Character-Generator)) [read]. DiceBear makes a deterministic avatar from a seed (MIT; 42 of 61 styles CC0, including two voxel styles) ([licences](https://www.dicebear.com/licenses/)) [read]. CC0 kits include Kenney Blocky Characters, KayKit Adventurers and Quaternius outfits ([Kenney](https://kenney.nl/assets/blocky-characters), [KayKit](https://kaylousberg.itch.io/kaykit-adventurers), [Quaternius](https://quaternius.com/packs/modularcharacteroutfitsfantasy.html)) [read]. I found no maintained open-source voxel character creator with a reusable part library [read; weak], so the creator is yours to design: integer sliders plus hash-addressed part variants, each with a licence entry [inference].

**Animation.** Adopt VRM's bone names with a normalised T-pose (+Z forward, Y up, metres) as the canonical skeleton: it is the common ground of VRM, Godot's humanoid profile and the Khronos draft ([Godot](https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/retargeting_3d_skeletons.html)) [read; the intersection is inference]. CC0 motion sets (Quaternius, 120+ clips; KayKit, 29) come in through a one-time converter ([library](https://quaternius.com/packs/universalanimationlibrary.html), [KayKit](https://kaylousberg.itch.io/kaykit-animations)) [read]. Mixamo is free to use, but a community post says raw files may not be redistributed or used to train models (Adobe's FAQ returned 403) ([post](https://community.adobe.com/questions-696/mixamo-faq-licensing-royalties-ownership-eula-and-tos-589400)) [secondary], so it is bring-your-own. No Rust retargeting library turned up [read; weak], so a small retargeter is M [inference]. Drive cosmetic animation by tick, never wall-clock [inference]. MikuMikuDance (PMX, VMD) is the biggest legacy pool and a licensing minefield (per-model readme terms); the MIT `mmd-anim` crates allow an optional bring-your-own plugin, M, never bundled ([mmd-anim](https://github.com/yohawing/mmd-anim)) [read].

**Showing avatars.** (a) Render the skinned mesh as is: a wgpu skinning pass, M, but out of place beside voxel terrain [inference]. (b) Voxelise at load into a rigged voxel figure, recommended: voxelise the rest pose at a fixed pitch (a 1.6 m avatar is 32 voxels tall at 5 cm), sample textures at triangle centres, assign each voxel to the dominant bone of the nearest skinned vertex, and draw one rigid group per bone or blend two bones in the vertex shader; M for a first version, L for quality [inference]. I found no project that voxelises a skinned avatar per bone [read; weak]; obj2voxel (MIT) is a static reference ([obj2voxel](https://github.com/eisenwave/obj2voxel)) [read]. (c) Map a humanoid onto a blocky skeleton, the 15 required VRM bones as the floor (S to M) [inference]. Combine (b) and (c). If a world voxel is human-sized, as today's two-voxel figure suggests, avatar voxels need a finer grid, shared with the Sculpt idea [inference].

**The stream rule.** Appearance must travel as a small, versioned record (namespaced ids, hashes, a few integers) and each file as a hash-addressed blob, requested on demand and embedded in recordings [inference from the project rules]. Veloren does this for plugin bundles: the server lists plugin hashes, the client requests the missing ones and caches them by hash ([server.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/net/src/msg/server.rs), [client.rs](https://gitlab.com/veloren/veloren/-/raw/master/common/net/src/msg/client.rs)) [read]. Your content-package decision defers hash caching to "the phase that adds assets" (`docs/decisions/packs-and-content.md`); this is that phase. Build in: re-encode every incoming skin PNG to canonical 64 by 64 RGBA with metadata stripped and bounded decoder memory; let the server, not the client, fetch from third-party services, outside the simulation; cap bones, voxels and message rates on peer data; keep a licence block (author, SPDX licence identifier, redistribution and modification flags, AI-generated flag) beside every blob so the server can refuse to relay; and reference third-party avatars by hash in replays, embedding them only where the licence allows [inference].

## Accessories, held tools and sockets: one rig, named sockets, two part kinds

| Place | Roblox [read] | Hytale slots [read] | Cubzh [read] | Bedrock creator [read] | Decentraland [secondary] |
|---|---|---|---|---|---|
| Head top | HatAttachment | headAccessory | hair | Headwear | hat, helmet, top_head |
| Face | FaceFront, FaceCenter | face, faceAccessory | painted into palette | Face Item | eyewear, mask, eyes, mouth |
| Torso | BodyFront, layered shirt | undertop, overtop | jacket | Top, Outerwear | upper_body |
| Back | BodyBack | cape | none | Back Item, Capes | rigid to a spine bone |
| Legs, feet | layered pants, shoes | pants, overpants, shoes | pants, boots | Bottom, Footwear | lower_body, feet |

([Bedrock](https://minecraft.wiki/w/Character_Creator), [Decentraland](https://docs.decentraland.org/creator/wearables-and-emotes/wearables/creating-wearables.md).) The common set I propose [inference]: named sockets on a small canonical rig (`head_top`, `hair`, `face_front`, `neck`, `shoulder_l/r`, `chest_front`, `back`, `waist_front/center/back`, `hand_l/r` with a grip locator, `foot_l/r`) and an alias table for the others' names. Each part declares a category, its sockets, an origin aligned to the socket frame, a hides list and a required-slot default as in Decentraland. Tint channels come from palette indices or material names (skin, hair, eyes, two accents), as in Veloren's index conventions, Cubzh's eight-slot palette and Hytale's gradient sets. The format must state one number, part voxels per world block, and allow two part kinds: box-tree parts (Minecraft, Bedrock, Hytale, Blockbench) and voxel-grid parts (`.vox`, `.3zh`, Veloren).

Held tools need a grip point: Bedrock's player geometry has a `lead_hold` locator on the right-item bone ([geometry](https://wiki.bedrock.dev/visuals/player-geometry)) [read]. For seats and engines, two early-stage glTF extensions are ready vocabularies: `OMI_seat` (back, foot and knee positions so inverse kinematics seats any humanoid) and `OMI_vehicle_thruster` (maximum force in newtons, gimbal angle, thrust along local +Z) ([OMI_seat](https://github.com/omigroup/gltf-extensions/blob/main/extensions/2.0/OMI_seat/README.md)) [read]; they map onto the command seat and rocket engine [inference]. Hytale lets servers toggle armour-slot visibility and requires listed servers to render the official cosmetic set, a precedent for a core set every server must render so show-off items stay portable ([Update 2](https://hytale.com/news/2026/1/hytale-patch-notes-update-2)) [read].

## Authoring tools: a Blockbench plugin reaches the most creators for the least work

**Blockbench first.** Blockbench is a free low-poly modeller, animator and texture painter (desktop and browser), GPL-3.0-or-later (version 5.2.2); its README exempts plugins and themes that use its API from the GPL, so they may be open source, proprietary or paid, and everything made with Blockbench belongs to its maker ([README](https://cdn.jsdelivr.net/gh/JannisX11/blockbench@master/README.md), [package.json](https://cdn.jsdelivr.net/gh/JannisX11/blockbench@master/package.json)) [re-read]. Hytale's official art route is a Blockbench plugin (GPL-3.0, TypeScript, public) for `.blockymodel` and `.blockyanim`, a complete template to copy ([plugin](https://github.com/JannisX11/hytale-blockbench-plugin), [about](https://cdn.jsdelivr.net/gh/JannisX11/blockbench-plugins@master/plugins/hytale_plugin/about.md)) [read]. A plugin is one JavaScript file registered with `Plugin.register`; a game-format plugin defines a `ModelFormat` (feature flags for bone rigs and attachment locators) and a `Codec` (compile, parse, export options) ([docs](https://www.blockbench.net/wiki/docs/plugin), [format.ts](https://cdn.jsdelivr.net/gh/JannisX11/blockbench@master/js/io/format.ts)) [read]. The `.bbmodel` project format has no complete specification and changed incompatibly at 4.9, 4.10 and 5.0, and 5.0 broke plugin APIs too ([bbmodel](https://www.blockbench.net/wiki/docs/bbmodel), [5.0 notes](https://newreleases.io/project/github/JannisX11/blockbench/release/v5.0.0)) [read], so use `.bbmodel` only as an import convenience [inference]. With a Voxunami format, Blockbench's own converters bring Java, Bedrock and OptiFine projects into it inside the editor, so the game never parses `.bbmodel` [inference]. An export-only first version is S to M once the game-side format exists; round trip with animations, attachments and a live link is M to L.

**The store's rules on AI-assisted code** matter because this project is largely written with an AI coding assistant. As re-read from the store repository's README ([README](https://cdn.jsdelivr.net/gh/JannisX11/blockbench-plugins@master/README.md)) [re-read]: code written with an AI coding assistant is allowed only if the author retains full understanding of how it works; "vibe-coded" plugins are not accepted; no AI-generated art may appear anywhere in a plugin, including the icon and the about page; and plugins that use generative AI are not accepted into the repository but may be shared externally (read beside the assistant clause, that means plugins with generative-AI features, not code written with an assistant [inference]). The same file limits a plugin to 2 MB, forbids executing code from external sources and obfuscation, requires public, human-readable, reproducible source for any bundled plugin, limits icons to 48 to 96 pixels and 12 KB, and makes a licence file optional [re-read]. For this project: AI-assisted plugin code is permitted, but the submitter must be able to explain all of it, and the rule gives no test for "full understanding", so a named person must own the code [inference]; icon and about images must be human-made; and any feature that calls a generative model (an assistant building hats through the [Blockbench MCP plugin](https://github.com/jasonjgardner/blockbench-mcp-plugin), a Model Context Protocol server, say) cannot go in the store but can be shared by installing from a file or HTTPS URL, which Blockbench supports ([plugin loader](https://cdn.jsdelivr.net/gh/JannisX11/blockbench@master/js/plugin_loader.ts)) [read].

| Candidate | What it unlocks | Licence | Effort | Verdict |
|---|---|---|---|---|
| Blockbench plugin | Creator reach; Minecraft-format conversion inside the editor | GPL-3.0-or-later; plugins free [re-read] | S to M export-only; M to L full | Do first |
| MagicaVoxel `.vox` | The common voxel interchange; no plugin API; Windows and macOS freeware; 256 voxels per axis, 255 colours, 90-degree rotations only | Spec repository MIT; software proprietary [read] | S to M import | File level ([site](https://ephtracy.github.io/mv_main.html), [spec](https://github.com/ephtracy/voxel-model)) |
| vengi | Lists about 50 voxel and mesh formats; direction per format unverified | MIT [read] | S to document; M to launch | External converter ([formats](https://vengi-voxel.github.io/vengi/Formats/)) |
| glTF (Blender and others), Goxel | Generic fallback input; Goxel is a free Linux-native editor | `gltf` crate MIT OR Apache-2.0 [read]; Goxel GPL-3.0 [read] | M with voxelisation | Needed for ships; recommend Goxel, do not embed |
| Aseprite, Pixelorama | PNG templates; "send to game" extensions | Aseprite EULA; Pixelorama MIT [read] | S each | Optional |
| Foxel, Asset Forge, GIMP 3, Krita | Scripting hooks for one-click export: Lua in Foxel (Pro tier) and Asset Forge, Python in GIMP 3 and Krita ([Foxel](https://foxel.io/), [Asset Forge](https://kenney.nl/tools/asset-forge), [GIMP](https://www.gimp.org/release-notes/gimp-3.0.html), [Krita](https://docs.krita.org/en/user_manual/python_scripting/introduction_to_python_scripting.html)) [read] | Foxel and Asset Forge are paid [read]; GIMP and Krita licences unchecked | S each, later | Low priority |

## Builds and ships: Minecraft schematics first, a mesh voxeliser second

| Source | What it unlocks | Openness | Effort | Verdict |
|---|---|---|---|---|
| Minecraft Java builds: Sponge `.schem` v3, vanilla `.nbt`, `.litematic` | Modrinth lists 15,748 datapacks; GrabCraft 6,000+ objects, 71 spaceships [read] | Palette of state strings plus index array; Sponge repository states no licence; Litematica has no specification [read] | S reader; M mapping | Do first |
| `.vox` ships and props | Teardown Workshop, itch.io kits | MIT spec [read] | S to M | With move 2 |
| Mesh libraries (glTF, OBJ, STL) | NASA, Kenney, Quaternius, Smithsonian, Sketchfab downloads | Per-asset licences | M | Needed anyway |
| Space Engineers 1 blueprints | Workshop with more than 500,000 creations, not split by type [read] | XML [secondary]; the EULA limits mods to use inside the game [read] | L vanilla; XL with mods | Highest ceiling; defer |
| Scrap Mechanic | Block-based JSON [secondary]; two GPL-3.0 converters [read] | No official schema | M | Next ship game |
| LDraw, Studio `.io` | 17,230 unique parts [read] | Text spec; licence disputed | M | Good |

([Sponge v3](https://github.com/SpongePowered/Schematic-Specification/blob/master/versions/schematic-3.md), [structure files](https://minecraft.wiki/w/Structure_Block_file_format), [Modrinth datapacks](https://api.modrinth.com/v2/search?limit=1&facets=%5B%5B%22project_type%3Adatapack%22%5D%5D), [GrabCraft](https://www.grabcraft.com/), [Space Engineers release](https://gamespress.com/Space-Engineers-2-Expands-with-UGC-Workshop-and-More-in-Vertical-Slice), [EULA](https://www.spaceengineersgame.com/eula/), [Scrap Mechanic tool](https://github.com/QuestionableM/SM-Converter), [LDraw library](https://library.ldraw.org/).) Kerbal Space Program `.craft` files name parts but hold no geometry, so a converter needs the player's install ([KerbalX](https://kerbalx.com/about)) [read; inference]; StarMade and Empyrion use undocumented binary formats that break each update [secondary]. Defer all three.

**Minecraft builds and the mapping problem.** Sponge, vanilla and Litematica files share one idea, a palette of flattened `namespace:name[property=value]` strings plus a dense index array, matching your namespaced ids [read]; Minecraft blocks are 1 m, and your two-voxel player figure suggests your voxels are close to that [inference]. Rust readers exist (`mc_schem` and `rustmatica`, both GPL-3.0-only; `nucleation`, MIT per its README) ([mc_schem](https://lib.rs/crates/mc_schem), [nucleation](https://lib.rs/crates/nucleation)) [read], and a hand-written reader is about a week [inference]. The work is mapping. No published study says how large a table covers 90 percent of real builds [read; weak]; my unmeasured, low-confidence estimate is that a few hundred explicit ids plus family rules (wood types by form, stone families, sixteen colours across wool, concrete, terracotta and glass) cover most block instances but far fewer whole builds, since one rare block fails a build without a fallback [inference]. Fallback chain: exact table, family rule, average texture colour on the nearest shape, then a preserved unknown id; measure real coverage on 50 to 200 sample files first [inference]. minecraft-data (legacy ids, collision boxes) is believed MIT, but that is [secondary]: its LICENSE file returned 404 when re-checked ([minecraft-data](https://github.com/PrismarineJS/minecraft-data)). The usual universal block dataset, Amulet's PyMCTranslate, is proprietary: all rights reserved, licence to be purchased ([LICENSE](https://raw.githubusercontent.com/Amulet-Team/PyMCTranslate/master/LICENSE)) [re-read]. Register imported blocks as `minecraft:...` ids owned by an optional plugin, so the rule that missing-plugin data is preserved byte for byte gives a lossless round trip and lets a better mapping re-run later; keep provenance (source, author, licence snapshot, import date, local-only flag) in every imported file [inference].

**Ship games and brick systems.** Keen's converter from Space Engineers 1 to 2 is the model for mapping: convert known blocks, replace unknown ones with armour, report what was dropped ([Grid Exporter](https://2.spaceengineersgame.com/experienced-players/grid-exporter/)) [read]. Rescale densities so an imported ship's mass equals the source's declared mass and pick engine counts so thrust matches, so a converted ship flies like the original [inference]. In LDraw a brick is 20 by 20 by 24 units and a plate 8 high ([spec](https://www.ldraw.org/article/218.html)) [read], so a 4-unit cubic voxel makes a brick 5 by 5 by 6, a plate 5 by 5 by 2 and a stud 3 by 3 by 1 exactly; expose "voxels per stud" (1, 2 or 5) [inference]. The parts library is under the Creative Commons Attribution Licence 2.0 through the LDraw.org Contributor Agreement, and the legal page announces no change ([legal](https://www.ldraw.org/legal-info.html)) [re-read]; a converted library needs a credits file, and LEGO's owner does not endorse the site [read].

**Mesh to voxel.** One voxeliser is the single door to NASA, Kenney, Quaternius and Smithsonian models and downloaded Sketchfab files. Surface voxelisation by triangle-box overlap is standard (a reference test carries an MIT-style licence) ([code](https://fileadmin.cs.lth.se/cs/Personal/Tomas_Akenine-Moller/code/tribox3.txt)) [read]; solid fill needs a watertight mesh or winding numbers ([paper](https://igl.ethz.ch/projects/winding-number/)) [read]. Downloaded spaceships are open shells, and walking players need cavities, so offer a shell of chosen thickness, flood fill from outside to find sealed cavities and keep them hollow, plus a solid mode [inference]. Options must include up axis, forward axis, handedness, unit and target length, since wrong handedness swaps port and starboard [inference]. Conversion runs at tool time or on the client, and the server loads only the resulting ship file, so float use here does not touch the deterministic simulation [inference]. NASA content is generally uncopyrighted in the US, with insignia protected ([NASA](https://www.nasa.gov/nasa-brand-center/images-and-media/)) [read]; Kenney's Space Kit is CC0 ([Space Kit](https://kenney.nl/assets/space-kit)) [read]. KitBash's page says ArtStation and Sketchfab are moving from Epic Games to KitBash, with no change for creators and no date or deal status ([KitBash](https://cart-checkout.kitbash3d.com/pages/kitbash-acquires-sketchfab-and-artstation-kitbash3d)) [re-read], so do not build on Sketchfab's programming interface. For optional offline photo-to-model use, TRELLIS.2 is MIT but needs a 24 GB NVIDIA GPU ([TRELLIS.2](https://github.com/microsoft/TRELLIS.2)) [read]; Hunyuan3D is a dead end (see below).

**Share strings and cards.** A Factorio-style blueprint string (a version byte, then base64 of zlib-compressed JSON) makes a ship pasteable into chat ([format](https://wiki.factorio.com/Blueprint_string_format)) [read]. A PNG screenshot with the payload in an ancillary chunk survives where hidden-pixel schemes such as Cosmoteer's die under any re-encode ([decoder](https://gist.github.com/GameDungeon/e783ee9147e5990bcbfa8273b9406676)) [read; the comparison is inference]; hosts that strip chunks defeat both, so keep the string too. A QR code holds at most 2,953 bytes ([capacity](https://www.qr-code-generator.com/blog/qr-code-data-capacity/)) [secondary], enough for a link or hash, not a ship [inference].

## Textures and resource packs: a mapping file turns Minecraft packs into block pictures

Minecraft Java resource packs are the large texture pool: 38,281 projects on Modrinth on 2026-11-13 ([Modrinth](https://api.modrinth.com/v2/search?limit=1&facets=%5B%5B%22project_type%3Aresourcepack%22%5D%5D)) [read]. A pack is a folder or zip with `pack.mcmeta`, block pictures under `assets/<namespace>/textures/block/`, animation as a vertical strip with a `.mcmeta` file, and JSON models ([resource pack](https://minecraft.wiki/w/Resource_pack)) [read]. Four tiers. Tier one maps Voxunami blocks to pack picture names through a mapping file, takes frame zero of animations and resamples to square power-of-two sizes of 8 to 256 pixels, which your texture step already accepts (`docs/decisions/packs-and-content.md`): S to M. Tier two resolves the real face-to-picture binding through blockstates and models (M). Tier three adds connected-texture rules, up to 47 tiles per block (L) ([OptiFine rules](https://optifine.readthedocs.io/ctm.html)) [read]. Tier four converts LabPBR normal and specular maps offline into your single JPEG XL file (S to M plus shader work) ([LabPBR](https://shaderlabs.org/wiki/LabPBR_Material_Standard)) [read]. Tier one is the target. Model JSON is drifting (year-based version names, lifted rotation limits, a removed shading field), so pin one supported pack format ([Model](https://minecraft.wiki/w/Model)) [read]. Legally, many packs are Mojang-derived or custom-licensed [inference]: Faithful's licence version 4 allows use in non-Minecraft projects with credit but forbids paywalls, AI training and crypto use and can change at any time ([Faithful](https://www.faithfulpack.net/license)) [read]. Whether Mojang's upstream rights allow another game is open [inference], so read packs the user supplies and ship none. Clean alternatives are Luanti's 132 texture packs, in an index that admits only free licences ([policy](https://content.luanti.org/policy_and_guidance)) [read].

## Controls, tracking and input: build the action system, then let devices plug into it

**The action system is the deliverable.** Define typed actions (button, one axis, two axes) in contexts (walk, seat, map, build, suit thrusters), bound through processors (invert, dead zone, curve, sensitivity, hold or toggle, chords) per device profile, saved in a versioned file keyed by namespaced action ids, unknown ids preserved verbatim [inference from the project's save rule]. Every importer then becomes a pure function (bytes in, scheme and "unmapped" report out), shippable as a sandboxed mod, and Microsoft's Xbox Accessibility Guideline 107 (remappable actions, per-axis inversion, no required holds or simultaneous presses, keyboard-only operation) becomes an acceptance test ([guideline](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/107)) [read]. Keep default bindings stable: about 810,000 community Steam layouts existed in March 2026 and work for non-Steam games ([stats](https://www.steaminputdb.com/news/2026-03-03_community_config_stats)) [read], so default keys, not action ids, are the compatibility surface [inference].

**Devices.** Use SDL3 for joysticks, HOTAS (hands-on-throttle-and-stick controllers) and gamepads, with winit for keyboard and mouse: SDL3 brings hot-plug, the mapping database, gyro, rumble, DualSense effect passthrough and, since a May 2026 merge, native support for the new Steam Controller ([gamepad API](https://wiki.libsdl.org/SDL3/CategoryGamepad), [releases](https://github.com/libsdl-org/SDL/releases)) [read, secondary]. The `sdl3` crate is 0.22.1 and warns to expect bugs ([crate](https://docs.rs/crate/sdl3/latest)); `gilrs` 0.11.2 (Apache-2.0 OR MIT) is the pure-Rust fallback, with no documented sensor interface and, by default on Windows, input only while the window has focus ([gilrs](https://docs.rs/gilrs/latest/gilrs/)) [read]. SDL's own zlib licence is [recalled], not checked. For head tracking, opentrack (mostly ISC) relays webcam, infrared, phone and eye-tracker poses as UDP packets of six little-endian doubles, 48 bytes ([opentrack](https://github.com/opentrack/opentrack), [layout](https://xrlinuxdriver.readthedocs.io/en/latest/opentrack-listener/)) [read, secondary]; a receiver is S and the best value per week here, though its default port could not be confirmed. Elite Dangerous `.binds` files are plain XML documented only by a community schema, devices named by hex vendor and product id ([example](https://gist.github.com/vishless/35f9e4442a48002ae675b39c91b5b48d), [schema](https://github.com/ngollan/Elite-Binds)) [read]. The six flight axes map cleanly onto thrust and attitude and most other actions do not; an importer is S to M for a modest audience (about 4,000 concurrent Steam players) ([Steambase](https://steambase.io/apps/359320)) [secondary], and whether Elite's `Joy_n` numbering matches SDL's indices is unverified.

**Avatar-driving protocols.** The Virtual Motion Capture protocol (VMC, version 3.1) is Open Sound Control (OSC) over UDP, default ports 39539 and 39540, meant for one local network, and is the VTuber world's common wire format ([VMC](https://protocol.vmc.info/specification)) [read]; VRChat's OSC interface (receive on 9000, send on 9001) is the second ([OSC](https://docs.vrchat.com/docs/osc-overview)) [read]. Rust crates `vmc` and `rosc` are Apache-2.0 OR MIT ([vmc](https://github.com/pykeio/vmc)) [read]. This is M and needs the avatar system first. Tracking data is wall-clock by nature, so the client must quantise it and send it as a cosmetic pose message through the server at a limited rate, which lands in recordings [inference]. OpenXR is L to XL and should wait: its action model matches the registry, hand and eye gaze are ratified extensions, no cross-vendor body or face extension is ratified, and wgpu has no first-party OpenXR path ([bindings](https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrSuggestInteractionProfileBindings.html), [hand tracking](https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_EXT_hand_tracking.html)) [read, secondary]. Do not depend on Steam Input or the Steamworks SDK (redistribution terms against the GPL and AGPL unread); skip Tobii's software kit (EUR 1,495 a year, commercial use listed "No") and EMG armbands (Meta's release is CC-BY-NC) ([Tobii](https://www.tobii.com/products/integration/tobii-streams-sdk), [Meta](https://github.com/facebookresearch/generic-neuromotor-interface)) [read].

## Modding infrastructure: a configured WebAssembly host, a plain package channel, data-first behaviour

**Wasmtime and the October 2026 advisories.** Wasmtime's determinism page recommends only deterministic host imports (virtualise clocks and files), NaN canonicalisation, deterministic or disabled relaxed SIMD, deterministic memory and table growth (a limiter, up-front allocation or validation), and fuel rather than epoch interruption ([determinism](https://docs.wasmtime.dev/examples-deterministic-wasm-execution.html)) [re-read]. The defaults do none of it: fuel is off, NaN canonicalisation is off, relaxed SIMD is on ([Config](https://docs.wasmtime.dev/api/wasmtime/struct.Config.html)) [read]. Two Critical advisories (severity score 9.9 of 10) were published on 2026-10-19 [re-read]. GHSA-897f-g5gm-fmxm: Cranelift packs value indices in a way that can overflow silently, so a very large or crafted function can miscompile and escape the sandbox on any platform; it affects releases below 36.0.19 and from 37.0.0 up to below 43.0.0; the only patched release it names is 36.0.19, 43 and later no longer have the bug, there is no workaround, and the page names no fix for 37 to 42 ([advisory](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-897f-g5gm-fmxm)) [re-read]. GHSA-mghf-wjfp-p5xm: on aarch64 only, a `br_table` with more than about 256,000 targets jumps to a wrong place; patched in 36.0.19, 48.0.7 and 49.0.4; no workaround ([advisory](https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-mghf-wjfp-p5xm)) [re-read]. The list's first page shows ten advisories, all from 2026 ([list](https://github.com/bytecodealliance/wasmtime/security/advisories)) [re-read], including a High-severity escape from the WebAssembly System Interface (WASI) filesystem sandbox (title only) [read]. Release policy: every release numbered a multiple of 12 is supported for 24 months, others for 2, and security fixes are backported to every supported release [re-read]; patch releases for 36.x, 48.x, 49.x and 50.x all shipped on 2026-10-29 ([releases](https://github.com/bytecodealliance/wasmtime/releases)) [read]. From this [inference]: pin a line that receives backports, meaning a long-term line (36.x or 48.x now), since an arbitrary major from 37 to 42 would have been the worst choice; import no WASI at all, which removes that advisory class; cap module and function size before compiling (untested as a mitigation, since the advisory lists no workaround); record the engine version in replays, because fuel accounting can shift across compiler versions; and run the same mod ticks on Wasmi 2.0, an interpreter released on 2026-09-01 with built-in fuel and a `deterministic` feature, as a differential-test oracle and as a fallback without a just-in-time compiler ([Wasmi](https://github.com/wasmi-labs/wasmi/releases)) [read].

**Scripting for in-game ship computers.** Do not embed stock Lua or Luau natively. Stock Lua seeds its string hash from the time and a stack address ([lauxlib.c](https://raw.githubusercontent.com/lua/lua/master/lauxlib.c)) [read], so iteration order can differ per process [inference], and maths go through the host library, whose transcendental precision Rust's documentation says varies by platform ([f32 docs](https://doc.rust-lang.org/std/primitive.f32.html)) [read]. Factorio patched ordering, maths and bytecode loading ([Factorio](https://lua-api.factorio.com/latest/auxiliary/libraries.html)) [read], and Luanti builds using LuaJIT fell to a sandbox escape scored 9.3 of 10 published on 2026-04-16 ([CVE-2026-40959](https://cve.circl.lu/vuln/cve-2026-40959)) [read]. The cheapest deterministic route is the official Lua 5.4 compiled to WebAssembly and run in the same host, as wasmoon does ([wasmoon](https://github.com/ceifa/wasmoon)) [read]; the determinism argument is mine and a spike must confirm it [inference]. Starlark is deterministic by design (Apache-2.0) but I found no documented step limit ([starlark-rust](https://github.com/facebook/starlark-rust)) [read].

**Distribution.** No free, open, generic channel exists for a third-party game [inference].

| Channel | Fit | Terms | Verdict |
|---|---|---|---|
| Own manifest plus hash-addressed packages on GitHub Releases or a container (OCI) registry, signed | Full control; the hash is the identity; unauthenticated GitHub allows 60 requests an hour per address ([limits](https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api)) [read], so host your own index [inference] | Free | Lowest regret |
| Steam Workshop | Probably the biggest audience [inference]; needs the game's AppID ([implementation](https://partner.steamgames.com/doc/features/workshop/implementation)) [read]; installs happen outside your control | A 2026 worm hit People Playground's Workshop ([report](https://www.gamingonlinux.com/2026/02/steam-game-people-playground-hit-by-malware-via-the-steam-workshop/)) [read]; Steamworks redistribution unread | Later mirror |
| mod.io | Generic user-content service with a REST interface | Terms not verified | Later mirror |
| CurseForge | Listing needs an API key | Third-party terms forbid caching API data ([terms](https://support.curseforge.com/en/support/solutions/articles/9000207405-curseforge-3rd-party-api-terms-and-conditions)) [read] | Skip |
| Modrinth | `.mrpack` accepts only `minecraft` as the game | No third-party registration found ([format](https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack)) [read] | Copy the design |
| Thunderstore, Luanti ContentDB | Self-hostable portals | Both AGPL-3.0, forkable by an AGPL project ([ContentDB](https://content.luanti.org/help/api/), [Thunderstore](https://api.github.com/repos/thunderstore-io/Thunderstore/license)) [read] | Option, L |

Copy the `.mrpack` design: an index with both hashes, per-side environment, an allowed-host whitelist and path validation [read]. Signatures are optional trust metadata above the hash; the `sigstore` Rust crate is still experimental ([sigstore-rs](https://github.com/sigstore/sigstore-rs)) [read]. The threat is real: a 2026 worm spread through a Steam Workshop game [read], and trojanised Minecraft mods spread malware ([WeedHack](https://www.pcgamesn.com/minecraft/weedhack-malware)) [secondary]. Packages hold only data, WebAssembly and size-capped assets, never executables, with hashes verified before and after install [inference].

**Mob AI as data.** Define your own small vocabulary: priority-ordered goals as in Bedrock, optional utility scores, behaviour tree or state chart, as JSON with a generated schema [inference]. Bedrock proves that a goal list with priorities carries a whole bestiary ([panic example](https://learn.microsoft.com/en-us/minecraft/creator/reference/content/entityreference/examples/entitygoals/minecraftbehavior_panic)), though its samples are under Mojang's EULA, not an open licence [read]; Hytale's roles, instructions and sensors are the nearest modern analogue ([NPC docs](https://hytalemodding.dev/en/docs/official-documentation/npc/5-sleep-behavior)) [read]. The Rust AI crates are Bevy-coupled or stale, so use them as test oracles [read]. For determinism use integer or fixed-point scores and tie-breaks by id [inference]. BehaviorTree.CPP's XML is a possible exchange format so the Groot2 editor works on Voxunami trees; its free tier suffices for editing ([XML](https://www.behaviortree.dev/docs/learn-the-basics/xml_format/), [Groot](https://www.behaviortree.dev/groot/)) [read]. Effort M for the core, L for breadth.

**Learned "brains" as data.** Ship a fixed-operation network file (tensor blob, operation list, sensor-and-action manifest) run by an in-house scalar evaluator [inference]. Use ONNX and NNEF only as offline import formats: tract uses hand-rolled SIMD kernels and makes no determinism claim ([tract](https://github.com/sonos/tract)) [read]. For bit-identical inference use scalar loops, fixed accumulation order, no transcendental library calls (`libm` or tables) and no auto-vectorised reductions, or integer-quantised weights [inference]. The strongest verified precedent for a brain as a shareable file is Creatures, whose genomes and exported creatures players bred and moved between worlds ([explainer](https://www.alanzucconi.com/2020/07/27/the-ai-of-creatures/)) [secondary]; I found no verified precedent for a mod-distributed neural policy that is bit-identical across platforms, which is the novel part [inference]. Effort M for the evaluator, L to XL for training.

## Licensing and terms: what may come in, and what to decide while you are the only copyright holder

**What may come in.** Inbound compatibility is excellent under GPL-3.0-or-later or AGPL-3.0-or-later. MIT, modified BSD, zlib and CC0 are compatible; Apache-2.0 is compatible with GPLv3; MPL-2.0 through its secondary-licence clause; CC BY 4.0 suits art, models and sounds but not code ([licence list](https://www.southampton.ac.uk/~qs2g22/mirror/www.gnu.org/licenses/license-list.html)) [read]. CC BY-SA 4.0 is one-way compatible with GPLv3, CC BY-SA 3.0 is not, and non-commercial or no-derivatives licences are non-free ([Creative Commons](https://creativecommons.org/share-your-work/licensing-considerations/compatible-licenses/)) [read]. A GPL-3.0-or-later tree can ingest only "or-later" or more permissive files, so check headers: GitHub's detector says only "GPL-3.0" for the Hytale plugin and Goxel [read]. Keep differently licensed material in separate packs: bundling independent works does not extend the GPL to them [read].

| Source | Read the player's file | Bundle it | Copy its code |
|---|---|---|---|
| Veloren | Yes | Assets entry by entry; never the non-commercial items [re-read] | Yes, GPL-3.0-or-later [re-read] |
| Blockbench, Hytale plugin | Yes | n/a | Yes; check "only" versus "or-later" |
| MagicaVoxel `.vox` | Yes | Spec files MIT; not the software | MIT repository |
| Minecraft | Not addressed by anything readable | No; even Mojang's samples are all rights reserved [read] | No |
| Hytale | Yes; the EULA lets players share models and skins [read] | No; mods may not include game files [read] | GPL plugin only |
| VRM models | Yes | By flag [read] | n/a |
| NASA, Kenney, Quaternius | Yes | Yes (NASA insignia excepted) [read] | n/a |
| Sketchfab | Per model | CC0, CC BY, CC BY-SA 4.0 only; no scraping [read] | n/a |
| Space Engineers, Steam Workshop items | Blueprint reading unaddressed | No [read] | No |
| Mixamo, Roblox, Kerbal Space Program, VRoid Hub, Thingiverse | Terms unread | No | No |

([Hytale EULA](https://hytale.com/eula), [Steam agreement](https://store.steampowered.com/subscriber_agreement/), [Luanti licence](https://raw.githubusercontent.com/luanti-org/luanti/master/LICENSE.txt).)

**Code, data and mods.** The FSF's GPL FAQ (read through a university mirror) says a program's licence generally does not govern output made from the user's data, and that game artwork or audio may be licensed separately [read]. By mode [inference]: compile-time Rust crates are part of the program; JSON packs and PNGs are separate data; sandboxed WebAssembly mods using a narrow interface of ids and messages lean "separate", but no court has applied the test to WebAssembly. The clean answer is a GPLv3 section 7 additional permission for the mod interface, or permissive licensing of interface and format crates, as Blockbench did for plugins [re-read]; it needs every copyright holder's consent, so decide while you are the only one. Under the AGPL, an operator who modifies the server must offer source to remote players, while served third-party content stays under its own licence [inference]. Supporting a file format is the safe part: the European Union's top court held in case C-406/10 that data file formats are not protected expression ([summary](https://en.wikipedia.org/wiki/SAS_Institute_Inc_v_World_Programming_Ltd)) [secondary], and US copyright excludes methods of operation ([17 USC 102](https://www.law.cornell.edu/uscode/text/17/102)) [read]. The risks are end-user licence agreements, circumvention (never decrypt or extract protected archives), trademarks and likeness. Hytale's gated Shared Source Agreement is confidential and has a non-compete, which supports your decision not to read it ([agreement](https://hytale.com/shared-source-license)) [read].

**Guardrails** [inference unless tagged]. (1) An SPDX identifier and copyright line in every source file, and a sidecar or REUSE annotation on every asset ([REUSE](https://reuse.software/spec-3.3/)) [read]. (2) A licence block in `pack.json`: SPDX expression, class (code, data, media), authors, source, attribution required, redistribution and commercial flags, `ai_generated`. (3) Bundle only CC0, public-domain-by-statute, MIT, BSD, Apache-2.0, zlib, OFL fonts, CC BY 4.0, CC BY-SA 4.0 and GPL-compatible code; refuse non-commercial, no-derivatives, "all rights reserved", unknown-author and platform-restricted content. (4) Honour VRM flags: the uploader attests authorship when `avatarPermission` is `onlyAuthor`, relay only where redistribution is allowed, add no technical measures. (5) A server content policy: relay only licence classes the operator allows, keep non-commercial content on the client, and put an uploader grant and warranty in server terms (Hytale, Steam and Space Engineers do) [read]. (6) No third-party marks in names or icons: say "can import X files" with a non-affiliation line, and publish your own trademark file (GPLv3 section 7 allows withholding trademark rights). (7) Clean-room discipline: record which public specification or sample files each reader came from. (8) Parse untrusted files in a sandbox with caps on sizes, depths, counts and archive expansion, and fuzz every reader. (9) An AI-generated flag: the US Copyright Office's January 2025 report ([office](https://www.copyright.gov/ai/)) [read], as summarised elsewhere, treats machine-determined expression as unprotectable [secondary]; Veloren refuses AI content [re-read]; Luanti requires disclosure [read]. (10) Refuse token-gated and NFT-linked content, as Hytale does in mods [read]. (11) Choose a developer certificate of origin (DCO) or a contributor licence agreement (CLA) before taking outside code: Veloren's contributing page mentions neither [re-read], and Spigot adopted a CLA after a DMCA takedown ([Bukkit](https://en.wikipedia.org/wiki/Bukkit)) [read].

## Wild options, ranked by interest and feasibility

| Rank | Idea | Closest real precedent | Effort | Note |
|---|---|---|---|---|
| 1 | "Voxunami card": a PNG chunk, paste string and QR code carrying an avatar, ship or control scheme, plus a replay bookmark that reopens the exact seed, tick and camera | Character-card PNGs, whose embedding is a community convention [secondary] ([card spec](https://github.com/malfoyslastname/character-card-spec-v2)); Factorio blueprint strings; Open Badges baked PNGs ([Open Badges](https://www.imsglobal.org/spec/ob/v3p0/)) [read] | S to M | I found no precedent for the bookmark; only a deterministic simulation allows it [inference] |
| 2 | Hash-derived default looks and signed badge entitlements | DiceBear seeds [read]; Minecraft capes tied to promotions ([Cape](https://minecraft.wiki/w/Cape)) [read]; default skins by UUID [recalled] | S | A unique look per player with zero upload |
| 3 | Open telemetry and input port: loopback WebSocket with a versioned schema, plus an opt-in OSC input channel | Elite Dangerous journal files [secondary]; Telemachus for Kerbal Space Program ([Telemachus](https://teaguild.github.io/Telemachus-1/guides/introduction/)) [secondary]; VRChat OSC [read] | S to M | Bind loopback, require a token; input enters as ordinary intents |
| 4 | Fly real ships: Veloren's galleon, NASA spacecraft with real masses in Kepler orbits | Keen's Grid Exporter; NASA models [read] | S to M | Mind insignia and per-model credits |
| 5 | A scripting VM inside the sandbox: ship computers, drone APIs | kOS for Kerbal Space Program, GPL-3.0 ([kOS](https://github.com/KSP-KOS/KOS)); CC:Tweaked ([CC:Tweaked](https://github.com/cc-tweaked/CC-Tweaked)); WASM-4 cartridges with rollback ([WASM-4](https://wasm4.org/docs/)) [read] | M | CC:Tweaked is mixed-licensed [read]: use a clean-room API [inference] |
| 6 | Write VRM 1.0 out, so blocky avatars work in VTuber tools | OxiHuman exports VRM 1.0 [read] | M | Makes Voxunami a source of VRM content |
| 7 | "Voxunami Bridge": an in-game key and a Blockbench panel editing server assets live | The community Hytale Bridge: an in-game `/blockbench` command gives a key, and a Blockbench panel browses server assets ([Bridge](https://cdn.jsdelivr.net/gh/JannisX11/blockbench-plugins@master/plugins/hytale_bridge/about.md)) [read] | M to L | Assets reach clients as hash-addressed blobs |
| 8 | Replay-verified feats: condition id, replay hash, server signature, baked into the card | Speedrun verification by re-simulation ([nesdev](https://nesdev.org/wiki/TAS)) [secondary]; Open Badges 3.0 [read] | M to L | Uptake unknown |
| 9 | Avatar from a Bluesky handle: the server resolves it and sends a few quantised pixels | Bluesky's public profile interface ([API](https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile?actor=bsky.app)) [read] | S | Impersonation and privacy risks |
| 10 | Phone or tablet as second screen and gyro stick | Browser phone-controller projects [secondary]; motion sensors need HTTPS ([MDN](https://developer.mozilla.org/en-US/docs/Web/API/DeviceMotionEvent)) [read] | M | Self-signed certificate friction |

## Dead ends, with reasons

| Dead end | Why (provenance) |
|---|---|
| Joining Veloren servers; Veloren crates; its server browser | Nightly, Specs, build-hash handshake, no external protocol specification found [read]; a listing needs Veloren's official authentication server and weekly release channel ([README](https://gitlab.com/veloren/serverbrowser/-/raw/master/README.md)) [read] |
| Ready Player Me and hosted avatar services | Shut down 2026-01-31 [re-read]; paid tiers priced for companies [read] |
| PyMCTranslate and Amulet data; Axiom `.bp` | Proprietary or closed [re-read] |
| Hytale gated server source; live Hytale avatars | Confidential with a non-compete; no client mods [read] |
| Roblox, VRChat and Resonite avatar containers; NFT platforms | Closed containers; terms unread or restrictive [read] |
| Hunyuan3D 2.x | Licence excludes the European Union, United Kingdom and South Korea and bars using outputs to improve other models ([licence](https://huggingface.co/tencent/Hunyuan3D-2.1/blob/main/LICENSE)) [re-read] |
| Gaia star data in the repository | CC BY-NC 3.0 IGO; ESA wants a request before any commercial use ([licence](https://cosmos.esa.int/web/gaia-users/license)) [re-read] |
| Sketchfab's interface as a dependency | Ownership changing [re-read]; no integration roadmap yet [read] |
| Qubicle, VoxEdit formats, a MagicaVoxel plugin interface | End of development; proprietary; none exists [read] |
| Stock Lua or Luau natively; mods as in-process libraries | Nondeterministic or unsandboxable [read] |
| Mixamo files bundled; the VRoid Hub software kit; VRChat as a VRM target | Redistribution barred [secondary]; Unity-only; no native VRM [read] |
| Tobii's software kit, EMG armbands, direct OpenVR | Licence-gated or non-commercial [read]; superseded by OpenXR [secondary] |
| Foreign-protocol clients and servers (Luanti, Minecraft crates) | Small hobby projects, protocol churn, demand unevidenced [read; inference] |
| Scraping NameMC, Skindex or ContentDB | They block automated access; use Mojang's interface and player files [read] |

## Where the sources disagree

| Topic | The disagreement | Treatment |
|---|---|---|
| LDraw parts licence | The legal page says Creative Commons Attribution Licence 2.0 [re-read]; a later document ([contributor agreement](https://www.ldraw.org/docs-main/licenses/ldraw-org-contributor-agreement.html)) is reported to say CC BY 4.0, with older parts possibly under both [secondary] | The legal page shows no change [re-read]; credit authors, check per part |
| Sketchfab | Earlier coverage said it would fold into Fab [secondary]; KitBash says it is moving from Epic to KitBash [re-read]; one outlet dates the sale 2026-08-10 ([report](https://digitalproduction.com/2026/08/12/kitbash-buys-artstation-and-sketchfab/)), another 11 August ([Japanese outlet](https://gaming-st.com/news/epic-sells-artstation-sketchfab-kitbash/)) [secondary] | Date and closing unconfirmed |
| Ready Player Me founders | TechCrunch's correction says only the chief technology officer moves; Engadget says the chief executive stays ([Engadget](https://www.engadget.com/gaming/netflix-is-acquiring-game-avatar-maker-ready-player-me-204443001.html)), and only TechCrunch covers the shutdown [re-read] | Unresolved; irrelevant to the shutdown |
| Amulet's licence | PyMCTranslate: all rights reserved, licence to be purchased [re-read]; the Amulet-Core 2.0 branch was read saying the same ([LICENSE](https://github.com/Amulet-Team/Amulet-Core/blob/2.0/LICENSE)) [read], but a re-check got 404 at the paths tried; older free builds were listed MIT on SourceForge ([mirror](https://sourceforge.net/mirror/amulet-map-editor/profile/)) [secondary] | Treat as proprietary for new work |
| Gaia data | ESA's licence page says CC BY-NC 3.0 IGO [re-read]; release documentation ([credit page](https://gea.esac.esa.int/archive/documentation/GDR3/Miscellaneous/sec_credit_and_citation_instructions/)) says open and free with credit [read] | The licence page wins |
| Open avatar sets | Open Source Avatars' repository ([repository](https://github.com/ToxSam/open-source-avatars)) records per-collection licences, one CC BY; its website ([site](https://www.opensourceavatars.com)) says all CC0 [read]. MakeHuman says exports are CC0 [read]; a wrapper says AGPL [secondary] | Trust the repository and the project |
| `nucleation` licence | README ([GitHub](https://github.com/schem-at/nucleation)) says MIT; lib.rs says MIT and possibly AGPL-3.0-only for the meshing sub-crate [read] | Treat the meshing part as AGPL |

## Open questions only the owner can answer

1. **Licence.** GPL-3.0-or-later or AGPL-3.0-or-later; a mod exception or permissive interface and format crates; DCO or CLA. Decide while you are the only copyright holder.
2. **Relay policy.** Will the server relay third-party avatars, skins and models between players, and for which licence classes? My default: CC0, CC BY, CC BY-SA and GPL, or flag-permitted; everything else stays on the owner's client.
3. **Scale and rig.** How many avatar voxels per world block; one canonical VRM-named rig or Veloren-style fixed parts; is a finer-grid entity system in scope (it is shared with Sculpt)?
4. **Mojang material.** After someone reads the EULA and Usage Guidelines, do you want skins, schematics and resource packs as user-supplied imports?
5. **Ship computers.** In scope? A Lua-in-WebAssembly spike, Starlark, or WebAssembly only?
6. **Distribution.** Own manifests first? Do you plan a Steam release, which brings the Workshop and Steamworks licence questions?
7. **Generative AI.** Allowed in bundled content with a flag, or refused as Veloren does? Who owns the understanding of AI-assisted Blockbench plugin code?
8. **Virtual reality.** Wanted at all? It costs L to XL.
9. **Token-gated content.** Refuse it outright (my recommendation)?

## Suggested order of first steps

1. **No code, this week.** Read Mojang's pages in a browser; settle the licence questions; write the guardrails (SPDX headers, the `pack.json` licence block, the content allowlist). S.
2. **Tests, then the carrier.** Golden tests for the appearance-record codec and the blob request, response and replay-embedding messages, hostile-input caps included; then build them. M.
3. **Textured-box figure and sockets.** Extend the entity box path to a textured box tree with the canonical rig and named sockets; add Minecraft skin import from a local file and the hash-derived default look. M plus S.
4. **Voxel-grid parts.** A `.vox` reader through `dot_vox` with a palette policy, socketed accessories and a per-part licence block. S to M.
5. **Blockbench plugin, export-only,** once the format is frozen, with a human-made icon, submitted to the store and installable by URL as a fallback. S to M.
6. **WebAssembly host,** configured as above, in parallel; importers are its first mods. M.
7. **Builds and ships.** Interchange model, Sponge `.schem` and vanilla `.nbt` readers with mapping tables as data packs, the mesh voxeliser and share strings. M to L.
8. **Then in any order:** the Veloren snapshot importer (once the entity system takes voxel bodies), resource-pack tier one, VRM and VRMA, the action system and input channels. Later: the Virtual Motion Capture receiver, OpenXR, a scripting VM, brains, a portal.

**Cheap to decide now, costly later.** (a) The licence, "only" versus "or-later", the mod exception, DCO or CLA. (b) The appearance record's shape, a version field, the blob hash (SHA-256 is Veloren's choice) and whether replays embed or reference blobs. (c) Part scale in voxels per world block, and socket names. (d) Whether a part carries its own palette, as Veloren's index conventions and Cubzh's eight slots do, or draws from the game's palette. (e) Stable action ids and default bindings. (f) WebAssembly interfaces with no WASI imports, versioned per feature, since one added function broke Veloren plugins [read]. (g) Provenance and licence fields in every pack, model and ship file, and a namespace such as `minecraft:` for foreign content so unknown ids survive. (h) Waiting for the Khronos names before writing any glTF avatar writer [inference].

## Sources

Every URL cited above, listed once under the section where it first appears.

### The short version

- https://gitlab.com/veloren/veloren/-/raw/master/Cargo.toml
- https://gitlab.com/veloren/veloren/-/raw/master/LICENSE
- https://gitlab.com/veloren/veloren/-/raw/master/assets/credits.ron
- https://gitlab.com/veloren/veloren/-/raw/master/CONTRIBUTING.md
- https://crates.io/api/v1/crates/veloren-common
- https://gitlab.com/veloren/veloren/-/raw/master/rust-toolchain
- https://crates.io/api/v1/crates/assets_manager
- https://gitlab.com/veloren/veloren/-/raw/master/voxygen/src/scene/figure/load.rs
- https://gitlab.com/veloren/veloren/-/raw/master/common/src/figure/cell.rs
- https://gitlab.com/veloren/veloren/-/raw/master/common/net/src/msg/server.rs
- https://gitlab.com/veloren/veloren/-/raw/master/common/state/src/plugin/module.rs
- https://gitlab.com/api/v4/projects/veloren%2Fveloren/merge_requests/5105
- https://book.veloren.net/players/roadmap.html

### Veloren

- https://book.veloren.net/contributors/artists/voxel-models.html
- https://gitlab.com/veloren/veloren/-/raw/master/assets/voxygen/voxel/humanoid_head_manifest.ron
- https://gitlab.com/veloren/veloren/-/raw/master/common/src/figure/mod.rs
- https://gitlab.com/veloren/veloren/-/raw/master/assets/common/manifests/ship_manifest.ron
- https://gitlab.com/api/v4/projects/veloren%2Fveloren/repository/tree?path=assets/voxygen/voxel/npc&per_page=100
- https://gitlab.com/api/v4/projects/veloren%2Fveloren/repository/tree?path=assets/voxygen/voxel/armor&per_page=100
- https://gitlab.com/veloren/veloren/-/raw/master/voxygen/anim/src/character/run.rs
- https://book.veloren.net
- https://gitlab.com/veloren/veloren/-/raw/master/common/src/comp/body/humanoid.rs
- https://gitlab.com/veloren/veloren/-/raw/master/CHANGELOG.md
- https://gitlab.com/veloren/veloren/-/raw/master/common/state/src/plugin/mod.rs
- https://gitlab.com/api/v4/projects/veloren%2Fveloren/issues/908
- https://book.veloren.net/contributors/modders/writing-a-plugin.html
- https://github.com/cpetig/veloren-plugin-canoe
- https://gitlab.com/veloren/veloren/-/raw/master/common/src/generation.rs
- https://gitlab.com/veloren/veloren/-/raw/master/rtsim/src/ai/mod.rs
- https://gitlab.com/api/v4/projects/veloren%2Fveloren
- https://veloren.net/blog/update
- https://gitlab.com/veloren/veloren/-/raw/master/plugin/wit/LICENSE
- https://gitlab.com/veloren/veloren/-/raw/master/common/src/astar.rs

### Avatars

- https://github.com/vrm-c/vrm-specification/tree/master/specification
- https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_vrm-1.0/humanoid.md
- https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_vrm_animation-1.0/README.md
- https://vrm.dev/en/vrm1/
- https://www.moguravr.com/?p=148493
- https://github.com/nanowater/vrm-runtime-rs
- https://github.com/not-elm/bevy_vrm1
- https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_vrm-1.0/meta.md
- https://vrm.dev/licenses/1.0/
- https://developer.vroid.com/api/oauth-api.html
- https://github.com/KhronosGroup/glTF/pull/2512
- https://github.com/KhronosGroup/glTF/blob/main/extensions/README.md
- https://metaverse-standards.org/wp-content/uploads/MSF-3D-Assets-BOF-SIGGRAPH-Jul26.pdf
- https://github.com/omigroup/gltf-extensions
- https://techcrunch.com/2025/12/19/netflix-acquires-gaming-avatar-maker-ready-player-me/
- https://avaturn.me/pricing
- https://minecraft.wiki/w/Skin
- https://raw.githubusercontent.com/bs-community/skinview3d/master/src/model.ts
- https://minecraft.wiki/w/Mojang_API
- https://hytale.com/news/2025/12/an-introduction-to-making-models-for-hytale
- https://github.com/hytale-tools/blockymodel-merger
- https://hytale.com/eula
- https://hytale.com/news/2025/11/hytale-modding-strategy-and-status
- https://github.com/bliporg/blip
- https://raw.githubusercontent.com/bliporg/blip/main/cubzh-file-format-3zh.txt
- https://raw.githubusercontent.com/bliporg/blip/main/lua/modules/avatar.lua
- https://create.roblox.com/docs/avatar/rigid-accessories/specifications
- https://adr.decentraland.org/adr/ADR-57
- https://raw.githubusercontent.com/decentraland/adr/main/LICENSE.md
- https://static.makehumancommunity.org/about/license.html
- https://static.makehumancommunity.org
- https://github.com/cool-japan/oxihuman
- https://github.com/M3-org/CharacterStudio
- https://github.com/LiberatedPixelCup/Universal-LPC-Spritesheet-Character-Generator
- https://www.dicebear.com/licenses/
- https://kenney.nl/assets/blocky-characters
- https://kaylousberg.itch.io/kaykit-adventurers
- https://quaternius.com/packs/modularcharacteroutfitsfantasy.html
- https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/retargeting_3d_skeletons.html
- https://quaternius.com/packs/universalanimationlibrary.html
- https://kaylousberg.itch.io/kaykit-animations
- https://community.adobe.com/questions-696/mixamo-faq-licensing-royalties-ownership-eula-and-tos-589400
- https://github.com/yohawing/mmd-anim
- https://github.com/eisenwave/obj2voxel
- https://gitlab.com/veloren/veloren/-/raw/master/common/net/src/msg/client.rs

### Accessories, held tools and sockets

- https://minecraft.wiki/w/Character_Creator
- https://docs.decentraland.org/creator/wearables-and-emotes/wearables/creating-wearables.md
- https://wiki.bedrock.dev/visuals/player-geometry
- https://github.com/omigroup/gltf-extensions/blob/main/extensions/2.0/OMI_seat/README.md
- https://hytale.com/news/2026/1/hytale-patch-notes-update-2

### Authoring tools and editor plugins

- https://cdn.jsdelivr.net/gh/JannisX11/blockbench@master/README.md
- https://cdn.jsdelivr.net/gh/JannisX11/blockbench@master/package.json
- https://github.com/JannisX11/hytale-blockbench-plugin
- https://cdn.jsdelivr.net/gh/JannisX11/blockbench-plugins@master/plugins/hytale_plugin/about.md
- https://www.blockbench.net/wiki/docs/plugin
- https://cdn.jsdelivr.net/gh/JannisX11/blockbench@master/js/io/format.ts
- https://www.blockbench.net/wiki/docs/bbmodel
- https://newreleases.io/project/github/JannisX11/blockbench/release/v5.0.0
- https://cdn.jsdelivr.net/gh/JannisX11/blockbench-plugins@master/README.md
- https://github.com/jasonjgardner/blockbench-mcp-plugin
- https://cdn.jsdelivr.net/gh/JannisX11/blockbench@master/js/plugin_loader.ts
- https://ephtracy.github.io/mv_main.html
- https://github.com/ephtracy/voxel-model
- https://vengi-voxel.github.io/vengi/Formats/
- https://foxel.io/
- https://kenney.nl/tools/asset-forge
- https://www.gimp.org/release-notes/gimp-3.0.html
- https://docs.krita.org/en/user_manual/python_scripting/introduction_to_python_scripting.html

### Builds and ships

- https://github.com/SpongePowered/Schematic-Specification/blob/master/versions/schematic-3.md
- https://minecraft.wiki/w/Structure_Block_file_format
- https://api.modrinth.com/v2/search?limit=1&facets=%5B%5B%22project_type%3Adatapack%22%5D%5D
- https://www.grabcraft.com/
- https://gamespress.com/Space-Engineers-2-Expands-with-UGC-Workshop-and-More-in-Vertical-Slice
- https://www.spaceengineersgame.com/eula/
- https://github.com/QuestionableM/SM-Converter
- https://library.ldraw.org/
- https://kerbalx.com/about
- https://lib.rs/crates/mc_schem
- https://lib.rs/crates/nucleation
- https://github.com/PrismarineJS/minecraft-data
- https://raw.githubusercontent.com/Amulet-Team/PyMCTranslate/master/LICENSE
- https://2.spaceengineersgame.com/experienced-players/grid-exporter/
- https://www.ldraw.org/article/218.html
- https://www.ldraw.org/legal-info.html
- https://fileadmin.cs.lth.se/cs/Personal/Tomas_Akenine-Moller/code/tribox3.txt
- https://igl.ethz.ch/projects/winding-number/
- https://www.nasa.gov/nasa-brand-center/images-and-media/
- https://kenney.nl/assets/space-kit
- https://cart-checkout.kitbash3d.com/pages/kitbash-acquires-sketchfab-and-artstation-kitbash3d
- https://github.com/microsoft/TRELLIS.2
- https://wiki.factorio.com/Blueprint_string_format
- https://gist.github.com/GameDungeon/e783ee9147e5990bcbfa8273b9406676
- https://www.qr-code-generator.com/blog/qr-code-data-capacity/

### Textures and resource packs

- https://api.modrinth.com/v2/search?limit=1&facets=%5B%5B%22project_type%3Aresourcepack%22%5D%5D
- https://minecraft.wiki/w/Resource_pack
- https://optifine.readthedocs.io/ctm.html
- https://shaderlabs.org/wiki/LabPBR_Material_Standard
- https://minecraft.wiki/w/Model
- https://www.faithfulpack.net/license
- https://content.luanti.org/policy_and_guidance

### Controls, tracking and input

- https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/107
- https://www.steaminputdb.com/news/2026-03-03_community_config_stats
- https://wiki.libsdl.org/SDL3/CategoryGamepad
- https://github.com/libsdl-org/SDL/releases
- https://docs.rs/crate/sdl3/latest
- https://docs.rs/gilrs/latest/gilrs/
- https://github.com/opentrack/opentrack
- https://xrlinuxdriver.readthedocs.io/en/latest/opentrack-listener/
- https://gist.github.com/vishless/35f9e4442a48002ae675b39c91b5b48d
- https://github.com/ngollan/Elite-Binds
- https://steambase.io/apps/359320
- https://protocol.vmc.info/specification
- https://docs.vrchat.com/docs/osc-overview
- https://github.com/pykeio/vmc
- https://registry.khronos.org/OpenXR/specs/1.1/man/html/xrSuggestInteractionProfileBindings.html
- https://registry.khronos.org/OpenXR/specs/1.1/man/html/XR_EXT_hand_tracking.html
- https://www.tobii.com/products/integration/tobii-streams-sdk
- https://github.com/facebookresearch/generic-neuromotor-interface

### Modding infrastructure

- https://docs.wasmtime.dev/examples-deterministic-wasm-execution.html
- https://docs.wasmtime.dev/api/wasmtime/struct.Config.html
- https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-897f-g5gm-fmxm
- https://github.com/bytecodealliance/wasmtime/security/advisories/GHSA-mghf-wjfp-p5xm
- https://github.com/bytecodealliance/wasmtime/security/advisories
- https://github.com/bytecodealliance/wasmtime/releases
- https://github.com/wasmi-labs/wasmi/releases
- https://raw.githubusercontent.com/lua/lua/master/lauxlib.c
- https://doc.rust-lang.org/std/primitive.f32.html
- https://lua-api.factorio.com/latest/auxiliary/libraries.html
- https://cve.circl.lu/vuln/cve-2026-40959
- https://github.com/ceifa/wasmoon
- https://github.com/facebook/starlark-rust
- https://docs.github.com/en/rest/using-the-rest-api/rate-limits-for-the-rest-api
- https://partner.steamgames.com/doc/features/workshop/implementation
- https://www.gamingonlinux.com/2026/02/steam-game-people-playground-hit-by-malware-via-the-steam-workshop/
- https://support.curseforge.com/en/support/solutions/articles/9000207405-curseforge-3rd-party-api-terms-and-conditions
- https://support.modrinth.com/en/articles/8802351-modrinth-modpack-format-mrpack
- https://content.luanti.org/help/api/
- https://api.github.com/repos/thunderstore-io/Thunderstore/license
- https://github.com/sigstore/sigstore-rs
- https://www.pcgamesn.com/minecraft/weedhack-malware
- https://learn.microsoft.com/en-us/minecraft/creator/reference/content/entityreference/examples/entitygoals/minecraftbehavior_panic
- https://hytalemodding.dev/en/docs/official-documentation/npc/5-sleep-behavior
- https://www.behaviortree.dev/docs/learn-the-basics/xml_format/
- https://www.behaviortree.dev/groot/
- https://github.com/sonos/tract
- https://www.alanzucconi.com/2020/07/27/the-ai-of-creatures/

### Licensing and terms

- https://www.southampton.ac.uk/~qs2g22/mirror/www.gnu.org/licenses/license-list.html
- https://creativecommons.org/share-your-work/licensing-considerations/compatible-licenses/
- https://store.steampowered.com/subscriber_agreement/
- https://raw.githubusercontent.com/luanti-org/luanti/master/LICENSE.txt
- https://en.wikipedia.org/wiki/SAS_Institute_Inc_v_World_Programming_Ltd
- https://www.law.cornell.edu/uscode/text/17/102
- https://hytale.com/shared-source-license
- https://reuse.software/spec-3.3/
- https://www.copyright.gov/ai/
- https://en.wikipedia.org/wiki/Bukkit

### Wild options

- https://github.com/malfoyslastname/character-card-spec-v2
- https://www.imsglobal.org/spec/ob/v3p0/
- https://minecraft.wiki/w/Cape
- https://teaguild.github.io/Telemachus-1/guides/introduction/
- https://github.com/KSP-KOS/KOS
- https://github.com/cc-tweaked/CC-Tweaked
- https://wasm4.org/docs/
- https://cdn.jsdelivr.net/gh/JannisX11/blockbench-plugins@master/plugins/hytale_bridge/about.md
- https://nesdev.org/wiki/TAS
- https://public.api.bsky.app/xrpc/app.bsky.actor.getProfile?actor=bsky.app
- https://developer.mozilla.org/en-US/docs/Web/API/DeviceMotionEvent

### Dead ends

- https://gitlab.com/veloren/serverbrowser/-/raw/master/README.md
- https://huggingface.co/tencent/Hunyuan3D-2.1/blob/main/LICENSE
- https://cosmos.esa.int/web/gaia-users/license

### Where the sources disagree

- https://www.ldraw.org/docs-main/licenses/ldraw-org-contributor-agreement.html
- https://digitalproduction.com/2026/08/12/kitbash-buys-artstation-and-sketchfab/
- https://gaming-st.com/news/epic-sells-artstation-sketchfab-kitbash/
- https://www.engadget.com/gaming/netflix-is-acquiring-game-avatar-maker-ready-player-me-204443001.html
- https://github.com/Amulet-Team/Amulet-Core/blob/2.0/LICENSE
- https://sourceforge.net/mirror/amulet-map-editor/profile/
- https://gea.esac.esa.int/archive/documentation/GDR3/Miscellaneous/sec_credit_and_citation_instructions/
- https://github.com/ToxSam/open-source-avatars
- https://www.opensourceavatars.com
- https://github.com/schem-at/nucleation

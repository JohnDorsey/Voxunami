# Seed secrecy: how seeds are cracked, what stops it, and what this engine does today (a survey, 2026-11-25)

**Why it was written.** The owner's intent, 2026-11-25, in short: every planet a player reaches should have materials that exist nowhere else in the universe; players explore, find a material with a wanted look or property, and sell it to others; that fails if holding a block of a material lets anyone work out where it came from. The server should never send a seed, only a generated pack, and should hold several seeds, some of them used to make others (a planet's seed, say), so that cracking one does not give away the more important ones. The owner asked for research on how seed cracking is prevented and for a review of that plan against it. **This document is the research; the review is in `procedural-materials-spec.md`.** Nothing here is built.

Marks: **[read]** in this repository; **[measured]** run by `spikes/seed_probe/` (one machine: Ryzen 5 7600X, 12 threads, Windows 10, release build; Linux not run); **[web]** read on the web, with how (the page itself, or only a summary from the search tool, which can be wrong); **[recalled]** from my memory, not checked; **[believed]** my reasoning, not run.

## 1. In short

- **Every seed crack on record has the same shape**: a *small* secret (Minecraft's effective 48 bits), a *weak, structured* generator (a linear congruential generator), and a *public, deterministic* map from the secret to something the attacker can see, so that each observation is a constraint on the secret. **[web: search summaries; one measurement of the generator below]**
- **Randar** (Minecraft Beta 1.8 to 1.12.2) is the case nearest the owner's worry. No seed was sent. But one generator that world generation had last reseeded also placed dropped items, so the position of one dropped item gave the generator's state, and the state gave **where a chunk had last been loaded: the position of other players**. A location leaked through an unrelated output. **[web: the authors' explanation; its first ~40,000 characters read verbatim by a fetching agent, the rest only as a summary]**
- **The cure has one shape too**: a secret of at least 128 bits from the operating system's random source; every value derived from it through a *keyed cryptographic hash with its own purpose for each use* (HKDF, HMAC, or BLAKE3's keyed and `derive_key` modes), never through a mixer or a game's generator; the secret never seeding anything players can observe; and clients sent outputs, not keys.
- **The engine's own seed mixing cannot be that derivation, and this is measured, not argued.** `terrain::mix` is a bijection (ten million random values: its inverse restored every one), a child seed made with the engine's lattice hash gives its parent back in **about 6 ns** (5.7 and 7.1 in two runs), and one full-precision property computed with the engine's value noise gives its seed back after at most 2,048 tries (**about 40 µs**: 39.2 and 43.0 in two runs). **[measured]** A hierarchy of seeds made with the engine's usual hash would protect nothing.
- **The size of the secret is a number.** A 32-bit secret searched through a keyed BLAKE3 took **26.1 s** on this machine's 12 threads (164.5 million candidates a second, the whole space). At that rate 2^64 is **3,554 years** of this machine; at a published GPU rate for SHA-256 (22.1 billion a second; BLAKE3 not measured on a GPU) it is about **26 years** of one top GPU or **10 days** of a thousand; 2^128 is out of reach of everyone. **[measured, then extrapolated]**
- **What decides the market's fate is mostly not cryptography**: the ids players see, the order registry numbers were given in, what the server will answer, and what a saved or replicated world file contains. Each can give the origin away with perfect cryptography underneath. Section 4.5; the review is in `procedural-materials-spec.md`.

## 2. How seeds are cracked

### 2.1 Minecraft's structure seeds **[web: search summaries, not the primary sources]**

- Minecraft's structure placement uses Java's `Random`, a 48-bit linear congruential generator, and most generation uses only the lower 48 bits of the 64-bit world seed. A *structure seed* therefore has 2^48 values, and the other 16 bits come from the biomes. Mods such as SeedCrackerX read what the client sees (structures, features, biomes) and narrow the candidates until one seed is left; a *lattice reduction* (LLL) can find the generator's state from an observed placement directly instead of searching all 2^48.
- **Measured for contrast** with Java's constants and our own implementation: **three observed floats gave the whole 48-bit state in 1.2 ms** (at most 2^24 low parts to try), and the state could be stepped back 5,000 places and forward 5,000 and land on itself. A generator like that is as easy to run backwards as forwards.

### 2.2 Randar **[web: the README of spawnmason/randar-explanation; first ~40,000 characters verbatim through a fetching agent on the second try (the first failed with a dropped connection), the rest only as the agent's summary, marked here]**

- **What leaked, and what an attacker had to observe:** the positions of dropped items when blocks are broken. A bot that mined blocks and recorded each drop's coordinates could recover the server's generator state. Beta 1.8 to 1.12.2 (the code was reused "until 1.13"; the 2b2t server stayed on 1.12.2 until 14 August 2023). Recordings made by a mod in those versions can reveal past positions afterwards.
- **The mechanism:** on every chunk load the server checks for a woodland mansion by calling `setRandomSeed` on the *shared global* generator, reseeding it from the world seed and the 80 by 80 chunk region's coordinates; the placement of a drop then draws three `nextFloat()` values from the same generator. So a drop's position exposes state that the latest chunk load set. The page calls it lazy coding and suggests a fresh generator for each check.
- **The attack:** each `nextFloat` is the top 24 bits of the 48-bit state, so three of them pin it; the page gives a brute force over the 2^24 low parts (the same search as the measurement in 2.1), steps the generator *backwards* with the modular inverse of its multiplier, and checks each state against a table of region seeds (about 2^32 entries, about 9 GB of GPU memory); it reports about ten million seeds a second on an RTX 3090 and "a few thousand" steps back to find the latest chunk load. One block break is the observation. *(The lattice-reduction method, a bound of 5,000 steps and a ten-nanosecond crack on a laptop are only in the agent's summary of the unread rest: not relied on.)*
- **The lessons it states, as the verbatim part gave them:** shared generator state that is reseeded predictably leaks through unrelated outputs; the generator is weak and its structure exploitable; generation should be deterministic, but gameplay randomness should not share that state.

### 2.3 What the cases share **[believed: my reading of 2.1 and 2.2]**

1. **A small secret.** 2^48 is hours of a GPU (the table in 4.1 says 3.5 hours for the cost of a hash each).
2. **A structured generator.** A linear generator turns observations into linear equations that algebra or lattice reduction solve; the secret is not searched, it is *computed*.
3. **A public, deterministic map from secret to something visible.** Every client has the generator, so the world is an oracle and each observation cuts the candidates.
4. **One secret behind everything**: any one leak opens all of it.
5. **State shared between what generates the world and what players see** (Randar), and **public constants** (the salts of structure types) where a secret was needed.

### 2.4 The hashed seed **[recalled; not confirmed by three searches]**

I recall that since Minecraft 1.15 the server sends the client a *hashed seed* (the first 8 bytes of SHA-256 of the seed) that the client uses for biome noise. Three searches did not confirm it (one result said a hashed seed field was added to the join-game and respawn packets, citing the protocol history, which I did not read). **Not relied on.** If it is so, it sends a derived value for the client's use instead of the seed, and a 64-bit seed under a hash is still searchable (2^64), so it hides the seed's text without making it secret. **[believed]**

## 3. What servers do about it **[web: descriptions of third-party plugins and a Paper patch's file name; I installed and tested none]**

- Spigot's `spigot.yml` has some 21 `seed-*` keys, the *salts* that structure placement mixes with the world seed; **SeedVeil**, a Paper plugin, randomises them (its page). Paper has a patch called "Configurable feature seeds" (the setting `feature-seeds.generate-random-seeds-for-all`). **SeedShield** replaces each structure type's salt with a value derived from a secret key. **AntiSeedCracker** also spoofs the seed in packets and blocks commands that show it.
- What they say they do not do: biome layout and the End's pillar heights still follow the seed; chunks generated before the change keep the old structures; the measures "raise the cost" and are not a guarantee (SeedVeil's page).
- **Reading [believed]:** this is the right shape (a secret for each feature, derived from a key) applied late, to a game whose clients compute terrain from the seed, so the part that matters most (biomes) cannot be changed. The owner's plan starts where Minecraft could not: **the secret part is a separate layer that clients never compute.**

## 4. What stops it, in principle

### 4.1 A secret too big to search **[measured, web]**

One keyed BLAKE3 hash per candidate on this machine's 12 threads, searching a whole space of secrets of `bits` bits (the secret was the last candidate): 2^24 in 0.10 s (174.1 million a second), 2^28 in 1.60 s (168.3 million), 2^32 in 26.12 s (164.5 million). The rest is that rate extended, with **one hash per candidate**, which is the cheapest check an attacker can have: they check the cheapest property they can see, not a whole material.

| Secret | This machine (164 million a second) | One RTX 4090 at 22.1 GH/s | A thousand of them |
|---|---|---|---|
| 2^32 | 26 s | 0.2 s | instant |
| 2^40 | 1.9 hours | 50 s | 50 ms |
| 2^48 (Minecraft's structure seed) | 19.8 days | 3.5 hours | 13 s |
| 2^56 | 13.9 years | 37.7 days | 54 minutes |
| **2^64** | **3,554 years** | **26.5 years** | **9.7 days** |
| 2^80 | 2.3e8 years | 1.7e6 years | 1,700 years |
| 2^96 | 1.5e13 years | 1.1e11 years | 1.1e8 years |
| **2^128** | 6.6e22 years | 4.9e20 years | 4.9e17 years |

- The GPU figure is hashcat's published speed for **SHA-256** on an RTX 4090 (22,138.4 MH/s in version 7.0.0; 21,188.1 in 6.2.6), **not BLAKE3, and not run here**; read the columns as orders of magnitude. **[web: hashcat forum posts]**
- A historical anchor: the distributed.net search for one 64-bit RC5 key took **1,757 days** and 331,252 participants and ended in 2002. **[web: news reports of it]** Hardware is far faster now; the point is that 64 bits are within reach of an organised effort and 128 are not.
- **My reading [believed]:** 64 bits protects a game's market from a hobbyist and not from someone who cares enough and rents a thousand GPUs for ten days; 128 bits ends the question. A game should take 128 and not argue; the root can be 256 at no cost (a large root also covers an attacker who attacks many servers at once, where the cost divides by the number of targets).

### 4.2 One-way derivation, one purpose for each use **[web, measured]**

- RFC 5869 (HKDF) says the optional `info` input exists to bind the derived key material to application and context, and should be independent of the input keying material. **[web: the RFC and search summaries of it]** BLAKE3's `derive_key` takes a context string that its documentation says should be hardcoded, globally unique and specific to the application (with a date and a purpose in it), and the maintainers' issue 13 says that building the context from dynamic data breaks the separation, so data that varies goes in the key material or the message, not the context. **[web: docs.rs and the issue, by search summaries]**
- **Cost [measured]:** `blake3::derive_key` with a context and a 40-byte input, **109 ns**; `blake3::keyed_hash` of 8 bytes, **54 ns** (single thread, ten million calls each). Deriving a key for each planet and each material is free next to making one brick (5 ms).
- **What goes wrong without it [measured]:** see section 5. The mechanism is the same as 2.3(2): a bijective or linear derivation is *computed backwards*, not searched.
- **Standards exist for the shape**, so the derivation can be pinned to published test vectors, which turns a silent mistake into a failing test: RFC 5869 has test cases in its appendix; BLAKE3 publishes test vectors. **[recalled for both; not read]**

### 4.3 Do not let what players see come from the secret's own stream **[web: Randar; read: our code]**

Randar's lesson as stated: generation deterministic, gameplay randomness separate. The engine's `lattice::voxel_random(seed, tick, grid, voxel, purpose)` hashes the world seed with `mix` for rules that decide by chance (fire, for one) **[read: `engine/lattice/src/random.rs`; `games/voxunami_cave/src/plugins/fire.rs` passes `world.world_seed`]**. That is fine today because that seed is public. **It would leak any secret passed to it as `seed`, by the same cause as Randar**: the outcomes players see are functions of the state. A rule for any future secret: *a secret never seeds a non-cryptographic generator whose output a client can observe.*

### 4.4 Send outputs, not keys, and make the outputs unlinkable **[believed]**

Seed cracking needs observations that are a function of the secret through a map the attacker knows. If the server computes materials from the secret and sends only the results, the attacker's only handle is the results themselves, and with a keyed hash underneath those say nothing about the key. This is stronger than a big secret alone, because it still holds if a generator inside has a weak spot. A *leaf* seed may be sent when it is a one-way image of the key (a texture's noise seed, say): recovering it from the pixels gives only itself.

### 4.5 The numbers around the numbers **[read, believed]**

- **Identifiers** a player can see (ids, names, a pack's name, a file's name) must carry nothing of the origin: no planet number, no counter, no time. A keyed hash of the planet and the material's number does, and cannot be guessed or linked.
- **Order.** A dense registry numbers things in the order they were made. Materials made together (one planet's) get consecutive numbers, so a client that holds two materials with consecutive numbers has learnt that they share a planet, and the order says which was found first. The engine's registries are dense and append-only, and the world description a client receives lists them **[read: `Welcome::world_description`, `RegistryAdditions`]**.
- **Oracles.** Any answer that depends on the secret for a place the client is not at is a handle for a search: a scan, a map entry, a "planet info" request.
- **Files.** A world file holds the seed in its header, and a world description (sent to every client) holds the seed and the recipes **[read]**; a secret needs a place that no description, replicated copy, recording or log ever includes.

## 5. What this engine does today

| Fact | How known | Consequence |
|---|---|---|
| `terrain::mix` is splitmix64's output function: a bijection of `u64`. Its inverse (three xor-shifts and two multiplications by modular inverses) restored 10,000,000 random inputs exactly, both ways; `mix(0)` is the published first output | **[measured]** | nothing derived through it is one-way |
| A child seed made as the engine hashes (`lattice_hash(parent, number, 0, 0)`) gives the parent back from the child and its number: 5.7 ns and 7.1 ns each in two runs, five million repeats | **[measured]** | "planet seeds derived from a master seed" with the engine's hash expose the master to anyone who knows one planet seed and its number |
| One property computed as `lattice_value(material_seed, 1, 0, 0)` (a float with 53 hash bits) gives the material seed back: the 11 missing bits are tried (2,048), the second property checks: found at try 1,621 in 39.2 µs and 43.0 µs in two runs | **[measured]** | a property made with the engine's value noise from a secret gives the secret away |
| Properties quantised to a few bits each are *not* shown safe by this: I did not try to invert them and **[believed]** a structured attack may; the answer is not to rely on their being coarse | **[believed]** | use a keyed hash |
| Java's 48-bit generator: three floats give the state in 1.2 ms, and it steps backwards as easily as forwards | **[measured]** | the Minecraft failure, reproduced |
| `blake3::derive_key` 109 ns, `keyed_hash` 54 ns; a 32-bit secret fully searched in 26.1 s on 12 threads | **[measured]** | the numbers in 4.1 and 4.2 |
| The world seed (`u64`, in `engine:header`) and every recipe's parameters, which hold each planet's seed, are in the world description every client gets, and clients make the terrain from them | **[read: `engine_sections.rs`, `Welcome`, `GridTerrainState`; Cave's planet family writes its seed as the first 8 bytes of its parameters]** | **terrain seeds are public by design**; they are not the place for a secret, and cracking them is moot |
| The only cryptographic hash in the tree is `sha2` (the pack hashes); `ed25519-dalek` and `rand_core` with the OS random source are in `identity`; **no** `blake3`, `hmac` or `hkdf` | **[read: `Cargo.lock`, `engine/identity`]** | a derivation function is a new dependency of one kind or another (`hkdf` and `hmac` build on `sha2`) |
| `zeroize` and `subtle` are already in the tree (dependencies of the signing crate) | **[read: `Cargo.lock`]** | a `SecretKey` type that clears itself and compares in constant time costs no new crate |
| `lattice::voxel_random` seeds gameplay chance with `mix` of the world seed | **[read]** | fine while the seed is public (4.3) |
| A brick's block channel holds up to 256 distinct values paletted and goes dense (a `u32` each) above; a material number is a `u32` registry id | **[read, measured]** | `procedural-materials-spec.md` section 2 |

## 6. What I did not verify

- **Linux, and any GPU.** Everything measured is one Windows machine's CPU; the GPU figures are another program's published numbers for another hash.
- **The Minecraft attacks beyond one explanation.** The structure-seed and lattice descriptions are the search tool's summaries; Randar's first 40,000 characters were read, the rest only as a summary. I did not run SeedCrackerX or any plugin.
- **The hashed seed**, and any other detail of Minecraft's protocol that I put under **[recalled]**.
- **Whether quantised properties made with `mix` can be inverted** (above), and **whether a particular derivation I recommend has a flaw that a cryptographer would see**: I am not one, and the sensible use of this survey is to take standard constructions (HKDF, HMAC, BLAKE3's own modes) and **not to invent any**.
- **Other games' practice with hidden procedural content.** I found no game that makes unique materials for each planet and runs a market on them, and did not look for long; **[believed]** novel.

## 7. Sources

- Randar: [spawnmason/randar-explanation](https://github.com/spawnmason/randar-explanation/blob/master/README.md).
- Seed cracking, by search summaries: [the CB30/SeedCracker project](https://github.com/CB30/SeedCracker) (a dungeon floor cracker with fast lattice reversal), [SeedCrackerX's mirror](https://sourceforge.net/mirror/seedcrackerx/), [a seed-cracking write-up by hube12](https://gist.github.com/hube12/368e7331e497b17e092e8ca4ba206b3c).
- What servers do: [SeedVeil](https://hangar.papermc.io/mrpunky/SeedVeil), [SeedShield](https://modrinth.com/mod/seedshield), [AntiSeedCracker](https://modrinth.com/mod/antiseedcracker), [Paper's "Configurable feature seeds" patch (a mirror)](https://ayakael.net/mirrors/papermc/src/commit/51b7b609f108911138de830f2b201adb5a14628c/patches/server/0800-Configurable-feature-seeds.patch).
- Derivation: [RFC 5869](https://datatracker.ietf.org/doc/html/rfc5869), [blake3::derive_key](https://docs.rs/blake3/1.5.5/i686-unknown-linux-gnu/fn.derive_key.html), [the BLAKE3 issue on contexts](https://github.com/BLAKE3-team/BLAKE3/issues/13).
- Reversing splitmix64: [a thread on the prng list](https://groups.google.com/g/prng/c/WPjMq-IH5HI).
- Search cost: [hashcat, RTX 4090, SHA2-256, version 7.0.0](https://hashcat.net/forum/post-63595.html); [distributed.net on Wikipedia](https://en.wikipedia.org/wiki/Distributed.net) and [eWeek's report of RC5-64](https://www.eweek.com/security/distributed-net-team-deciphers-message-encrypted-in-rc5-64/).
- Block ids and palettes in other games: [Minecraft's Flattening](https://minecraft.wiki/w/Java_Edition_1.13/The_Flattening), [the chunk format (wiki.vg)](https://wiki.vg/Chunk_Format) and [a newer protocol mirror](https://voidminecraft.github.io/VoidMC/reference/protocol-specs/v26.1.2/chunks.html), [Forge's missing registry entries (forum)](https://forums.minecraftforge.net/topic/63882-fatally-missing-registry-entries/), [Luanti's world format](https://gogs.librecmc.org/OWEALs/minetest/raw/f99a1a7c7c1a3d96332169978b63ffb10c56dbb9/doc/world_format.md) and [its name-id mapping code](https://grok.dragonflybsd.org/xref/dports/games/minetest/minetest-5.4.1/src/nameidmapping.cpp?a=true).

## 8. The probe

`spikes/seed_probe/` (a standalone workspace, like `spikes/wasm/`, **not kept compiling**: it uses the engine's `terrain`, `voxel_grid` and `voxel_space` by path, and the programs in `spikes/` were written against the code of their day). `seed_probe inversion java derive palette` runs the quick sections; `seed_probe brute 24 28 32` is the slow one (about half a minute). Its numbers are the ones quoted above and in `procedural-materials-spec.md`.

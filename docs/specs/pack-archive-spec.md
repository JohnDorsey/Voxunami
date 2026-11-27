# One pack archive for servers and clients (proposal)

Asked for by the owner on 2026-11-20 after `folder-structures-review.md`: *"server packs and client packs are literally exactly the same zip file. The client would create the zip file based on packets they received from the server. That would include blocks and manifests and all such things. Packs would be verifiably identical on the server side and client side, with the exception of code, which will not be downloaded unless it is wasm."* **A proposal and my opinion; nothing in it is built. The owner answered on 2026-11-21: see "The owner's answers" at the end.** Labels: **[read]** I read the code or document; **[measured]** a run; **[believed]** reasoning I did not run.

## What exists now

- A pack is already a set of files in memory before it is anything else: `data_packs::RawPack` (`pack.json` text, `blocks/`, `patches/`, `bodies/`, `textures/`) **[read]**. A zip is a way to write that set down, not a new thing to model.
- A client is **not sent packs**. It is sent the *result* of loading them: `PackageData` (block definitions, the property schema, raw RGBA pictures by size), inline in `Welcome` and in `PackageReplaced`, and a recording carries it **[read: `package_data.rs`, `decisions/packs-and-content.md`]**. Cave's package is 24,289 bytes **[measured: the pin in `block_golden.rs`]**.
- `decisions/packs-and-content.md` already says why it is inline: *"Caching by hash needs a request message, a cache and a way for recordings to embed what was fetched, and saves nothing until packages hold assets; it is built in the phase that adds assets."* Pictures are those assets, so that phase has come **[read]**.
- The workspace has **no zip crate and no WebAssembly runtime** in `Cargo.lock`; `sha2`, `flate2` and `png` are there **[read]**. `architecture-review.md` already leans to "assets hash-addressed, size-capped" and to WASM modules that are **server only** and fuel-metered; the owner's line (a client may download wasm) widens that, and a client running wasm needs its own sandbox decision **[read]**.

## My opinion

**Yes, do it, and the idea is better than "a client may read the same folder"**, confidence medium-high. Three things come for free: the built-in content stops being a hand-kept list (the base game becomes one more pack archive, which ends the `TEXTURE_FILES` trap of 2026-11-18); a client's cache of a server's packs *is* a folder of packs it can host itself; and "verifiably identical" becomes one 32-byte comparison instead of a belief.

The parts I would not do the obvious way:

1. **"Literally the same zip" needs a canonical zip.** Two zip programs make different bytes from the same files (timestamps, order, compression, extra fields). So the identity is the hash of a **canonical archive**: names sorted, stored (no compression), a fixed date, no extra fields, no data descriptors, no zip64, UTF-8 names that are lower-case `a-z 0-9 _ . /` only (so Windows case folding cannot make two files one). The server **writes** this form from any folder or zip an author gives it, so authors keep using any tool; `--packs` takes a folder or a `.zip`. Transport compression is the connection's job, not the file's: PNGs are compressed already. **[believed]** that stored is the right size trade; Cave's textures are a few KB.
2. **The client parses only the canonical form, and refuses anything else.** This is the rule the package codec already follows ("decoding refuses anything the encoder would not write"). It makes zip-slip, bombs, symlinks, duplicate names and a second reading of one file **impossible to express** instead of checked for. The client never extracts to disk by name: it keeps the bytes (cache file named by hash) and reads entries from memory. Size caps on the archive, the entry count and each entry, before reading.
3. **"Identical, except code" cannot be byte-identical at the archive** if the server's archive holds code the client does not get. Two ways:
   - **X (my pick, confidence medium):** the pack's identity is a **root hash over the file index** (sorted path, size, hash of each file). The server sends the index; the client holds the archive of the files it is sent and checks every one against the index; a file it was not sent (code that is not wasm) is a line in the index it can see and cannot read. The client's archive is then a *subset* with the same index, so "the same pack" is the root hash, and each file is verifiably the server's.
   - **Y:** two archives per pack, `NAME.zip` (sent, byte-identical, hashed whole) and `NAME.code.zip` (server only). Simpler to verify, but an author has two files to keep in step, and "one pack, one file" is lost.
4. **Do not drop the resolved package in the first step.** Send the pack archives *and* the resolved `PackageData`; the client loads the archives with the same loader and **checks that it gets the same block table** (a cross-check that turns a mismatch of loader, schema or order into a loud error, as the project likes). Drawing keeps using the resolved package until the cross-check has run clean for a while. The block numbers are the world's (they come from the saved registry), so the welcome keeps the ordered list of block ids in any case **[read: `package-replacement-spec.md`, "blocks keep their numbers by name"]**.
5. **Client-only content stays outside the identity.** The client's own texture folder and its shader packs are *local packs* in the same format, listed apart from the server's, marked local, never hashed into the server's set, never sent. That is the ordered list of D9 of `block-textures-spec.md`: server packs are cache entries, local packs are the player's, and the order is the player's. Shader packs use the same container with `shaderpack.json` (the owner's rename of 2026-11-20 tells the kinds apart by the manifest).

## The wire

New messages (protocol 13, **[believed]** the right shape, not tried):

| Message | Direction | Content |
|---|---|---|
| `Welcome` gains the **pack list** | server to client | in load order: id, version, root hash, archive size, and the file index |
| `RequestPacks` | client to server | the root hashes it does not have in its cache |
| `PackChunk` | server to client | root hash, offset, bytes, in order, with a last-chunk flag; the client checks size and hash at the end and refuses a pack that fails |
| `PackageReplaced` | server to client | the new pack list (the delta of hashes) and the chunks it needs, then the resolved package as now |

A client with a pack in its cache (a folder named by hash in the game data folder) asks for nothing. A **recording** embeds the archives it was started with and every one it was sent later (once each), so replay still depends on nothing installed; this is the "way for recordings to embed what was fetched" the old decision waited for. Singleplayer's integrated server hands the same archives over in memory.

## How the hashes are used (the concrete picture)

A pack has a **file index**: one line per file, sorted by path, each with the size, the hash of the file's bytes and whether it is **sent** to clients. The **root hash** `R` is the hash of that index's canonical bytes. Example, a pack `my_pack`:

```
path                    size   hash   sent
blocks/lava.json         230   h2     yes
pack.json                412   h1     yes
scripts/rules.py         900   h4     no   (server code that is not wasm: never sent)
textures/lava.png       1190   h3     yes
worldgen/hills.wasm    41000   h5     yes  (code a client may run, once wasm exists)
```

`R = hash(this list)`. **The root hash does not manage code. It makes the code part of the pack's identity while the client cannot read it**: change one byte of `rules.py` and `h4` changes, so the index changes, so `R` changes; every client then knows the pack is not the one it had, though it never held the script.

What happens, in order:

1. **Welcome** carries, for each pack in load order, `(id, R, the index)`. An index is about 50 bytes a file.
2. The client looks for `R` in its **cache** (a folder in the game data folder, one canonical zip per pack named by `R`). If it is there, it **asks for nothing**. It checks the file once on loading (hash each file against the index, hash the index against `R`).
3. If not, the client sends `RequestPack(R)`. The server answers with `PackChunk`s: the canonical zip of **the files marked sent**, in pieces. When the last piece is in, the client checks every entry against its line of the index, and the index against `R`, and refuses the pack if anything differs. Only then does it write the zip into the cache (the bytes it received, which are canonical already).
4. **When nothing is withheld, the client's zip is byte for byte the server's zip.** When something is, it is the server's zip minus those files, and the index says exactly what is missing and what its fingerprint is.
5. **A reload** (`PackageReplaced`) carries the new index. The client compares it line by line with the old: a file whose hash is unchanged is kept, and only the changed files are asked for (`RequestFiles(R_new, paths)`). A modder who repaints one picture of a pack of 40 sends one picture. Hashing the whole zip alone could not tell which part changed.
6. **A recording** keeps the index and the sent files of every pack it was sent, once each, so replay depends on nothing installed.
7. **Two servers** (or a server and a recording) are running the same pack if the `R` are equal. `check-pack` prints `R`.

Wasm for world generation (wanted by the owner, later) fits without a new rule: `worldgen/hills.wasm` is a file that is sent, its `h5` is in the index, and **a generator's identity is its id and the hash of its module**, so a changed module cannot pass for the old generator (the hard rule that a released generator version is never edited). How a world records which module made it is a question for that step: saved files hold ids only, so the version in `pack.json` or the hash would need an owner's decision.

## The `replicate` entry of `pack.json` (owner, 2026-11-21)

Whether the pack is **replicated as a zip on the client's machine** is the author's choice, a key of `pack.json`: `"replicate": true` or `false`. My proposed meaning, to confirm:

- `true`: the pack goes to the client as the archive above and is kept in its cache; the client can verify it, and can host it again.
- `false`: the client is sent **only the resolved package** (as today: the block table and the pictures as pixels), and no file copy is made.
- **It is not a secrecy switch.** Whatever a client draws or reads, it has: a block's boxes and a picture's pixels are in the resolved package either way. The key decides whether there is a verified **file** copy, not what can be seen. Secret rules belong in code, and code is withheld.
- A pack whose wasm the client must run (the world generator) **must** have `replicate: true`; the loader refuses the other combination.
- Default: my pick is `true` (moddability first: a player can take what they were sent and host it), confidence medium.

## The owner's answers (2026-11-21 and 2026-11-22)

1. **The index and the root hash are built from the start** (2026-11-22). The first archive crate writes and checks them.
2. **Canonical form, and an author's zip rewritten by the server: yes** (2026-11-21).
3. **Wasm:** a client may run wasm, **later**; the owner definitely wants it for world generation (`generator-identity-spec.md`).
4. **Both ways, by the pack's author: accepted as proposed** (2026-11-22): the key is `"replicate"`, `true` is the default, `false` means the client is sent only the resolved package, and a pack with wasm the client must run needs `true`. The resolved package stays on the wire in any case.
5. **A client keeps a replicated zip after it disconnects** (2026-11-22).
6. **A cached pack's file name has the pack's own id and the hash being looked for**: `my_pack-HASH.zip`, where `my_pack` is the `id` of its `pack.json` (the author's intended name; it is already a safe file name: lower case, digits and `_`). My pick for `HASH`: the first 32 hex characters of the root hash. It is only the key to look the file up by; the whole root hash is checked when the file is loaded, so a clash of the short form can never put a wrong pack in use. (A full 64 characters would make long paths on Windows, a trap this project has met.)
7. **The client's folder for a server should strongly resemble a world folder, and ideally be one** (2026-11-22): see the next section.

## The client's folder for a server (owner's idea, 2026-11-22; a discussion)

The idea: the folder a client makes for a remote server holds the replicated packs *and anything else it caches of that server*, in the shape of a world folder, so that long after the player stopped playing there, it is **a world that can be played in singleplayer, though it may be incomplete**.

**A fact first, which changes the question: there is no world folder today.** A world is one file named by `--world FILE`, with its ships in `FILE.ships/` beside it; the game data folder holds the identity and `shaderpacks` and nothing else **[read: `options.rs`, `ship_files.rs`, `identity`]**. So the first thing to design is the world folder; the server folder is then the same shape with a marker. My proposal (confidence medium-high that one shape for both is right; medium on the details):

```
DATA/worlds/NAME/              a world made here
DATA/worlds/HOST-NAME/         a world made from a remote server (the same shape)
    world.voxunami             the world file, as the server writes it today
    world.voxunami.ships/      its ships
    packs/my_pack-HASH.zip     the replicated packs the world uses
    remote.json                only in a remote one: the server's address and name, when it was last joined
```

A pack inside the world's own folder makes any world **portable**: copy the folder and the world and the content it needs go together. Packs are copied into each world that uses them rather than shared; they are small (Cave's whole package is 24 KB **[measured]**) and a shared cache is one more place to get out of step **[believed]**.

**What a client could write, and what it could not.** The client's view of the world comes only from the server's message stream (a hard rule), so the client can save only what the stream told it. That is exactly why the world is "incomplete", and it also says what it can hold **[believed from the design; not tried]**:

| Kept | Why it is there | Missing, so it plays as the pristine generator would |
|---|---|---|
| the packs, the block order, the generator recipes, which generator is used where | all in the welcome | |
| the **override bricks the client was sent** (what players built or dug in what it saw) | the stream sends exactly these | those it was never sent: edits out of its view |
| the ships it saw | the stream carries them | ships it never saw; the other players |
| unmodified terrain | **regenerated from the generators**, so it is complete by construction wherever the generator is in the packs | |

Some things have no source on the client (the clock, entities the client did not see, anything the server's own code kept), and the integrated server that loads the folder needs *something* there: defaults. **That piece, a world writer driven by the client's view and not by a server's state, is new, and I have not looked at how much of the world file's format a client can fill.** I would look before promising it (a spike: write a world file from a client that has joined a test server, load it in singleplayer, compare what is seen).

**Two things the owner should decide:** (a) a server operator may not want its builds to leave the server as a world file: a **server setting** (`allow_world_replication`) is cheap and the default is the owner's choice (I lean to allowing it: the player has seen all of it already); (b) while the world is a copy of what one player saw, `remote.json` says so, and a loaded copy is a **snapshot**: it will not know anything that happened after the last join.

## Decided 2026-11-22 (second answer) and what is still open

- **The folder shape is approved** (`worlds/NAME/` and `worlds/HOST-NAME/`, one shape, `packs/ID-HASH.zip` inside, `remote.json` only in a remote one).
- **Copying a server's world is on by default and the operator opts out** (owner, 2026-11-22): the server setting `allow_world_replication` (name chosen by the owner, 2026-11-22; default true) turns it off.
- **The cache goes in the world folder from the start**; the world file inside it comes later (my pick, taken as agreed since the folder was approved).
- Still open: the world writer's spike (what of a world file a client can fill), and the generator-identity questions in `generator-identity-spec.md`.

## Steps (each ends with tests, clippy, fmt, doc; estimates are **[believed]**)

1. **BUILT (2026-11-22): `pack_archive`** (a crate under `engine/`; see "What step 1 built" below): a `RawPack` to and from the canonical archive, the root hash, the file index. Tests first: the same files in any order give the same bytes (a property test that shuffles), refusal of every non-canonical form (a corpus of bad zips: absolute paths, `..`, duplicates, case twins, compressed entries, extra fields, an entry larger than it says, a short archive), a round trip with the existing example packs. About two days. This is the safe piece: nothing else changes.
2. **BUILT (2026-11-22; see "What step 2 built"): packs from a folder or a zip**: `--packs` and `check-pack` read both; `pack-schemas` unchanged. The data pack layout (`textures/NAMESPACE/NAME.png`) is the layout of an archive, so the client's texture folder reads it too (and keeps `NAMESPACE/textures/NAME.png` for `plugins/` until step 4). A day.
3. **BUILT (2026-11-22; see "What step 3 built"): built-in content as an archive**: `base_blocks` and Cave's own blocks are read as packs from archives built by `build.rs` (every file of the folder is in it, so nothing is "forgotten"); the Rust list that fixes their numbers stays and a test checks the archive against it. This removes `TEXTURE_FILES`. The pins (`PINNED_PACKAGE`, `PINNED`) must not move, which is the check that the step changes nothing. Two days, the first step with a risk.
4. **BUILT (2026-11-22; see "What step 4 built"): the wire and the cache**: protocol 13, the messages above, the cache folder, the recorder. The first piece that touches replay, so tests first for replay equals live. Three to four days.
5. **BUILT (2026-11-23; see "What step 5 built"): the client's own load and the cross-check** (item 4 above); then, if the owner wants, the resolved package becomes derived on the client and leaves the wire. A design decision of its own.
6. **PART BUILT (2026-11-23; see "What step 6 built"): withheld files** (the root hash over the index was built in step 1); **wasm entries NOT built**, once there is a runtime and a sandbox decision for the client (`architecture-review.md` says server only today).
7. **BUILT (2026-11-23; see "What step 7 built"): watch `--packs`** (step 6 of `folder-structures-review.md`): a change of a folder is a new archive, a new hash, a `PackageReplaced`.

## Risks I see

- **The loader must be bit-identical on the client**, or the cross-check fails for the wrong reason. It is deterministic in what it is given (`decisions/packs-and-content.md`), so I expect it to hold **[believed]**; step 5 is what proves it.
- **A client that has the pack can read everything in it**: blocks, patches, bodies. Packs are data and meant to be public, but a server author must know it: anything secret is code, and code is withheld.
- **PNG decoding of bytes from a server** is attack surface on the client that the raw-RGBA package avoids today. The `png` crate decodes with limits and the size rules are already strict (square, 8 to 256, a power of two); I would cap the decoded size before decoding **[believed]**.
- **Archive size**: a pack with many large pictures makes a bigger welcome than the package. Chunking and the cache answer it; the number to watch is first-join time with an empty cache.

## What step 1 built (2026-11-22)

`engine/pack_archive`, depending only on `sha2`, `crc32fast` and `thiserror` (all already in the lock file; no engine crate). **Verified:** 44 tests pass (writer pinned byte by byte; a shuffle property; a refusal corpus of 28 cases each asserting its own error; a changed-byte property that what is accepted writes back identically; random bytes never panic; the solar-system data pack and the three example shader packs round-trip), and four deliberate breaks of the code (the date, the canonical comparison, the checksum, the sort check) each failed the tests that should catch them.

- `Archive::from_files` (any order; refuses paths that are not allowed, twice, or a file and a folder) and `to_bytes`; `Archive::from_bytes(bytes, &Limits)` reads the canonical form only. It reads every field to name the reason, then **rewrites the archive and compares bytes**, so no second spelling of one archive can pass. Limits (default 10,000 files, 256 MiB a file, 1 GiB in all) are checked before a file is copied.
- Paths: 1 to 255 bytes of `a-z 0-9 _ - . /` (a dash was allowed on 2026-11-22 at the owner's question: nothing needed it refused), no empty, `.` or `..` part. An upper-case or other character in an author's file name is still refused, which step 2 must say plainly when the server rewrites a zip.
- `FileIndex` (path, size, SHA-256, sent), `root_hash` = SHA-256 of the index bytes (`VXPI`, version 1, count, entries; **BLAKE3 and version 2 since 2026-11-26**, below), `to_bytes`/`from_bytes` (strict), `verify` (a whole archive) and `verify_received` (what a client was sent: withheld files absent, none unlisted).
- `cache_file_name(pack_id, &root)` gives `my_pack-<first 32 hex>.zip`.
- **Not decided here, believed fine:** SHA-256 over BLAKE3 (already in the lock, no new dependency); the entry time is 00:00 on 1980-01-01. **The hash was decided on 2026-11-26 (owner): BLAKE3**, for file hashes and root hashes alike, through the `blake3` crate with its `pure` feature (Rust intrinsics, no C or assembly). The index's version went to 2 and the protocol's to 15, so that an old program is refused by name and not by a pack that fails its check. A client's store keeps its old files under names no pack has now; nothing deletes them.

## What step 2 built (2026-11-22), and what it did not

**Built (verified by tests and by running `check-pack`):** `data_packs` reads a pack from a folder or from a canonical `.zip` through **one** function (`files.rs`: a map of path to bytes in, a `RawPack` and the problems out), so the same files give the same pack and the same error text either way (12 new tests; the 7 old folder tests pass unchanged, which locked the refactor). `read_directory` reads folders and `.zip` files together, sorted by pack name, so `--packs`, `check-pack` and the reload read zips too; a folder and a zip of one name are an error. A zip's pack name is its file name without `.zip` and without a cached name's `-<32 hex>`. `read_archive(pack, &Archive)` reads one in memory (what the client will use). `pack_identities(root)` and the `check-pack` output give each pack's **root hash**, equal for a folder and for its zip.

**Not built:** the server rewriting an *author's* zip (deflated, with folders, a top-level wrapper folder, an `__MACOSX` folder) into the canonical form. Today a zip that is not canonical is **refused with its file name and the reason**. Rewriting needs a lenient zip reader, that is the `zip` crate (a new external dependency, with deflate), behind a feature of `pack_archive` that only the server's tools turn on; I have not added it, pending the owner's yes.

**Found, and the owner should decide:** a folder pack may have files beside `pack.json` (a `README.md`, a `LICENSE`), which the loader ignores and which today are fine. **An archive refuses upper-case names, so such a folder cannot be hashed**: `check-pack` prints "no root hash yet, it cannot be made into an archive" and names the file. Options: (a) the archive of a folder holds only the files the loader reads (`pack.json` and what is in `blocks`, `patches`, `bodies`, `textures`, later `worldgen`), so a README never matters; (b) the archive holds everything and an author must rename `README.md`; (c) allow upper case and refuse only twins that differ in case alone. My pick is (a), confidence medium-high: the identity then covers exactly what affects the game, and a licence file can sit in the folder without changing the pack's hash. The cost: a licence is not carried to clients; an author who wants one carried puts it in a folder the loader will one day read (`docs/`?), which is a small decision for later.

**Decided (owner, 2026-11-22): option (b).** The archive holds **everything**: a README or a licence is in the replicated pack too, and **every file name must be lower case** (`a-z 0-9 _ - . /`). Built the same day: a folder with a file named otherwise is an **error that names the file** (`mine/README.md: this file name is not allowed in a pack, because every file of a pack is replicated...`), in `read_directory`, so `--packs` and `check-pack` refuse it as a zip is refused; `pack_archive::check_path` is the one rule. Names that start with a dot (`.git`, `.DS_Store`) are still left alone and are in no archive. The "no root hash yet" line of `check-pack` can no longer happen for a folder that read cleanly.

**Step 2, the rest: an author's own zip (built 2026-11-22, the owner said yes to the `zip` crate).** `pack_archive::import_zip` (feature `import`, off by default; `data_packs` turns it on) reads any zip an author's tool writes and gives the canonical `Archive` of its files: folder entries dropped, a wrapper folder (one top-level folder holding `pack.json`) taken off, `\` read as `/`, a tool's leftovers (names starting with a dot, `__MACOSX`) dropped. **Refused, naming the file:** a name that is not lower case or otherwise not allowed (never changed for the author, since the pack's JSON names its files), two entries of one path, a symbolic link, an encrypted or damaged entry, anything past the limits (declared sizes are checked before an entry is inflated, so a zip bomb is refused at its header). `read_directory` tries the strict reader first and imports otherwise, so `--packs` and `check-pack` take author zips; `check-pack` prints the root hash of the canonical rewrite, equal to the folder's. **Verified:** 17 import tests, 2 new data_packs tests, and breaking the duplicate check, the size limit, the leftover filter and the wrapper rule each failed a test (the wrapper mutation first survived, which showed a missing case, now tested). **Found:** the `zip` crate silently keeps one of two entries that have exactly the same name; `import_zip` compares the end record's count with the entries it got to catch it. **Cost:** a new dependency (`zip` 8.6 with `flate2` on the pure-Rust `zlib-rs`, plus `indexmap`, `memchr`, `typed-path`; no C code); since `data_packs` turns the feature on, clients link it too (a client only uses the strict reader). **Not built:** a command that writes the canonical zip to a file; the server will write it into its cache in step 4.

## What step 3 built (2026-11-22)

**Built:** `base_blocks` and Cave each carry their own pack as one canonical archive, made by a `build.rs` of the crate (`pack_archive` as a build dependency) and read by `data_packs::read_builtin(id, bytes)`, which **stops the program** (a panic naming the pack and the file) if the bytes are not the canonical archive, a folder nothing reads is in it, there is no `pack.json`, or the manifest names another pack. `include_str!` of each block and the list `TEXTURE_FILES` are gone. New in `pack_archive`: `files_of_folder(folder, prefix, keep)`, the files of a folder tree for a build script (names that start with a dot are left alone, as when a server reads a pack folder). Each crate has `pack_archive_bytes()`, which a server will hand to a client (steps 4 and 5).

- **`base_blocks`:** the archive is the whole `data` folder (a new `data/pack.json`, `{"id": "base_blocks"}`, and `blocks/*.json`) and **every `.png` directly in `textures`**. The folder `textures` is the painter's working folder (VirtualAtlas, its `atlas.json`, the two scripts, a `README.md` with capital letters), and the owner's rule that every file of a pack has a lower-case name would refuse those, so the archive takes pictures only: **this is my choice, to confirm**. Files were not moved, so VirtualAtlas and `--texture-pack plugins` work as they did. The list that fixes the numbers of the blocks stays in Rust (`BLOCK_ORDER`), and a test compares it with the files in the folder and with the files in the archive, each on its own.
- **Pictures no block uses are in the pack now** (twelve of them: gravel, sand, ice, snow, the waters, the plants): a world gets only the pictures its blocks use, so nothing changed in the package (see below), and a picture the owner paints and a block starts to name needs no list edit. The old test that said "the embedded pictures are exactly the used ones" became "every picture a block uses is in the pack, and every companion goes with a picture the pack has".
- **Cave:** the archive is its whole `data` folder (a new `data/pack.json` with the id `voxunami_cave`, `blocks/dev_ball_joint.json`, `blocks/dev_docking_port.json`, `bodies/planet.json`); `block_from_data` and the planet read from it. A test says the files the pack has are exactly the ones the code names (a block file nobody registers would otherwise be silent).
- **A new picture dropped into the folder is in the program with no edit** (measured: a copy of `stone.png` made the pack 33 pictures, taking it away 32; cargo watches the folders, so a new file builds the archive again). The old trap of 2026-11-18, five of the owner's companions not shipped, cannot happen.

**Verified (the check that the step changes nothing):** before the change, the Debug text of `BaseBlocks::new()` (114,127 bytes, FNV-1a `0xa6caa7136d184d7d`) and of its block table (`0xf516a70fbcbc231d`) and the nine pictures in use were recorded with a throwaway test; after it they are **identical**. `PINNED_PACKAGE` of all three games (Cave 24,289 bytes, 2048 3,370, Eels 1,413) and the `PINNED` of the three golden sessions pass **unmoved**, as do Cave's ten `packs` tests (they name `plugins/base_blocks/textures/stone.png` in a message, which the pack keeps). 5 new `read_builtin` tests, 6 `files_of_folder` tests, 2 for `base_blocks`, 1 for Cave.

**Not done, believed fine:** the archive of a built-in pack is not pinned by a root hash (an edit of a picture would move it, and the pins above already say the content did not change); the three games' `check-pack` does not list the built-in packs yet (step 5 compares them with a server's).

## What step 4 built (2026-11-22)

**Built, and tested at each end and across both:** protocol 13 with three messages, a server that tells its clients of its packs and sends the archive to a client that asks, a client that takes them from a store or from the server and checks them, and a recording that holds every pack its client had. **Verified:** protocol (34 tests, the layouts pinned byte by byte), server (12 new, all 198 + 78 + 6 pass), client (8 new), `end_to_end_tests` (4 new: a replay holds the packs and sees the views the live client did, with the pack sent, with it taken from the client's own store, with a list replaced mid-session, with and without a store), `data_packs` (`replicate`, `read_pack_directory`), `game_shell` (the folder store, `served_packs`), and in real Cave a server made from a folder of packs sends one and not the one that says `replicate: false`, the pack is kept for the next join, and both recordings replay with it. How it works is in `docs/reference/protocol-and-replay.md`, "Packs a server replicates".

**Where I chose differently from the plan above (the owner may overrule any):**
- **`Packs` is a message of its own**, not a field of `Welcome` and `PackageReplaced`. It is the whole list and replaces the client's, sent after the welcome and after a replaced package. Reason: no change to the two largest messages and the ~20 places that build them, and a server without packs sends nothing new, so the streams of the existing tests moved only by the version byte (**verified** for the recording and the three golden sessions by putting the version back to 12: the old checksums came back).
- **A client asks for a whole pack**, not for files (`RequestFiles`): a repainted picture sends the pack again. The index already lists each file's hash, so the per-file request is a later change that needs no new identity.
- **Pieces are paced**: 64 KiB a piece, at most 256 KiB a tick to a client (`ServerConfig::pack_bytes_per_tick`), at least one piece, before the end of the tick.
- **`Connection::note_frame`** (a new method with a default that does nothing) is how a client tells a recording of a pack it took from its own store; only `RecordingConnection` does anything with it.
- **A pack that arrives wrong stops the client** with its name and the reason; a piece of an unlisted pack is ignored; a file in the store that does not verify is ignored and the pack asked for.
- **The cache is `DATA/worlds/ADDRESS/packs/ID-HASH.zip`** with `remote.json` (`address` and `game`) beside it, made when a player joins a server over the network. I left out "when it was last joined": it is a wall-clock time. What the client kept is a folder `read_pack_directory` reads as it reads a server's, which a test checks. A server in the player's own process sends in memory and nothing is kept.
- **`replicate`** is read (default `true`, in the schema and the reference); the rule that a pack with wasm needs `true` is for when there is wasm.
- **The list is in directory order** (sorted by pack name), not load order: the loader orders by dependencies, and the client will with the same loader in step 5.

**Not built:** the built-in packs (`base_blocks`, Cave's) are not in the list: a client has them in its program, and comparing their root hash with the server's is a cross-check of step 5. Nothing uses the packs a client holds. Withheld files: `ServedPack::new` takes the rule, and the shell sends every file. The server setting `allow_world_replication` and the world file in the remote folder are not built (they are about worlds, not packs).

**Found on the way, and fixed in its own commit before step 3's:** `binary_codec::read_varint` accepted `0x81 0x00` for 1, so a number had two encodings (the `wide_float` property test found it by chance).

## What step 5 built (2026-11-23): the client loads the packs and checks them, and the resolved package stays on the wire

**Built:** once a client holds every pack the server listed, it loads them with its own copy of the loader over its own copy of the game and compares what that makes with the package the server sent (`game_shell::pack_check`, `Session::pack_check`). The comparison is **by block name, not number** (a server that reloaded keeps the number of a block that went, as missing content), and covers the definition of each block that has one, the pictures, the properties, the channels and the unlit brightness; the result is `Waiting`, `NotApplicable`, `Agrees` or `Disagrees` with a line for each difference. A disagreement is **told once, on the error stream, and not hidden**, and the client goes on drawing with what the server sent (item 4 above: the resolved package stays until the check has run clean for a while). It is made again when the list or the package changes, and a replay makes it from the packs its recording holds and gets what the live client got.

**Protocol 14, one byte:** `Packs` says whether its list is **every pack the server runs** besides the game's own. Without it a client cannot tell a server whose package is its packs' from one that also runs a pack it did not replicate (`"replicate": false`, or given to it in memory, as every test of the old `RawPack` kind does), and would call that a mismatch. A server that runs only the game's own says nothing (an empty list that is whole), so the client then compares **its own copy of the game's content against the server's package**: that checks the built-in content of the two programs against each other, which the spec had as "built-in packs in the list" and which this does without a list. `ContentPackage` keeps the ids of the packs it applied and says the list is whole when each is among the replicated.

**Verified:** `pack_check` (5 tests: the same package, order and missing content not compared, a changed block, a block on one side only, the rest), protocol (the byte pinned both ways, refusal of a 2), server (2 new), client (1 new), the package's whole-ness (1), and in real Cave: a pack folder makes `Agrees` live and in a replay; no packs at all makes `Agrees` (the game's own against itself); a pack that is not replicated makes `NotApplicable`; a server whose package was made from one marble and whose packs say another makes `Disagrees` naming `my_pack:marble`, reported once, with the client drawing the server's marble; a reload that adds a pack and a patch makes `Agrees` again. **Two deliberate breaks** were caught (the definitions no longer compared: the unit test and the Cave test failed; the packs no longer applied: the Cave `Agrees` test failed).

**Cost [measured, dev profile]:** making a Cave content with no pack takes 4.7 ms, and with a pack of one block 21 ms, so the check is about **21 ms on the thread that ticks the view, once when the packs are all in and once for each reload**. Packs with many pictures will take more (the pictures are decoded as the server decodes them); it is not on a worker thread, and I would move it there only if a real pack makes the hitch seen.

**Not built, and the owner's decision:** **the resolved package leaving the wire** (a client derives it from the packs). The check is the evidence for that, and it has run clean only in tests; it needs real servers and packs, and the check's `Agrees` count over a long time, before the package can go. Until then every client is sent both, 24 KB for Cave. Also not built: the check of a server's *program* version against the client's (a `Disagrees` on the game's own content is the only sign today), the withheld files and wasm (step 6), watching `--packs` (step 7).

## What step 6 built (2026-11-23): files a server keeps to itself

**The rule (my choice; the owner may overrule it):** a pack's **`server/` folder** is withheld. `data_packs::is_sent_to_clients(path)` is the one place that says so, used for the identity `check-pack` prints and for the pack a server serves, so the two cannot differ. Why a folder and not a manifest list: a file cannot be withheld by forgetting to list it, a reader of a pack sees at once what is kept, and it needs nothing in `pack.json` or on the wire. **The wire did not change** (the index already had the `sent` flag and the client already refused a withheld file that came anyway); protocol stays 14 and no pin moved. Confidence that a folder is the right form: medium; that it is a form that can change without harm: high, because nothing reads the folder yet.

**What it does:** the files of `server/` are in the index with their size and hash, so **the root hash covers them** (one byte of server code is another pack), are not in the archive a client is sent, and so a client holds a pack whose archive is a subset with an index that says what is missing. The loader reads nothing in `server/` and no longer calls it an unread folder (it was an error for any folder it does not read, which stays so for the others), so a client without the files loads the same pack, and the cross-check of step 5 agrees. `check-pack` says how many files a pack keeps back.

**Verified:** `data_packs` (3 new: the rule on names that only look like the folder's, the loader reading the same pack with and without the folder, the identity being the root of an index that lists the files as not sent, and changing with one byte), `game_shell` (2 new: the served pack and its listing, the count `check-pack` prints), and in real Cave with a real server and a client: the bytes sent do not hold the file's text, the client holds the pack as sent and sees one line of the index as not sent, `Agrees`, the cache has it under the root, a replay holds the same and agrees. The first Cave run of the loader test found the loader refusing the folder, which is why the loader changed.

**A trap to know [believed from the code, not run]:** the root of a pack a client holds cannot be re-made by the client from what it holds, because the withheld files are not there. `check-pack` on a client's cache folder prints a root that is **not** the server's for a pack that has withheld files (it is the root of an index where everything present is sent). The cache is named and checked by the root the server told, so nothing in the game is affected; a person who copies a client's cache to host it would be hosting another pack (a different identity), which is right, as it lacks the code.

**Not built:** wasm entries (no runtime, no sandbox decision for a client, and the generator-identity questions are open), and what happens when a pack with code that a client must run says `"replicate": false` (the loader's refusal of that combination is for when there is wasm).

## What step 7 built (2026-11-23): `--watch` for a server's packs folder

**Built:** `HostSettings::watch_packs` (set by `--watch` on `serve`, and by the existing `--watch` of a game that starts its own server with `--packs`). The server thread holds a `packs::PackWatch` (the `reload.rs` `Watcher` over a `DiskSource` of the folder) and, **between two ticks**, looks at it; a settled change of content calls `reload_from_directory`, the very code `/reload` runs, so there is one way to replace packs and the watcher only decides when. The packs are read and checked beside the state in use and swapped only if all are good (`ContentPackage`/`reload_packs`); a failure prints the reason and is not tried again until a file changes (the watcher takes the changed files as seen).

**What it covers that the spec asked for:** a change to *any* file of a pack, one nothing reads (a note, a `server/` file) too, is another root hash, so clients are told the new list (`Packs`, first in the next tick) and ask for what they lack; a change that alters the package is sent as `PackageReplaced` as before. A zip in the folder is one file to the watcher, so replacing a zip works as editing a folder does.

**Verified:** `--watch` parsed for `serve` (default off); in real Cave on a real server thread with `watch_packs` and nothing typed: a repainted block reaches a connected player, with a new pack root and the pack held completely; then a note and a `server/` file, which change no package, still give another root. With watching turned off in the test it fails at the repainted block (it waited 30 s), so the test bites. The `Watcher` itself (settle, digest, no-op saves) is the existing `reload.rs` with its own tests.

**Believed, not run:** a half-written pack on a running server is what the settle is for; I did not write a test that interrupts a write. The watcher polls from the server's loop with `Instant`, as the loop already paces itself (it is the server's housekeeping, never simulation). The folder being deleted while running gives `Unreadable`, printed once. A client that is mid-download of a pack when the list changes drops it and asks for the new one (step 4's behaviour, tested for a list replaced mid-session).

**Not built:** watching a client's own folders for hosting; a setting for the settle and interval (the defaults, 150 and 250 ms, are those of the shader packs).

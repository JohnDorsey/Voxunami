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
2. **Packs from a folder or a zip**: `--packs` and `check-pack` read both; `pack-schemas` unchanged. The data pack layout (`textures/NAMESPACE/NAME.png`) is the layout of an archive, so the client's texture folder reads it too (and keeps `NAMESPACE/textures/NAME.png` for `plugins/` until step 4). A day.
3. **Built-in content as an archive**: `base_blocks` and Cave's own blocks are read as packs from archives built by `build.rs` (every file of the folder is in it, so nothing is "forgotten"); the Rust list that fixes their numbers stays and a test checks the archive against it. This removes `TEXTURE_FILES`. The pins (`PINNED_PACKAGE`, `PINNED`) must not move, which is the check that the step changes nothing. Two days, the first step with a risk.
4. **The wire and the cache**: protocol 13, the messages above, the cache folder, the recorder. The first piece that touches replay, so tests first for replay equals live. Three to four days.
5. **The client's own load and the cross-check** (item 4 above); then, if the owner wants, the resolved package becomes derived on the client and leaves the wire. A design decision of its own.
6. **Withheld files and the root hash over the index (X)**; wasm entries, once there is a runtime and a sandbox decision for the client (`architecture-review.md` says server only today).
7. **Watch `--packs`** (step 6 of `folder-structures-review.md`): a change of a folder is a new archive, a new hash, a `PackageReplaced`. It falls out of 1 and 4.

## Risks I see

- **The loader must be bit-identical on the client**, or the cross-check fails for the wrong reason. It is deterministic in what it is given (`decisions/packs-and-content.md`), so I expect it to hold **[believed]**; step 5 is what proves it.
- **A client that has the pack can read everything in it**: blocks, patches, bodies. Packs are data and meant to be public, but a server author must know it: anything secret is code, and code is withheld.
- **PNG decoding of bytes from a server** is attack surface on the client that the raw-RGBA package avoids today. The `png` crate decodes with limits and the size rules are already strict (square, 8 to 256, a power of two); I would cap the decoded size before decoding **[believed]**.
- **Archive size**: a pack with many large pictures makes a bigger welcome than the package. Chunking and the cache answer it; the number to watch is first-join time with an empty cache.

## What step 1 built (2026-11-22)

`engine/pack_archive`, depending only on `sha2`, `crc32fast` and `thiserror` (all already in the lock file; no engine crate). **Verified:** 44 tests pass (writer pinned byte by byte; a shuffle property; a refusal corpus of 28 cases each asserting its own error; a changed-byte property that what is accepted writes back identically; random bytes never panic; the solar-system data pack and the three example shader packs round-trip), and four deliberate breaks of the code (the date, the canonical comparison, the checksum, the sort check) each failed the tests that should catch them.

- `Archive::from_files` (any order; refuses paths that are not allowed, twice, or a file and a folder) and `to_bytes`; `Archive::from_bytes(bytes, &Limits)` reads the canonical form only. It reads every field to name the reason, then **rewrites the archive and compares bytes**, so no second spelling of one archive can pass. Limits (default 10,000 files, 256 MiB a file, 1 GiB in all) are checked before a file is copied.
- Paths: 1 to 255 bytes of `a-z 0-9 _ - . /` (a dash was allowed on 2026-11-22 at the owner's question: nothing needed it refused), no empty, `.` or `..` part. An upper-case or other character in an author's file name is still refused, which step 2 must say plainly when the server rewrites a zip.
- `FileIndex` (path, size, SHA-256, sent), `root_hash` = SHA-256 of the index bytes (`VXPI`, version 1, count, entries), `to_bytes`/`from_bytes` (strict), `verify` (a whole archive) and `verify_received` (what a client was sent: withheld files absent, none unlisted).
- `cache_file_name(pack_id, &root)` gives `my_pack-<first 32 hex>.zip`.
- **Not decided here, believed fine:** SHA-256 over BLAKE3 (already in the lock, no new dependency); the entry time is 00:00 on 1980-01-01.

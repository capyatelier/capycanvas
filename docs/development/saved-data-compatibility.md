# Saved data compatibility plan

[Developer guide](README.md) · [Settings and persistence](../ui/settings.md) · [Package contract](../reference/capy-package.md)

**Status: proposed.** Milestone 1 must land before the next release; until then
an upgrade from 1.0.15 discards every painter's palettes and brush settings.

## Goal

From 1.0.15 on, installing a newer build never discards what a painter made:
workspaces, saved toolbars, palettes, brush settings, preferences, export
presets, drawings or recovery copies. A change in meaning is converted
automatically when the data is read. An older build opening newer data keeps
what it can read and never resets the store.

We get there with as few mechanisms as possible. Each one below answers a
failure we already have, and follows a practice that other long-lived
applications adopted after the same failure.

## What fails today

| Data | Reader | What happens with another build's data |
| --- | --- | --- |
| Editing state: palettes, swatches, colour history, brush overrides, remembered tools, selection, fill and gradient options | `WorkspaceManager::load_editing` decodes one strict `EditingState` | Any mismatch discards all of it. A 1.0.15 copy fails on main in four ways: `region_sources` grew from two to three entries, three fields were added without defaults, `version` must now be exactly 3, and the region value `smoothing` became `smooth_edges`. In 1.0.15 the same failure reset the whole store. |
| Workspaces | `Entity` rows with `deny_unknown_fields` and exact layout-history versions | An unreadable workspace is skipped. When no workspace can be read, startup resets the whole store, including toolbars and editing state (`sqlite_startup_replaces_workspaces_of_the_same_version_it_cannot_read`). One layout change makes every stored workspace unreadable at once. Maintenance decodes every pending delivery, so one unreadable delivery fails startup the same way. |
| The reset copy | `preserve` in `crates/layer-workspace/src/sqlite.rs` | One `workspaces.unreadable.sqlite3`, replaced by the next reset. |
| Older build, newer store | `SCHEMA_VERSION` | It changes only for table changes, so an older build treats a newer store as its own and resets it when the item formats differ. |
| Export presets | `ExportPresets::restore` | A file this build can't decode counts as empty, and the next save replaces it. |
| `.capy` files | Closed field sets per record (`crates/layer-core/src/package/`) | 1.0.14 wrote occurrence content as `objects` inside `capy.occurrence/3`; 1.0.15 reads only `object`, without a version change, so those drawings open view-only. The preview limit fell from 1024 to 512 px, so most of them can't even show their preview. The fixtures were re-saved through the new writer, so no test noticed. |
| Recovery sessions | `capy.session` v1, which reuses package records | Same as `.capy`. Sessions this build can't open are kept and offered with Discard. |

Preferences already keep every top-level field that still reads, including
after an unknown `version`. Workspace rows, toolbar rows and packages written by
1.0.15 still load on main. The problem is that nothing stops a change from
breaking these readers, and the failure handling deletes far more than the part
that failed.

## Stores in scope

| Store | Where | Rule |
| --- | --- | --- |
| Workspace store: workspaces, toolbars, editing state, switcher | SQLite on native hosts, one IndexedDB record on Web | Sections 1–4 |
| Preferences | `settings.json`, Android `SharedPreferences` and Web `localStorage`, all through `Settings::restore` | Sections 1 and 3; already field by field |
| Export presets | `export-presets` | Sections 1 and 2: keep each preset this build can read |
| Paper and colour-profile libraries | Content-addressed files plus an index | Section 1 for the index; files are never deleted by a reader |
| Exported files | `.capyworkspace`, `.capytoolbar` and `.capykeys`, which painters keep and import later | Sections 1 and 3 |
| Drawings and sessions | `.capy`, session store heads, generations and recovery metadata | Sections 1 and 3, with `.capy`'s closed field sets |
| Palette files | `.gpl` and `.ase` | External formats; out of scope |
| Caches, temp files, file-dialog folders | — | May be discarded |

## Design

### 1. Change formats in ways that need no conversion

These rules need no new code. Each of today's breaks broke one of them.

- **Never rename, reuse or remove a saved key, ID or enum name, and never change
  the meaning of an existing key.** A new meaning gets a new key; the old one is
  converted by a step (section 3). An older reader therefore never misreads a
  key it knows. `#[serde(rename = "...")]` keeps a stored name when the Rust name
  changes.
- **A field added after 1.0.15 has a default** (`#[serde(default)]` on the field).
  Identity, ownership and structural fields stay required.
- **Changing a field's type, including a fixed array's length, needs a step.**
  `region_sources` growing from `[_; 2]` to `[_; 3]` is the example.
- **App-state readers ignore fields they don't know.** Remove
  `deny_unknown_fields` from saved app-state types: workspace rows, toolbar
  definitions, editing and working state, packages, export presets and the paper
  index. It turns every field removal into a lost record and makes every older
  build reject newer data. Size limits and `validate` still bound what is
  accepted.
- **No reader requires an exact version.** Older versions run their steps; newer
  ones are read as far as they go.
- **`.capy` keeps its closed field sets.** A required record or field this build
  doesn't know still makes the drawing view-only, and the file is never modified
  ([package contract](../reference/capy-package.md#object-records-and-defaults)).
  New optional data that older builds can safely drop uses the existing
  `ancillary` records.

Precedent: Protocol Buffers requires removed field numbers and names to be
[reserved, never reused](https://protobuf.dev/programming-guides/proto3/#deleting),
because a reused number silently reinterprets old data. Martin Fowler's
[Tolerant Reader](https://martinfowler.com/bliki/TolerantReader.html) describes
the matching rule for consumers: read only what you need and ignore the rest.
PNG makes the same split as `.capy`: a decoder must understand every critical
chunk and may skip unknown
[ancillary chunks](https://www.w3.org/TR/png/#5Chunk-naming-conventions).

### 2. Keep what can be read; never reset for one bad record

- **Field-by-field restore for single documents.** Turn the algorithm in
  `Settings::restore` into one shared helper. It decodes the whole document
  first, then on failure keeps each top-level field that still reads and gives
  the rest defaults. Use it for `Settings`, `EditingState`,
  `WorkspaceWorkingState` and each export preset. A palette list that can't be
  read then costs the palettes, not the brush settings as well. For the
  workspace store, the version snapshot (section 4) holds the original.
  Preferences and export presets lose only the fields that fail to read.
- **Unreadable rows are skipped and kept.** An unreadable workspace, toolbar,
  switcher row or pending delivery is never a reason to reset.
  - Custom rows stay listed as Unreadable workspace and are never rewritten.
  - Maintenance finds component references in pending deliveries by scanning the
    raw JSON, as it already does for item content, instead of decoding them. A
    delivery it can't read therefore keeps its components and stays in place.
- **A save changes only the editing fields its window changed.** Editing state
  is shared by every window but loaded once at startup, and each save writes
  the whole document. So a colour picked in one window drops a palette or brush
  override saved from another, even when both run the same build. Instead, a
  save sends only the top-level fields that differ from what its window last
  saved. The store merges them into the stored document, validates the result
  and returns it, and the window adopts the returned values for fields it
  hasn't changed since. Two windows that change the same field, such as both
  editing palettes, still keep the later save.
- **When every workspace is unreadable,** the included workspaces are rebuilt
  from their included layouts, but only in a start whose version snapshot
  succeeded. In any other start, startup runs in memory with the existing notice
  and writes nothing.
- **Reset only for a table layout older than the first release.** Later table
  changes are steps (section 3).

Precedent: Android Room's
[`fallbackToDestructiveMigration`](https://developer.android.com/reference/androidx/room/RoomDatabase.Builder)
re-creates the database whenever no migration path exists, and Room's docs warn
that this deletes all data. An app that enables it loses user data on any update
whose migration is missing, without an error. Our whole-store reset is the same
pattern. Chromium's
[`JsonPrefStore`](https://chromium.googlesource.com/chromium/src/+/HEAD/components/prefs/json_pref_store.cc)
moves an unparsable preferences file aside as `.bad` and starts empty. The user
keeps a copy but loses every setting. For a file that parses, field-by-field
restore keeps everything except what failed. `Settings::restore` already works
this way, so this extends an existing helper rather than adding a mechanism.
Sending only changed fields is
[JSON Merge Patch](https://www.rfc-editor.org/rfc/rfc7396): a patch replaces
the members it names and leaves the rest. It avoids the lost update that a
whole-document overwrite causes when two writers start from the same read.

### 3. A conversion step only where meaning changed

A step converts the raw JSON of one document type from one version to the next.
It is written only for a concrete change, never in advance:

```rust
fn upgrade_editing(stored: u32, value: &mut serde_json::Value) -> Result<(), String> {
    if stored < 2 {
        // region_sources gains its third entry; smoothing > 0 becomes smooth_edges.
    }
    Ok(())
}
```

- **Where the version comes from:** documents that carry one keep it: editing
  state, layout history, working state, preferences, packages, the session
  envelope and each `.capy` record type ID. A document without one, such as a
  toolbar definition, gets one only with its first step, and a missing version
  means the 1.0.15 meaning.
- **`.capy` steps** sit next to the record table in
  `crates/layer-core/src/package/registry.rs`, keyed by record type ID, so a
  `capy.occurrence/3` record becomes `/4` before decoding. SQL table changes are
  `ALTER TABLE` steps keyed by `user_version`.
- **Steps work on `serde_json::Value`,** never on frozen copies of old Rust
  types, so a step is a few lines and old types don't pile up in the code.
- **Steps run on read, in memory.** Writers write the current version. A `.capy`
  file changes only when the painter saves it.
- **Every step from 1.0.15 on ships forever.** Painters skip releases, and
  drawings outlive any release.

Precedent: Blender opens files from two decades ago by running every
version-gated step written since, from the `versioning_*.cc` files in
[`blenloader`](https://projects.blender.org/blender/blender/src/branch/main/source/blender/blenloader/intern),
each guarded by the version the file was saved with. Because each step is local
to one version, the code grows with the number of changes but stays readable.
Room's numbered `Migration` objects over SQLite's `user_version` do the same for
tables. With section 1 in place most changes need no step, and the two above are
the only ones known today.

### 4. A copy before another version writes the store

When the storage worker opens the native store, it takes the store's write lock
before maintenance or any other write. It then compares this build's version
with the one in `workspaces/last-version`. If they differ and the store has
items, it does the following, in order:

1. Copies the store with the existing `backup_database` to
   `workspaces.<recorded version>.<random id>.sqlite3`. The name uses `earlier`
   when no version is recorded, and the random ID comes from `new_id`, so two
   copies never share a name. The existing code publishes the copy by an
   fsynced rename.
2. Records its own version.
3. Once startup has adopted a workspace, prunes old copies.

Because the lock is held during the copy, no other process writes until the copy
exists. If the copy fails, for example on a full disk, the marker stays as it
was and startup continues in memory with the existing notice. The next start
tries again.

**Retention:** keep the ten newest copies while they total at most 256 MB, and
never fewer than three. If a release converts data badly, the copy from before
it must still exist when the release that repairs it starts. That repair reads
the newest copy recorded by a version before the faulty one, while loading, so
pruning hasn't run yet. The recorded version in each name is what makes that
copy easy to find.

- **Why ten:** Android and Web painters run every release, and hotfixes can
  ship several a day, so the needed copy has to outlast several version changes.
- **Why 256 MB:** a store in daily use is about 5 MB, so ten copies take about
  50 MB. Only unusually large stores keep fewer than ten.

On Web, the same IndexedDB transaction that first writes the store also stores
the copy and the version, the way a reset already stores its `unreadable`
record. A quota failure aborts that transaction and leaves the store as it was.

Builds up to 1.0.15 still reset a store they can't read. They keep one
`unreadable` copy, and their next reset replaces it. Going back to such a build
therefore resets the store. So does a stale Pages cache that briefly serves the
1.0.15 page after a deploy, because the page is fetched network-first. On every
start, if that copy is newer than the version marker, it is copied into the
retained copies, so it outlives the next reset. On Web, the version record
remembers a hash of the `unreadable` record it last saw, and copies a different
one.

This covers whatever the rules and tests miss. It runs on every version change,
up or down, so going back to an earlier build is safe and the newer build's data
is kept. The copy holds the store as it was at the version change, so edits
made after it are not in it.

Precedent: Lightroom Classic
[copies the catalog before upgrading it](https://helpx.adobe.com/lightroom-classic/desktop/technical-support/workflow-issues/catalog-issues/catalog-faq-lightroom.html)
and leaves the original for earlier versions, so photographers can update
without a manual backup step.

### 5. Files from each release as fixed test inputs

- **Release inputs.** Each release adds
  `crates/layer-workspace/tests/compat/<release>/` and
  `crates/layer-core/tests/compat/<release>/`, written by that release's own code
  and never edited or re-saved:
  - a native `workspaces.sqlite3` and the Web store's JSON text;
  - editing state, preferences and export presets;
  - the paper and colour-profile library indexes;
  - `.capyworkspace`, `.capytoolbar` and `.capykeys` files;
  - `.capy` drawings and a session store.

  The content covers every variant by looping over every tool, command, tool
  slot and panel, all on toolbars, plus non-default brush overrides, palettes
  with swatches, every region source and value, a custom workspace, an edited
  included workspace, every layer kind and every `.capy` record type. Renaming
  any saved ID then fails a test, with no separate list of IDs to maintain. One
  drawing is saved from the released app, so its preview is covered as well.
- **Expected values** live separately in
  `crates/<crate>/tests/compat/expected/<release>/` and are blessed with
  `CAPY_BLESS=1`. The first expected file for a release is reviewed against its
  input field by field, so it doesn't lock in a loss the current reader already
  has. After that, a diff to it is reviewed like code. Inputs the reader must
  keep view-only, like the existing `unsupported-pass-through.capy`, expect that
  outcome.
- **Generator.** An ignored test in each crate, `write_compat_fixtures`, writes
  the inputs into the directory named by `CAPY_COMPAT_OUT`. Milestone 1 adds it
  and runs it once from a temporary worktree at the `v1.0.15` tag. After that,
  the release procedure runs it at the release commit.
- **Upgrade test.** `release_fixtures_load_without_loss` opens every input
  through the path the app uses: the startup controller for the store,
  `load_editing`, `import_package`, `Settings::restore`, `ExportPresets::restore`,
  package open and session load. It requires no reset or notice, and a decoded
  value equal to the expected file. Native store cases run with
  `cargo test --locked -p layer-workspace --features native`; everything runs on
  the CPU in the crates' normal suites.
- **Tolerance test.** `saved_documents_tolerate_missing_and_unknown_fields`
  removes each top-level field of each app-state input in turn and adds an
  unknown field to every object. It requires the rest to load. This keeps
  section 1 true without depending on review.
- **Snapshot tests.** Three cases:
  - A version change copies the store before its first write.
  - A failed copy leaves the store byte for byte unchanged and runs in memory.
  - Going up, down and up again keeps distinct copies, and pruning keeps the
    ten newest within 256 MB and never fewer than three.
- **Inputs stay fixed.** `.githooks/pre-push` additionally rejects a push that
  modifies or deletes a file under `crates/*/tests/compat/<release>/`; its
  existing ancestry and message checks stay as they are. The package contract
  already said not to replace a baseline with new writer output. That was done
  anyway for the `object` key, so the rule needs a check.

Precedent: Room
[exports a schema JSON per database version](https://developer.android.com/training/data-storage/room/migrating-db-versions)
into version control, and its `MigrationTestHelper` builds a database at an old
version and checks the migration against it. Our release inputs are the same
idea, with real data instead of empty schemas. Blessed expected files follow
rustc's UI tests (`--bless`), [`expect-test`](https://docs.rs/expect-test)
(`UPDATE_EXPECT=1`) and [`insta`](https://insta.rs/). A regenerated file shows
in the diff what changed instead of hiding it.

## Performance

- **Opening data:** The usual path is unchanged; the whole document decodes
  first. Field-by-field restore and steps run only for another version's data or
  a failed decode, once per document, where decoding runs today.
  - Editing state decodes on the controller's owner thread. Milestone 1
    measures the slow path on the low-tier tablet with a large palette library,
    and moves restore into the storage worker if it takes longer than a frame.
- **`.capy` steps** touch only the manifest JSON. Tile resources are never
  copied, decoded or rewritten by a step.
- **The snapshot** runs once per version change, on the storage worker, before
  adoption.
  - Its cost grows with the store. `HISTORY_BUDGET_BYTES` (100 MiB) bounds only
    trimmable layout history, not the whole store.
  - Disk use holds one copy over the limit until pruning.
  - Milestone 1 records the cold-start time to the first canvas frame on the
    low-tier tablet with a large store, with and without the copy.
- **Merged editing saves** read and write the one editing row inside the
  transaction that already writes it.
- **Tests** use small inputs and need no GPU.

## Safety

- A row this build can't read is never overwritten. The one exception is
  rebuilding included workspaces after a successful snapshot (section 2).
- Restore and steps run in memory; the stored original stays until the next
  accepted write. For the workspace store, the version snapshot holds it beyond
  that.
- A failed snapshot means no persistent write in that start.
- A merged editing save is validated as a whole before it commits.
- Steps receive untrusted JSON only after the existing size and depth limits,
  and walk only the fields they change.
- Snapshot paths go through `validate_database_export_destination`, so they
  never land inside the lock store.

## Not doing

- **No migration framework, schema registry or frozen copies of old Rust types,
  and no steps written in advance.**
- **Unknown fields are not preserved when an older build rewrites a document.**
  Protocol Buffers does preserve them, but every saved type would need an extra
  catch-all field. Going back is covered by the snapshot instead, and the only
  fields lost are options the older build doesn't have.
- **No fencing between processes of different versions.** Readers from 1.0.16 on
  read each other's data, and the snapshot protects the rest.
- **No per-commit samples of the current writer.** Only released data reaches
  painters, and the release inputs are checked on every push.
- **No CI job that builds an earlier release, and no render comparison.** The
  expected files compare authored data exactly.
- **No compatibility for data from before 1.0.15.** 1.0.14 drawings with image
  layers stay view-only.
- **No restore command and no automatic repair of a damaged SQLite file.** The
  storage guide explains how to restore a copy by hand, and a repair release can
  read the copies itself.

## Work

Milestone 1 lands before the next release, as a few commits, each with its
tests and the guide changes it makes true.

1. **Before the next release.**
   - The generator, and every 1.0.15 input (app state, libraries, exported
     files, drawings and sessions) with its reviewed expected file.
   - The shared field-by-field restore.
   - Field defaults, and no `deny_unknown_fields` on saved app-state types.
   - Exact version checks replaced by the editing-state step for
     `region_sources` and `smoothing`.
   - Editing saves that merge changed fields, with the regression
     `editing_saves_from_two_windows_keep_both_changes`.
   - No store reset for unreadable rows or deliveries.
   - The version snapshot on native and Web, including keeping an older build's
     `unreadable` copy.
   - The upgrade, tolerance and snapshot tests, and the `pre-push` check.
   - A step in [releasing](releasing.md) that runs the generator at the release
     commit and commits its output, and a line in each release's notes that
     going back to 1.0.15 or earlier resets workspaces.
   - One line each in [testing](testing.md) naming the upgrade and tolerance
     tests.
   - Guide updates: [settings](../ui/settings.md#saving-and-loading),
     [workspace startup](../internals/workspace-ownership.md#startup-always-adopts-a-workspace),
     and [storage](../internals/storage.md). The storage guide lists the copies
     and how to restore one by hand.

   Done when every 1.0.15 input loads with its expected value, a 1.0.15 store
   with custom palettes, brush overrides and toolbars upgrades with nothing lost
   on each host, two windows keep each other's editing changes, and the tablet
   measurements are recorded. The hosts are GTK with the store in
   `CAPY_STORAGE_DIR`, Web with the `--stale-storage` journey seeding the 1.0.15
   record, Android, Windows and, where available, Apple.
2. **With the first `.capy` change that needs it:** the record step table next
   to `registry.rs`, with that change's step and its test against the 1.0.15
   inputs.

[`AGENTS.md`](../../AGENTS.md) already states the rules in section 1. When
milestone 1 lands, its link points to the guides that own them, and this plan
is deleted.

# Saved data compatibility

[Design history](README.md)

This records the saved-data compatibility design as of 1.0.15. The failure
inventory describes the behavior that prompted the changes, before the fixes.
Current contracts live in [settings and persistence](../ui/settings.md#saving-and-loading),
[storage](../internals/storage.md#workspace-version-copies) and the
[package contract](../reference/capy-package.md).

## Goal

The goal is to preserve data written by 1.0.15 across upgrades: workspaces,
saved toolbars, palettes, brush settings, preferences, export presets, drawings
and recovery copies. Updated readers keep readable fields and records, with
concrete conversions where meaning changed and copies before version changes.
Older shipped builds retain the downgrade limits described below.

Five mechanisms address the observed failures. Each follows a practice that
other long-lived applications adopted after similar failures.

## What fails today

| Data | Reader | What happens with another build's data |
| --- | --- | --- |
| Editing state: palettes, swatches, colour history, brush overrides, remembered tools, selection, fill and gradient options | `WorkspaceManager::load_editing` decodes one strict `EditingState` | Any mismatch discards all of it. A 1.0.15 copy fails on main in four ways: `region_sources` grew from two to three entries, three fields were added without defaults, `version` must now be exactly 3, and the region value `smoothing` became `region_smooth_edges`. In 1.0.15 the same failure reset the whole store. |
| Workspaces | `Entity` rows with `deny_unknown_fields` and exact layout-history versions | An unreadable workspace is skipped. When no workspace can be read, startup resets the whole store, including toolbars and editing state (`sqlite_startup_replaces_workspaces_of_the_same_version_it_cannot_read`). One layout change makes every stored workspace unreadable at once. Maintenance decodes every pending delivery, so one unreadable delivery fails startup the same way. |
| The reset copy | `preserve` in `crates/layer-workspace/src/sqlite.rs` | One `workspaces.unreadable.sqlite3`, replaced by the next reset. |
| Older build, newer store | `SCHEMA_VERSION` | It changes only for table changes, so an older build treats a newer store as its own and resets it when the item formats differ. |
| Export presets | `ExportPresets::restore` | A file this build can't decode counts as empty, and the next save replaces it. |
| `.capy` files | Closed field sets per record (`crates/layer-core/src/package/`) | 1.0.14 wrote occurrence content as `objects` inside `capy.occurrence/3`; 1.0.15 reads only `object`, without a version change, so those drawings open view-only. The preview limit fell from 1024 to 512 px, so previews above 512 px no longer display. The fixtures were re-saved through the new writer, so no test noticed. |
| Recovery sessions | `capy.session` v1, which reuses package records | Same as `.capy`. Sessions this build can't open are kept and offered with Discard. |

Preferences already keep every top-level field that still reads, including
after an unknown `version`. Workspace rows, toolbar rows and packages written by
1.0.15 still load on main. The problem is that nothing stops a change from
breaking these readers, and the failure handling deletes far more than the part
that failed.

## Stores in scope

| Store | Where | Rule |
| --- | --- | --- |
| Workspace store: workspaces, toolbars, editing state, switcher | SQLite on native hosts; database, version and snapshot records in IndexedDB on Web | Sections 1–4 |
| Preferences | `settings.json`, Android `SharedPreferences` and Web `localStorage`, all through `Settings::restore` | Sections 1 and 3; already field by field |
| Export presets | `export-presets` | Sections 1 and 2: restore readable preset metadata fields |
| Paper and colour-profile libraries | Content-addressed files plus an index | Section 1 for the index; files are never deleted by a reader |
| Exported files | `.capyworkspace`, `.capytoolbar` and `.capykeys`, which painters keep and import later | Sections 1 and 3 |
| Drawings and sessions | `.capy`, session store heads, generations and recovery metadata | Sections 1 and 3, with `.capy`'s closed field sets |
| Palette files | `.gpl` and `.ase` | External formats; out of scope |
| Caches, temp files, file-dialog folders | — | May be discarded |

## Design

### 1. Change formats in ways that need no conversion

These rules avoid conversions for ordinary additions. The failures above
show why stable names, defaults and tolerant app-state readers matter.

- **Never rename, reuse or remove a saved key, ID or enum name, and never change
  the meaning of an existing key.** A new meaning gets a new key; the old one is
  converted by a step (section 3). An older reader therefore never misreads a
  key it knows. `#[serde(rename = "...")]` keeps a stored name when the Rust name
  changes.
- **A field added after 1.0.15 has a default** (`#[serde(default)]` on the field).
  Identity, ownership and structural fields stay required.
- **Changing a field's type, including a fixed array's length, needs a step.**
  `region_sources` growing from `[_; 2]` to `[_; 3]` is the example.
- **App-state readers ignore fields they don't know.** Saved app-state types
  omit `deny_unknown_fields`: workspace rows, toolbar definitions, editing and
  working state, packages, export presets and the paper index. Strict unknown-
  field rejection turns ordinary additions into unreadable records in an older
  build. Size limits and `validate` still bound what is accepted.
- **App-state document readers do not require an exact version.** Released
  versions run their concrete conversions; newer ones retain readable fields.
  Physical schemas and file envelopes keep their own admission rules.
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

- **Field-by-field restore for single documents.** The algorithm from
  `Settings::restore` is shared by the saved-state readers. It decodes the whole
  document first, then on failure keeps each top-level field that still reads and gives
  the rest defaults. It serves `Settings`, `EditingState` and
  `WorkspaceWorkingState`. Export presets restore the top-level `profiles`,
  `destinations` and `named` metadata fields with validation, keeping references
  consistent without per-preset salvage or reindexing. The binary envelope,
  digest and bounds remain strict. A palette list that cannot be read costs the
  palettes, not the brush settings as well. For the
  workspace store, the version snapshot (section 4) holds the original.
  Preferences and export presets lose only the fields that fail to read.
- **Unreadable rows are skipped and kept.** An unreadable workspace, toolbar,
  switcher row or pending delivery is never a reason to reset.
  - Custom rows stay listed as Unreadable workspace and are never rewritten.
  - Maintenance finds component references in pending deliveries by scanning the
    raw JSON, as it already does for item content, instead of decoding them. A
    delivery it can't read therefore keeps its components and stays in place.
- **A save changes only the editing fields its window changed.** Editing state
  is shared by every window but loaded once at startup. Whole-document saves
  let a colour picked in one window drop a palette or brush override saved from
  another, even when both run the same build. A save therefore names the
  top-level fields that differ from what its window last submitted. Its immutable payload keeps the complete document for recovery
  and fallback. The store merges the named fields into the stored document and
  validates the result inside the write transaction. If validation fails, it
  replaces the whole document with the submitted one. Receipts and live state
  do not change; windows do not adopt another window's editing values. Two
  windows that change the same field, such as both editing palettes, still keep
  the later save.
- **When no workspace can be read,** startup creates a new workspace from the
  included layout and leaves the unreadable rows untouched.
- **Reset only for a table layout older than the first release.** Later table
  changes are steps (section 3).

Precedent: Android Room's
[`fallbackToDestructiveMigration`](https://developer.android.com/reference/androidx/room/RoomDatabase.Builder)
re-creates the database whenever no migration path exists, and Room's docs warn
that this deletes all data. An app that enables it loses user data on any update
whose migration is missing, without an error. The former whole-store reset followed this
pattern. Chromium's
[`JsonPrefStore`](https://chromium.googlesource.com/chromium/src/+/HEAD/components/prefs/json_pref_store.cc)
moves an unparsable preferences file aside as `.bad` and starts empty. The user
keeps a copy but loses every setting. For a file that parses, field-by-field
restore keeps everything except what failed. `Settings::restore` already works
this way, so this extends an existing helper rather than adding a mechanism.
Replacing only named top-level members avoids the lost update that a
whole-document overwrite causes when two writers start from the same read.
This is the top-level case of
[JSON Merge Patch](https://www.rfc-editor.org/rfc/rfc7396), except null is kept as
a value. Nested objects are replaced as complete fields.

### 3. A conversion step only where meaning changed

A conversion changes raw JSON before typed decoding and validation. It exists
only for a released representation whose meaning changed; it does not require
frozen copies of old Rust types.

- **The concrete editing conversion is version 1 to current version 3.** It
  extends two region-source entries with Auto and turns numeric smoothing into
  the `region_smooth_edges` boolean. Version 2 never shipped and has no
  conversion. New option fields have defaults.
- **Versions belong to the data they describe:** preferences, editing and working
  state, layout history, packages, session envelopes and authored record IDs.
  A document without a version does not gain one for a hypothetical conversion.
- **Conversions run on read, in memory.** Loading does not rewrite the original;
  writers save the supported representation on the next accepted save.
- **Released meanings remain readable.** Concrete authored-data changes follow
  the [package contract](../reference/capy-package.md#adding-controls-after-ga)
  and unchanged-file regressions. SQL table conversions use `user_version`.
  No authored-record step table or pre-1.0.15 converter is part of this design.
- **Keep conversions from 1.0.15 onward.** Painters skip releases, and drawings
  outlive any release.

Precedent: Blender keeps version-gated conversions in the `versioning_*.cc`
files in
[`blenloader`](https://github.com/blender/blender/tree/main/source/blender/blenloader/intern),
each guarded by the version the file was saved with. Because each step is local
to one version, the code grows with the number of changes but stays readable.
Room's numbered `Migration` objects over SQLite's `user_version` do the same for
tables. Stable names and defaults keep most changes out of this machinery. The
editing-state conversion addresses the concrete released mismatch.

### 4. A copy before another version writes the store

When the storage worker opens the native store, it takes the store's write lock
before maintenance or any other write. It then compares this build's version
with the one in `workspaces/last-version`. If they differ and the store contains
persisted data, including editing or pending data, it does the following in
order:

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

**Retention:** keep the ten newest copies within 256 MiB, retaining at least
three even when those three exceed the byte budget. Pruning runs only after
workspace adoption, so the pre-change copy survives loading. A failed prune
does not discard the adopted workspace.

- **Why ten:** hotfixes can ship several times a day, so a useful recovery point
  needs to outlast several version changes.
- **Why a byte budget and a floor:** count alone does not bound tablet or browser
  storage, but a large store still needs multiple recovery points. A 5 MB store
  was a planning estimate, not a measured daily-store bound; the floor takes
  precedence over the budget.

Web keeps the marker and copies in separate IndexedDB records beside
`database` and `unreadable`, preserving the database shape that 1.0.15 reads.
The same transaction publishes the copy, marker and first mutation. Quota or
transaction failure publishes none of them; pruning uses copy metadata.

Builds up to 1.0.15 can still reset a store they cannot read. They retain one
`unreadable` copy, which another reset replaces. On native startup, a newer
legacy copy is preserved before advancing the marker; Web remembers the last
preserved legacy-copy hash. Returning to an older build, including a stale
network-first Web page, can therefore reset workspaces, while the retained
copies preserve earlier states.

Snapshots protect the workspace store at capture time, on version changes in
either direction. They exclude preferences, export presets, libraries and
sessions, which have separate storage. They do not fence different app versions
or capture later edits by another process. Restore is manual, as described in
the [storage guide](../internals/storage.md#workspace-version-copies).

Precedent: Lightroom Classic
[copies the catalog before upgrading it](https://helpx.adobe.com/lightroom-classic/desktop/technical-support/workflow-issues/catalog-issues/catalog-faq-lightroom.html)
and leaves the original for earlier versions, so photographers can update
without a manual backup step.

### 5. Files from each release as fixed test inputs

- **Release inputs.** Each release adds
  `crates/<crate>/tests/compat/<release>/` in `layer-workspace`, `layer-core`,
  `layer-ui` and `layer-host`, written by that release's own code
  and never edited or re-saved:
  - a native `workspaces.sqlite3` and the Web store's JSON text;
  - editing state, preferences and export presets;
  - the paper and colour-profile library indexes;
  - `.capyworkspace`, `.capytoolbar` and `.capykeys` files;
  - `.capy` drawings and a session store.

  The content covers every variant by looping over every tool, command, tool
  slot and panel, all on toolbars, plus non-default brush overrides, palettes
  with swatches, every region source and value, a custom workspace, an edited
  included workspace, every layer kind and every `.capy` record type. These inputs
  make saved-ID changes visible without a separate inventory of tool, command,
  slot and panel IDs. One drawing is saved from the released app, so its preview is covered as well.
- **Expected values** live separately in
  `crates/<crate>/tests/compat/expected/<release>/` and are blessed with
  `CAPY_BLESS=1`. The first expected file for a release is reviewed against its
  input field by field, so it doesn't lock in a loss the current reader already
  has. After that, a diff to it is reviewed like code. Inputs the reader must
  keep view-only, like the existing `unsupported-pass-through.capy`, expect that
  outcome.
- **Generator.** An ignored test in each crate, `write_compat_fixtures`, writes
  the inputs into the directory named by `CAPY_COMPAT_OUT`. The 1.0.15 inputs
  were written from a temporary checkout of the `v1.0.15` tag with test-only
  helpers. The
  [release procedure](../development/releasing.md#saved-data-fixtures) runs the
  generators at the release commit.
- **Upgrade test.** `release_fixtures_load_without_loss` opens every input
  through the path the app uses: the startup controller for the store,
  `load_editing`, `import_package`, `Settings::restore`, `ExportPresets::restore`,
  package open and session load. It requires no reset or notice, and a decoded
  value equal to the expected file. Native store cases run with
  `cargo test --locked -p layer-workspace --features native`; everything runs on
  the CPU in the crates' normal suites.
- **Tolerance test.** `saved_documents_tolerate_missing_and_unknown_fields`
  removes defaultable option fields and adds unknown app-state struct fields.
  Independent values remain readable; required identities and structure still
  fail safely. Closed artwork records retain their separate strict checks. This
  checks the rules in section 1 without depending on review.
- **Snapshot tests.** Three cases:
  - A version change copies the store before its first write.
  - A failed copy leaves the store byte for byte unchanged and runs in memory.
  - Going up, down and up again keeps distinct copies, and pruning keeps the
    ten newest within 256 MiB and never fewer than three.
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
rustc's UI tests (`--bless`), [`expect-test`](https://docs.rs/expect-test/latest/expect_test/)
(`UPDATE_EXPECT=1`) and [`insta`](https://insta.rs/). A regenerated file shows
in the diff what changed instead of hiding it.

## Not doing

- **No migration framework, schema registry or frozen copies of old Rust types,
  and no steps written in advance.**
- **Unknown fields are not preserved when an older build rewrites a document.**
  Protocol Buffers does preserve them, but every saved type would need an extra
  catch-all field. The workspace snapshot preserves the capture-time original;
  separate stores and later edits are outside that copy.
- **No fencing between processes of different versions.** Readers keep the
  fields they understand, and snapshots preserve the store at capture time.
  Concurrent later writes are not fenced.
- **No per-commit samples of the current writer.** Only released data reaches
  painters, and the release inputs are checked on every push.
- **No CI job that builds an earlier release, and no render comparison.** The
  expected files compare authored data exactly.
- **No compatibility for data from before 1.0.15.** 1.0.14 drawings with image
  layers stay view-only.
- **No restore command and no automatic repair of a damaged SQLite file.** The
  storage guide explains how to restore a copy by hand.

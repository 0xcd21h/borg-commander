# Virtual panels, indexed search, and size scanning

Status in 0.4.0: collections, results presentation, live recursive filename search,
and folder size scanning are implemented. Folder totals currently sum logical content
bytes; allocated and reclaimable size accounting remain follow-ups. The live
implementation caps each search/collection at 20,000 entries; paging against a persistent
index is part of the later indexed-search slice. The sections below include that future
work as well as the implemented features.

## Outcome

Turn a panel into a working set of files from multiple folders. Populate it manually,
from a search, or from a size scan, then use the existing viewer and file jobs on the
results. Keep the terminal layout, keyboard navigation, mouse controls, and shell.

Build this in three independently usable steps: collections, indexed filename search,
and directory size scanning. Content search follows after these foundations.

## One results panel, three sources

| Source | Membership | Default view | Save behaviour |
| --- | --- | --- | --- |
| Collection | Explicit references added by the user | Name and source folder | Save the chosen members |
| Search | Matches for a query within selected roots | Name and source folder | Save the query and scope; pin matches to a collection |
| Sizes | Files or directory aggregates under a root | Allocated size, descending | Save the scan scope and view; pin real entries to a collection |

A collection is not a temporary directory. Adding a file creates no filesystem copy.
Saved searches are live definitions; saved collections remember explicit choices.
Result groups and summary rows are presentation only, never file-operation targets.

## Results that feel like a commander

### Layout

Use the existing panel slots. One side shows results; the other remains a normal
folder destination or the existing quick preview. Offer an expanded results view
for paths and additional columns, with the same selection and navigation state.
Expanding and restoring must preserve the previous split and opposite panel.

The title identifies the source: `Collection: Release bundle`, `Search: *.log`,
or `Sizes: /workspace`. A compact context line shows scope and scan/index state.

Default columns:

- Collection and search: mark, name, source folder, size, modified.
- Sizes: mark, name, source folder, allocated bytes, logical bytes, scan state.
- Narrow panel: mark, name, size; keep the full source path in the focused-row footer.
- Wide panel: show source folder inline; optional type and additional date columns.

Use existing themes and icons. Known totals are numbers, partial totals have a `≥`
prefix, and missing information uses `—`, never a misleading zero. Do not restore
relative-size bars. A size-tree mode may add child counts and expandable directory
rows; the numerical columns remain the main comparison.

Illustrative expanded results layout:

```text
┌ Collection: Release bundle ────────────────────────────────────────────────┐
│ 8 items · 2 marked · 3 source folders              Group: folder  Sort: size ↓│
│   Name                  Source folder                  Size       Modified │
│ ▾ /workspace/releases                                                      │
│ * borg-linux.tar.gz     /workspace/releases             7.3 MiB    today    │
│ * borg-macos.tar.gz     /workspace/releases             6.9 MiB    today    │
│ ▾ /workspace/project                                                       │
│   release-notes.md       /workspace/project              4.2 KiB    today    │
├ /workspace/releases/borg-linux.tar.gz · available ──────────────────────────┤
│ View   Copy   Move   Remove from collection   Reveal original               │
└────────────────────────────────────────────────────────────────────────────┘
```

This is a content sketch; final cell widths and placement belong in render snapshots.

### Sorting, grouping, and filtering

- Click column headers to sort and reverse; show the active direction. Provide the
  same choices in the panel menu, reachable with F9 or Esc then 9. Avoid assigning
  printable single-key commands that would steal shell input.
- Sort by name, source folder, extension/type, size, modified, or collection order.
  Size mode distinguishes allocated and logical size. Use locator as a deterministic
  final tie-break; keep the focused row and marks by identity, never row number.
- Group by none, source folder, root, or file type. Group headers show item counts and
  applicable totals, collapse independently, and cannot accidentally become selected files.
- A labeled filter narrows the displayed rows without changing collection membership
  or a saved search. Show visible, total, and marked counts separately. The action
  summary explicitly includes marked files hidden by the filter.
- Group marking applies only to the group's visible matching items. Overlapping
  directories and descendants are normalised when preparing a real file job.
- Changing grouping or sort preserves marks. Filter changes may move focus to the
  nearest visible item; clearing the filter restores the previous anchor when possible.
- New scan/index data updates progress and metadata without repeatedly moving rows
  under the cursor. Show that newer results are available; Refresh applies a fresh
  ordered result snapshot. Opening an initial search streams bounded pages.

### Collection interactions

- Add marked files, or the focused file, through a clearly named panel menu action.
  The action can create a collection or choose an existing one.
- F5 with a collection as destination adds references. Label the bottom action
  `Collect` in this context. Between two collections it also adds references.
- F5 from a collection to a real folder uses normal Copy jobs. F6 Move requires a
  real destination; moving into a collection is unavailable, with an explanation.
- In a collection, F8 is labeled `Remove` and removes references only. Deleting original
  files is a separate `Delete originals…` action with the real paths in its confirmation.
  Search and size results use the same explicit action for deleting originals.
- F3 and Ctrl-Q resolve the actual source and reuse the existing viewer/preview;
  images, large-file reads, scroll position, and source toggles behave identically.
- Enter on a directory opens that directory while retaining the results in panel
  history. Reveal original opens its parent and focuses the file in the other panel.
- Preserve the real directory as the shell working directory when entering a results
  panel. Display it at the prompt; results have no invented filesystem cwd.
- Repeated additions deduplicate the same source locator. Identical basenames from
  different directories remain separate and show their origins.
- Missing items remain visible and removable; file actions recheck current sources.
  Moves performed by Borg update affected references after job outcomes are known.
  External renames may leave missing entries until the user repairs or removes them.
- Git actions resolve selected source paths. Never run them relative to a fictitious
  collection root. Initially disable mixed-repository Git actions with a clear reason;
  preserve single-repository actions and normal repository checks.

## Indexed filename search

Index only user-selected roots. Offer live recursive filename search before the first
index exists, and keep it usable with indexing disabled. Keep current-directory
Ctrl-S quick find; recursive search gets a separate menu command and window.

First queries cover substring/glob, extension, file kind, size range, and modified range.
Use familiar form fields first, with an optional readable query representation.
Match the existing quick-find case convention. Full-text and regex query languages
are outside this first slice.

### Storage and reconciliation

Propose a local SQLite cache, behind a small index interface. Resolve the Rust adapter
and migration details during implementation; the cache is replaceable derived data.

Store root, provider, raw source path, parent, display name, kind, logical size,
allocated size when available, modification time, optional filesystem identity,
observation generation, and scan errors. Keep paths lossless on Unix; display text
is not a reversible source locator. Index metadata without reading file contents.

- One bounded scan coordinator serves metadata discovery and size aggregation.
  Do not run independent recursive walkers for search and disk sizes.
- Stream directory entries and write bounded batches. Apply backpressure; never
  collect a whole directory tree or the entire index in the UI state.
- Overlapping roots share entries and deduplicate query results. Keep scan policy
  and completion state per root. Removing a root removes cache coverage, not files.
- Maintain resumable scan generations. Publish a rebuilt generation atomically;
  keep the previous usable index if a rebuild is cancelled or interrupted.
- Delete absent cached entries only after successfully reconciling their parent
  scope. A permission error must not make descendants look deleted.
- Borg jobs invalidate affected paths. Refresh/reconcile selected roots on demand
  and on reopening, displaying the last successful observation time.
- Add filesystem-event adapters after the initial reconciler. Treat events as
  invalidation hints; lost events or watcher overflow require reconciliation.
  Changes while Borg was closed must not be presented as a continuously fresh index.
- Default to no symlink traversal and no crossing into other mounted filesystems.
  Make hidden/ignored-folder policy explicit and saved per root; display exclusions.
- Support pausing, cancellation, rescan, root removal, and full cache rebuild.
  Only one active generation per root; obsolete requests cannot publish late results.
- Page queries into the core using a query token, stable ordering, and a bounded row
  window. Do not copy every match into the current `Panel.entries` vector.

Cache goes under the platform cache directory and respects XDG_CACHE_HOME / Borg's
existing directory conventions. Explicit collections live in persistent state, saved
search definitions in configuration. Rebuilding or deleting the index preserves both.

## Size scanning

Expose `Calculate sizes` on a directory, plus a `Largest items` results view. Reuse
indexed metadata where covered, and show its age; update through the shared scanner.
A live scan works without enabling a persistent index.

- Show files early and propagate directory aggregates as children complete.
- Distinguish not scanned, scanning, partial, complete-at-time, failed, and stale.
  Permission-denied and skipped subtrees contribute visible coverage errors.
- Store logical and allocated sizes separately. File count and disk free space are
  distinct metrics. Never describe an allocated-size sum as guaranteed recoverable space.
- Detect hard links using filesystem identity where available. Compute a unique
  allocated total for a selected set so repeated references and parent/child overlap
  do not inflate it. Per-directory totals can overlap; label them and do not add
  arbitrary directory rows together as an exact disk total.
- Report unmeasured allocation as unknown. Symlinks account for the link unless the
  user explicitly chooses traversal; apply cycle and mount boundaries consistently.
- Sorting incomplete size values keeps their uncertainty visible; missing values
  sort last. Mark an explicitly requested largest-first snapshot as partial if necessary.
- Ordinary collection/search footers total known file sizes and report unmeasured
  directories separately. Do not trigger recursive scans merely to draw a footer.

The size view is inspection only in this slice. Cleanup recommendations, duplicate
content hashing, Time Machine exclusions, and snapshot management are later features.

## Fit to the current code

Current `Panel` stores a directory and directory-local `Entry` values. Selections,
preview paths, and Git status often reconstruct a source with `dir.join(entry.name)`.
That assumption must be removed from shared actions before mixed-directory rows ship.
The current VFS backend exposes directory listing only; it is not a complete remote
filesystem abstraction.

1. Introduce a source locator and stable row identity in `borg-vfs`, initially local
   paths with a provider identifier. Preserve symlink paths and non-UTF-8 bytes;
   don't canonicalise references into different targets. Leave room for remote locators
   without implementing remote storage or credentials now.
2. Add a panel-source enum: directory, collection, search, sizes. Extract common
   cursor, mark, scroll, sort, and view state rather than duplicating four panels.
3. Make core actions resolve selections to explicit locators and destinations.
   The local adapter produces the existing `borg-jobs` path-based specs.
4. Route preview, viewer, reveal, editor, shell filename insertion, and Git actions
   through the source resolver. Preserve existing local-directory behaviour.
5. Add pure collection/query/scan state and typed effects in `borg-core`. Execute
   persistence, SQL, directory walks, and allocation queries in `borg-app` / a small
   index-and-scan crate, keeping them out of rendering and the pure update loop.
6. Add reusable results rows and column geometry in `borg-ui`, including mouse hit
   testing. Paging and sorting produce typed effects; rendering performs no I/O.

Do not build an external plugin API for this work. Collections and results are core
panel behaviour; later optional tools can produce entries for these same sources.

## Delivery sequence and acceptance

### A. Collections

Deliver explicit source resolution, create/name/save collections, add/remove members,
origin-aware sorting, persistence, normal copy/move jobs, and shared previews.

Accept when two same-named files from different folders can be collected, independently
marked, previewed, sorted, copied, and restored after restart. Removing a reference
must leave its original intact. Normal panels must retain their current behaviour.

### B. Results presentation and live search

Deliver column sorting, grouping, filtering, expanded view, source reveal, bounded
result pages, cancellation, and live recursive metadata search.

Accept when sorting/filtering preserves selection identity, column clicks agree with
keyboard actions, group headers cannot enter jobs, and a replaced query cannot
publish stale results. Existing viewer performance remains unchanged.

### C. Rebuildable metadata index

Deliver selected-root management, cache-backed queries, scan coverage, invalidation,
reconciliation, and cancel-safe rebuilds. Live search remains available.

Accept when indexed and live results agree on a static fixture, deleted cache files
are rebuilt, cancelled rebuilds preserve the last generation, denied subtrees stay
explicitly incomplete, and changed roots reconcile after restarting Borg.

### D. Size results

Deliver incremental logical/allocated aggregates, largest-first results, tree drill-down,
coverage states, and duplicate-aware selected totals. Add event-driven invalidation
only after manual reconciliation is correct.

Accept when a fixture with hard links, symlinks, sparse files, denied directories,
overlapping roots, and nested selections produces defined, non-inflated totals or
explicit unknown/partial states. Reusing the metadata scanner must avoid a second walk.

## Validation and performance

Use unit tests for identity, action semantics, sorting, grouping, query matching,
reconciliation, cancellation, and accounting. Add render snapshots at narrow, normal,
and wide sizes, covering same-name rows, missing members, filters with hidden marks,
partial scans, and resized panels. Add real-session tests for collection operations,
restart persistence, search-to-collection, quick preview, and cancelling scans.

Benchmark on a generated million-entry index without requiring a million real files.
Proposed target: first 200 cached matches within 200 ms on a documented reference
machine. Record cold and warm timings separately. Measure scan memory, UI input
latency during active scanning, query cancellation, and time to first results.

Start with bounded batches and a small worker pool; choose exact queue, page, worker,
and memory budgets from measurements. Queue length and UI row cache must stay bounded
as total file count grows. Run workspace tests, formatting, and Clippy before shipping.

## Explicit follow-ups

Full-text search should use this same result model, returning a file plus a match
location and snippet that the shared viewer can open. Start with a cancellable live
content scan; evaluate optional persistent text indexing separately, with file-type,
size, binary, and cache-budget policies. Remote panels, cleanup rules, Time Machine
controls, and an operations/storage journal remain outside this implementation slice.

# Store

Gazette uses a SQLite journal for the workspace registry and release history. The registry lists sites, projects, pages, routes and relationships and can be rebuilt from the workspace. Release history is append-only. Keep its preparations, approvals, attempts, observations and settlements. The site cache sits beside the journal and can be deleted. It is keyed by the inputs that can change the rendered bytes.

## Creating a journal

```sh
gazette initialize-store --help
```

Run `initialize-store` against an empty private directory. It refuses to overwrite, import or upgrade an existing database. Inspection, backup, verification and restore also refuse an unsupported version, altered schema or foreign tables.

The current schema is version **6**. A new journal applies `0001-journal.sql`, `0002-collection-purpose.sql`, `0003-collection-navigation.sql`, `0004-project-lifecycle.sql`, `0005-collection-order.sql` and `0006-project-subject.sql`. The registry stores a collection's optional purpose, authored navigation and member order separately from its page families and containment, and accepts the five project states: planned, active, paused, done and closed. Project kind and public entity identity have separate optional columns. This executable refuses journals from versions 1 through 5. There is no in-place upgrade or automatic reclassification of old project records. Keep the old journal and its backups when you change executable versions.

For release work, run `gazette enroll-local-coordinator` before the first write. It keeps an anchor outside the journal with the state expected after each committed change. This detects a journal restored on its own after an accident or crash. Someone who controls both files can change both.

## The writer and the journal header

A single writer holds the journal at a known generation. A write request supplies the expected header, `{schema_version, journal_id, journal_epoch, writer_fence}`. If it differs, Gazette refuses the write. Acquiring a writer advances the generation even if the following operation fails.

`gazette acquire-writer`, `gazette release-session` and `gazette recovery-session` hold a writer. `gazette inspect-journal` and `gazette inspect-store` run read-only SQL and acquire no writer generation. SQLite WAL and shared-memory coordination files can still require maintenance, which has its own permission.

## The registry

`gazette reconcile-registry` replaces all workspace rows in one transaction and reads them back. Removing a declaration removes its rows. The chosen publication identifies which source bytes to read. The registry still includes configured foreign sites and disabled or unused declarations. Use `gazette inspect-registry` to check all rows without writing.

Rebuildable means the retained workspace inputs can supply the registry again; it does not promise that the whole rebuild is atomic or succeeds in one pass. Gazette prepares the complete model, acquires a writer, replaces the rows and closes the writer in separate stages. The row replacement is atomic, but a later failure does not undo an acquired writer generation or an already verified replacement. Inspect the reported stage and store state before retrying.

The registry includes each project’s repository address and declared origin (`original`, `successor-fork` or `convenience-mirror` with an upstream), its single `primary` placement and any `mirror` placements, plus routes, aliases and redirects served for other sites. See the [content model](./content-model.md) for their meanings.

Principal websites and ordered `same_as` references have their own rows. A change to either changes the desired registry identity. A site website keeps its declared site reference, while an external website keeps its explicit URL. Neither proves identity or supplies a contact address. The current schema creates these tables only for a new journal.

## Backing up

```sh
gazette backup-store --help
gazette verify-backup --help
```

::::steps

:::step "Choose a fresh backup directory"

Declare the authored, staging, generated and live journal roots. Gazette checks that the new directory is outside them.

:::

:::step "Save the manifest digest separately"

`backup-store` uses SQLite's online backup API for a consistent copy. It reopens the copy read-only, checks its integrity and writes `backup.json` next to `store.sqlite`. Save the reported manifest digest outside the backup.

:::

:::step "Verify the copy"

`verify-backup` uses that digest to check the manifest, then checks the database. A backup has exactly two files. Symbolic links or extra files cause verification to fail. A failed backup remains in its original directory, marked unverified. Use a new directory for another attempt.

:::

::::

## Restoring

`gazette prepare-store-restore` verifies the backup and copies it to a fresh directory with `restore.json`. The live journal stays in place. To switch journals, run `gazette adopt-store-restore` with the latest clean anchor state and the original restore record. Every release in the old journal must be settled first. Adoption creates a successor and leaves the old journal untouched.

## Limits

Defaults are 256 MiB for the backup database, 16 KiB for an output directory path and 256 KiB for a command report. Run the command with `--help` to see its limits. When a report cannot be delivered, Gazette returns the information needed to inspect the result: the initialized header, backup manifest digest or restore record digest.

See [recovery](./recovery.md) for interrupted operations and [releasing](./releasing.md) for the release history.

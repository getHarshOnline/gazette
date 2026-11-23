# Recovery without guessing

Inspect an interrupted operation before you repeat it. A process exit or a missing reply leaves open whether the operation wrote files or pushed to a remote.

::::steps
:::step "Keep the original request and result"

Save the request, any result returned, the journal and the output path. You need these to identify the same operation after restarting.

:::
:::step "Inspect the destination"

For a push, read the remote refs. For a local write, inspect the destination or journal. Do not repeat a write or push to find out whether the first one worked.

:::
:::step "Continue from the state you can verify"

Compare the observed state with the original request. If you cannot determine what happened, preserve both and stop before making another change.

:::
::::

Gazette uses an ordinary approved Git push. GitHub Pages or Mintlify may then build from GitHub. The destination does not need a Gazette receiver or recovery script.

## Local output

`render-site`, documentation assembly and preparation write to fresh staging locations. To update a configured destination, use [the output writer](output-materialization.md). When staging reports leftover files or completed private output, keep the path it reports. Inspect the directory before deciding whether to keep or remove it. Use a fresh directory for the next build.

## SQLite store

Use `inspect-store` to read metadata without acquiring a writer. It does not create or upgrade a database. SQLite coordination-file maintenance needs separate permission even when the SQL is read-only. Check [journal compatibility](store.md#creating-a-journal) before using another executable version.

Use `backup-store` to create a verified copy and `verify-backup` to compare it with the manifest digest you stored separately. `prepare-store-restore` creates and checks a separate copy. It does not replace or open the live journal for writing. Keep the original store, backup, manifest and restore result until you have verified that the intended replacement is live. An old snapshot must never make a used authorization valid again or allow a second effect.

## Release transaction

Find the approval, its lease and consumption, the attempt, and its reconciliation separately in the journal:

```text
inspect-release-authorization
inspect-release-leases
inspect-release-consumption
inspect-release-attempts
inspect-release-reconciliations
```

Use `recovery-session` to inspect the interrupted transaction. It cannot dispatch or retry a release. Read the remote refs in the original request, record what the remote returned and reconcile the already consumed authorization. If bytes, destinations or preconditions have changed, obtain a new approval.

| What the remote shows | What you can record in `settlement` mode |
|---|---|
| The intended change happened | The release took effect |
| The original state is intact and no change happened | The release had no effect |
| The remote conflicts with both states | Keep the conflict for review |
| There is not enough information to tell | Keep the interruption unresolved |

For an interrupted receive-pack, `observe-interrupted` only records the observation. It cannot settle or seal the consumed release even if the current refs look conclusive. Provider build and served-site observations follow Git read-back and cannot change its result.

## Find the command inputs {#discover-exact-inputs}

Use CLI help or the live MCP catalog that VS Code uses. Keep local paths, journal coordinates and approval data in your private request.

```sh
gazette recovery-session --help
gazette observe-release-targets --help
gazette record-release-observation --help
gazette inspect-release-reconciliations --help
```

These commands inspect or record recovery state. Publishing still requires approval from the publisher.

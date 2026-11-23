# Releasing

Release the generated files the publishers approved. The ordinary path prepares the complete output for review, records the publisher’s explicit confirmation of its digest, and pushes each target with that publisher’s selected credential. Gazette retains the exact intent, attempts and independent read-back needed to inspect an interrupted push. Signed authorization remains an optional mode.

GitHub Pages or Mintlify may build and serve a site after the push. Gazette needs no receiver service or scripts on the destination. Verify the push and the served pages as separate steps.

## The chain

| Step | Command or operation | What it leaves behind |
| --- | --- | --- |
| Prepare | `gazette prepare-sites` | For each site: a retained stage, its disclosure and output checks, and a verified local Git commit on an independent deployment worktree |
| Inspect | `gazette inspect-release-preparations` | Confirms the whole prepared set is complete and consistent; certifies nothing |
| Hold a writer | `gazette release-session` (open) | One anchored journal writer at a known generation |
| Review | `release-session` operation `review` | The complete verified set, its publisher scopes, owner credentials and confirmation digest; no approval is granted |
| Confirm | The publisher reviews the output | Explicit confirmation of the exact digest returned for review |
| Register | `release-session` operation `register_reviewed` | The original confirmed bytes, held in custody |
| Consume | `release-session` operation `consume_reviewed` | One intent per target, recorded once; the approval can never be used again |
| Dispatch | `release-session` operation `dispatch_reviewed` | One attempt per target: a durable start, the push, and its result |
| Read back | `gazette observe-release-targets`, `gazette record-release-observation` | What each destination actually holds |
| Settle | `release-session` operation `reconcile` | A settlement per release, appended to its history |

`gazette mcp` offers the same session as `open_release_session`, `operate_release_session` and `close_release_session`.

## Preparing

Pass one private JSON request to `gazette prepare-sites`. It contains the workspace, journal, expected journal header, decisions about which content to include, disclosure rules, candidate check and each site's render and stage settings. Supply required nullable fields explicitly. Use the recipe schema for optional fields.

Gazette checks the complete request before reading sources. It refuses unfiltered, preview and development output. It captures the inputs once, renders the set, then prepares the sites in order. For each site it scans for private values and runs the configured output checker over the bytes to be delivered. A refusal keeps the stage and stops before Git.

Each deploy commit uses a separate worktree and an explicit parent, with no remotes, hooks or linked worktrees. Gazette reads the commit back before reporting it. The publication’s `repository` supplies its Git destination on `main`; an explicit `destination = { kind = "git", remote, branch }` overrides those coordinates. With neither field, the site has no delivery target. `deployable` remains the release eligibility switch; configuring a repository does not grant release approval.

If any site fails, the report marks the set incomplete, lists the sites already prepared and names the failed phase. Previous work stays in place. Inspect it before continuing instead of submitting the same request as a new attempt.

### Set up configured output repositories

`gazette materialize-sites --checkouts /absolute/private/checkouts.json` can prepare each configured output repository before the existing journal-owned output write. MCP `materialize_sites` accepts the same optional `checkouts` object. It must contain exactly the rendered site set, keyed by site ID. The retained workspace supplies each remote and branch, including the `repository` → `main` default; the setup request does not repeat a destination.

Each entry contains `git`, `expected`, `history`, `read` and `options`. `git` selects the absolute native executable, empty private isolation directory and optional explicit credential helper. `expected` selects the local state: `{"state":"missing"}`, `{"state":"unborn"}`, or `{"state":"commit","oid":"…"}`. `history` selects an independently observed remote parent: `{"kind":"acquire","expected":"…","root":"/absolute/private/new-history.git","allow_acquire":true}` fetches into a fresh bare repository; `{"kind":"retained","expected":"…","source":{"kind":"bare","path":"/absolute/private/history.git"}}` uses complete retained history. `{"kind":"unborn"}` requires a qualified native observation of absence; a silent or failed HTTPS advertisement cannot establish that state. `read` carries separate read/network permissions and process bounds. `options` carries separate `allow_initialize`, `allow_update` and repository admission limits. All permissions default to denied.

Acquisition checks the expected branch before fetching, verifies the complete local commit/tree/blob ancestry, and reads the remote ref independently afterward. Native storage is admitted against a size limit after fetching; that limit is not a hard cap on bytes written by the Git process. Process deadlines and captured-output limits remain separate. Failure retains the selected local history path and its possible effects; Gazette never substitutes a root commit or retries acquisition automatically.

Output setup creates missing metadata or adopts only a strictly empty independent repository, preserving an existing `.git` directory’s identity. It records the configured branch, `origin` tracking and the parent’s index without checking remote files out over generated or unrelated files. An existing configured branch must match the selected local commit and can advance only to a verified descendant. Detached preparation worktrees remain separate. Pending materialization history, unsafe configuration, unrelated refs, divergence or a changed root identity refuses setup. If a later site fails, the result retains the completed metadata prefix; generated output is written only after the original complete-set output planner succeeds. Keep the request and result for inspection instead of rerunning a partly completed setup.

### Build a local preparation request

Preparation uses the shared SDK `build::LocalPreparationRecipe`. To discover its exact JSON shape through MCP, find `prepare_sites` in the paginated `tools/list` response. Its `inputSchema.properties.recipe_json.contentSchema` is generated from that recipe's Rust owner. The outer input schema separately describes native permissions and limits. [Tool discovery](mcp.md#discovery-and-acceptance) explains discovery; the declaration-schema CLI does not expose this recipe.

A one-site caller can construct the request below from its own retained inputs. Work in a private directory and supply these files:

| File | Value it supplies |
| --- | --- |
| `workspace.toml` | A complete one-file workspace with absolute configured source locations and the selected site/destination. |
| `expected-header.json` | The exact `{schema_version, journal_id, journal_epoch, writer_fence}` observed for the existing journal; do not predict a successor generation. |
| `coordinator.json` | The independently retained `{anchor_root, binding}` from enrollment; explicit JSON `null` selects an ordinary unguarded writer. Release work uses the enrolled selection described in [Store](store.md#creating-a-journal). |
| `disclosure.json` | The actual private prohibition policy, with all six classes; see [required checks](policy-checks.md#required-preparation-checks). Example or empty rules cannot stand in for those values. |
| `commit-metadata.json` | Explicit `parent`, `author`, `committer` and `message`. Each author/committer has `name`, `email`, nonnegative Unix `seconds` and `offset_minutes`. This example requires `parent: null`; an existing parent also needs a selected native `parent_source`. |
| `preparation-id.txt` | A new, caller-selected 64-character lowercase SHA-256 identity, retained with this request before invoking preparation. Reuse is inspected, never treated as a fresh attempt. |

Save this as `make-preparation.py`, substitute your own absolute journal/stage roots and exact site identity, and run it once to write `preparation.json`:

```python
import json
from pathlib import Path

def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"Duplicate JSON key: {key}")
        result[key] = value
    return result

def read_json(name):
    return json.loads(Path(name).read_text(encoding="utf-8"), object_pairs_hook=unique_object)

metadata = read_json("commit-metadata.json")
if metadata["parent"] is not None:
    raise ValueError("This example selects a root commit; supply a parent source for history")

request = {
    "workspace": {"kind": "supplied", "source": Path("workspace.toml").read_text(encoding="utf-8")},
    "journal": "/absolute/private/journal",
    "expected_header": read_json("expected-header.json"),
    "coordinator": read_json("coordinator.json"),
    "disclosure": read_json("disclosure.json"),
    "candidate_check": None,
    "sites": [{
        "render": {"site": "site:notes", "model_output": "model.json", "graph": None},
        "stage_parent": "/absolute/private/stages",
        "stage_name": "notes-candidate",
        "preparation_id": Path("preparation-id.txt").read_text(encoding="utf-8").strip(),
        "metadata": metadata,
        "parent_source": None,
    }],
}
with Path("preparation.json").open("x", encoding="utf-8") as output:
    json.dump(request, output, ensure_ascii=False)
    output.write("\n")
```

This example supplies one complete TOML buffer. For a modular `workspace.kind = "local"` request, include its complete native `limits` object as shown in the discovered recipe schema; preparation requires the serialized named shape, including those nested defaults.

The stage parent must already exist, be private and be separate from the source, journal and history roots. The workspace must select deployable output and its explicit destination. `candidate_check: null` selects no pre-render checker; it does not create a passing review. A configured output checker still runs over the rendered bytes. To require candidate responses or a qualified output policy, use the [policy-check selections](policy-checks.md#select-once-execute-explicitly) and the exact recipe schema rather than adding an unchecked approval field.

For an enrolled local preparation, the corresponding command is:

```sh
gazette prepare-sites --local --allow-read --create-stage \
  --record-preparation --advance-writer-generation --reserve-local-coordination \
  --write-coordinator-anchor --allow-coordination-files --initialize-worktree \
  < preparation.json > preparation-result.json
```

A configured checker also needs `--allow-policy-execution`. That permission runs its selected command with native effects; it is not a process sandbox. Review `gazette prepare-sites --help` for independent input, output and process limits. This command creates local stages, journal records and Git objects; it neither signs nor pushes. Retain both input and output even when the command fails.

On success, `status` is `local-preparations-complete-not-certified`, and `sites` contains the preparation receipts. `not-started` means preparation did not begin; `candidate-failed` and `render-failed` preserve the earlier checker/render evidence. An `incomplete` report contains `requested`, `completed` and the failed site/phase. Inspect that completed prefix and any later `stage`, `stage_residue`, journal or Git effect coordinates before continuing. A missing response cannot establish which phases ran. [`inspect-release-preparations`](mcp.md#release-tools) reads the exact preparation selection; [recovery](recovery.md) explains interrupted custody. Do not regenerate the request with new IDs to hide uncertain effects.

### Closed candidates

Omit `selection` to include all non-draft content and its required retained assets. Gazette derives one complete selection from the captured corpus before checking or rendering it. Preparation uses this default even when no candidate checker is selected. Ordinary local previews retain their configured draft policy.

To override that default, supply complete `content` and `assets` decision maps over the captured corpus. Each decision is `include` or `exclude`; asset keys use the captured logical path. For example, a corpus containing exactly one page and no assets can select:

```json
{"content":{"content:notes":"include"},"assets":{}}
```

That override is complete only for that corpus. Missing, extra or conflicting decisions, included drafts and relationships to excluded required endpoints refuse the candidate. Selecting content does not approve its release. The [retained review inventory](render-inspection.md#review-exact-retained-surfaces) provides the source surfaces to review; the compiler checks candidate closure before rendering. Preview output and unfiltered local inspection remain separate from a preparation candidate.

## Reviewing and confirming

Open `gazette release-session` before requesting approval. The returned readiness names the actual held journal generation; keep that session open through registration, consumption and dispatch. The CLI accepts one JSON operation per line. MCP exposes the same operations through `operate_release_session`; discover its schema for the complete field and limit definitions.

The open request’s optional `reviewed` selection contains `workspace`, optional `sites`, `credentials`, `allow_read` and supplied-document `limits`. The workspace uses the same `supplied` or `local` shape as preparation. Local configuration capture requires `allow_read: true`. Omitting `sites` selects the workspace’s configured build set; an explicit ordered set is fixed for the lifetime, never reduced to the sites that succeeded. Every selected site must be deployable and have a destination. Each distinct publisher needs one credential selection: `{publisher, owner, credential: {helper, allow_credentials}}`. The helper is an absolute executable path, with separate permission to invoke it for HTTPS. The owner label records the operator’s selection; it does not establish provider-account membership. The ordinary `git` selection supplies the executable and isolation directory with `credentials: null`; reviewed operations route each target through its own publisher’s credential.

Send `review` with the actual `expected` header and an `input` containing a new `record_id`, a one-use `nonce`, explicit `approved_at` time and the complete preparation `selections`. Its `options` supply the existing native preparation and bounded JSON verification limits. Gazette inspects the whole native set, checks its destinations against the held workspace, derives publisher coverage from the site declarations, and returns a `Reviewed` result containing `approval`. This operation neither records approval nor contacts a destination.

The publisher reviews the generated output and the complete returned approval. Its `canonical_body_sha256` covers every field except the digest itself: the held journal, ordered sites and targets, prepared output and evidence, publisher coverage, credential owners/helpers and review time. A changed output, destination, owner or set requires a new review and confirmation.

For `register_reviewed`, `consume_reviewed` and `dispatch_reviewed`, retain the original approval JSON as `input.approval`, supply the separately confirmed digest as `input.confirmed_sha256`, a current `input.observed_at`, and the original full `input.selections`. A document’s own digest does not provide confirmation. Gazette independently reconstructs native target expectations and compares the document with the workspace publisher scopes selected when the session opened. These operations require their existing separate registration, lease, consumption, read and dispatch permissions. No signing key or `authority` command is needed.

## Optional signed authorization

The authorization is a JSON record, `WebsiteReleaseBatchAuthorizationV1`, that names the journal, its generation, the ordered targets and the evidence digests of the prepared set. For each target it names the domain, remote, branch, the single refspec, the commit to push, the output digest and the commit the remote is expected to hold now, or that the branch does not exist yet.

Each publisher signs the SHA-256 digest of the record's RFC 8785 canonical form, without its signatures, prefixed by the domain `WEBSITE_RELEASE_BATCH_AUTHORIZATION_V1` and a zero byte, using Ed25519. Every target is covered by exactly one publisher. Decoding is strict: unknown, duplicate or missing fields, decimals, forced pushes, deletions and wildcard refspecs are all refused. `gazette verify-release-authorization` checks signatures and bindings alone; `gazette verify-prepared-authorization` also checks them against the local preparations. The exact authorization and binding fields are defined by the [native release declarations](../crates/gazette-core/src/release.rs). The verifier accepts their original JSON strings; its MCP schema describes that wrapper, not a generated authorization schema. See [supplied verification](mcp.md#supplied-release-verification). `gazette schema` covers declaration contracts only, as listed in the [Theme reference](theme-reference.md#native-schemas).

Open the release session and acquire its writer before asking for signatures, because the authorization is signed for the generation the writer holds. Keep the same session for register, consume and dispatch.

## Pushing

Consuming reviewed approval rechecks the confirmed digest, workspace scopes and complete native preparations, observes every destination through its selected credential, and compares it with the approved pre-state. The signed mode additionally checks signatures and refreshes publisher keys through its configured authority command. If any target has moved, the whole batch is refused. Otherwise one intent per target is recorded in a single transaction, and the authorization can never be consumed again.

Each dispatch records its start before Git runs, then pushes one exact, non-forced refspec, fenced on the approved pre-state. File and HTTPS remotes are supported; SSH is not. An HTTPS credential comes only from one explicitly named credential helper, with TLS verification on and no redirects. A timeout or a lost response leaves the attempt uncertain. A retry is allowed only after a completed rejection or a refusal before any effect, with the exact pre-state observed again and approval rechecked; signed mode also refreshes its authority.

## Reading back and settling

Read-back asks each destination which commit its branch holds, and settlement compares that with the original approved intent. A release is **succeeded** when every target holds its intended commit, **failed with no effect** when every target still holds its pre-state and nothing was ever uncertain, **partially terminal** when some targets succeeded and the rest conclusively conflict, and otherwise **needs reconciliation**. A terminal settlement closes the release for good. A GitHub Pages build or a served page is a separate observation (`gazette observe-github-pages-build`, `gazette observe-delivery`); neither settles a release.

When the outcome stays uncertain, reconcile from what the remote holds and stop if that does not settle it. Never push again on a guess. Interrupted releases are handled through `gazette recovery-session` ([recovery](./recovery.md)). A reviewed recovery can select the same independent workspace scopes and owner credentials through its optional `reviewed` field. This supplies destination-read routing only; recovery has no registration, consumption or dispatch operation. Original records and missing results remain unchanged.

## Where the history lives

Every step is recorded in the SQLite journal described in [store](./store.md), and each has an `inspect-release-*` command that reads its history without contacting a destination or revalidating a signature. The policy check a release relies on is described in [policy checks](./policy-checks.md).

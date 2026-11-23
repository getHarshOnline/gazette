# Configured policy checks

Run a checker you configure against a [candidate review](releasing.md#closed-candidates) or against the complete rendered files. Gazette runs the command and checks that its response refers to the supplied inputs. Your checker decides what passes. Use [output disclosure checks](output-disclosure.md) for Gazette's built-in string, path, endpoint and format scans.

Call these operations from the Rust application or SDK. A checker result is one input to a release decision. Gazette does not decide the applicable rights, privacy rules or publisher approval.

## Configure and allow a checker {#select-once-execute-explicitly}

The workspace declaration accepts one optional root-level command:

```toml
[policy_check]
program = '/absolute/path/to/checker'
directory = '/absolute/path/to/workspace'
arguments = []
environment = { MODE = 'candidate-review' }
```

The executable and directory can use absolute native paths, workspace-relative paths or the existing machine roots. Mark path arguments explicitly as `{ path = 'machine:checker/check.py' }`; string arguments remain literal values. [Machine-bound program binding](runtime-commands.md) explains the shared math/checker declaration and the captured machine file. Native execution receives absolute program and directory paths. Gazette neither searches `PATH` nor inherits the parent environment; omitted arguments/environment are empty. Environment names use ASCII letters, digits and underscores, without a leading digit. Keep secrets out of configuration. Command text rejects NULs and is limited to 64 KiB across both paths, all arguments and environment keys/values; at most 256 arguments and 64 environment entries are supported.

Parsing this configuration performs no filesystem access or execution. A missing checker does not disable ordinary standalone local publication work. An explicit request to run a checker with none configured returns `policy_unconfigured`, never an invented successful check.

An independently selected executable can also be pinned in that same command:

```toml
[policy_check.executable]
bytes = 12345 # Exact file length, not an input/output budget.
sha256 = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef'
```

Replace these illustrative values with the approved executable's exact length and SHA-256. Pinning is optional for ordinary local execution; it is never inferred from the file being checked. A selected pin requires a nonempty regular file of at most 256 MiB. The native runner opens it without following the final symlink, checks its complete bytes and file/path identity before spawning, retains that handle, and rechecks it after the child is reaped before returning success. The same guard applies to bounded stdout capture. A mismatch before spawn returns `process_executable` with `not-started` custody; a failed final check returns `process_executable_changed` with `reaped` custody and no captured stdout. Permission refusal still precedes executable reads. Original transport failures retain their original failure/custody evidence; they do not establish a verified executable lifetime or authorize retry.

These paired observations require trusted local namespace control. Native spawn still uses a path: this is not protection against a hostile replace-and-restore race, executable-image attestation or a sandbox. Pinning a script does not pin its interpreter; pinning an interpreter does not pin scripts, libraries, imports, policy sources or remote inputs. Those remain separately selected dependencies.

Rust callers use `build::check_policy(&selected, process::Options, ReviewLimits)`. `allow_execute` defaults to false and must be acknowledged for every call. Review the [trusted-code model](architecture.md#trusted-code-and-saved-results) before enabling it. Gazette selects the command from the checked object's own retained workspace, not from a second SDK/CLI command declaration. Permission and option validation precede projection or process creation.

## Read the checker input and process limits {#exact-input-and-bounded-transport}

Gazette writes a compact `gazette-candidate-policy/1` JSON envelope to stdin, then closes stdin. Its `review` is the complete `{sha256, request}`. Its `response` is either null or the original matched response. The invocation digest covers the whole envelope. A one-site check and a set check serialize it the same way. The program's stdout cannot add meaning to the source.

The checker must understand this input. Gazette does not infer rules from the executable or add arguments.

| Operation | What Gazette reads or limits |
| --- | --- |
| Exit-only check | Discards stdout and stderr. Uses the process result. |
| Candidate or output response check | Captures stdout up to the configured limit and validates the response. Discards stderr. |
| Complete stdin | 16 MiB by default, with a supported maximum of 64 MiB. Review creation has its own limits. Oversized input fails without sending a prefix. |
| Write stdin and wait for the child | One deadline after spawn, 30 seconds by default. Accepted range is 1 millisecond to 5 minutes. |
| After failure | Attempts to terminate the direct child and allows another 100 milliseconds to reap it. |

The adapter runs on Unix. Other hosts refuse before spawn. Gazette does not promise that process creation can be interrupted. The result tells whether the child never started, was reaped or may still be running. If inspection fails, do not signal an unverified PID. An embedding application must leave child reaping and status handling to Gazette.

The configured program can read files, use the network or launch other processes. Its directory, cleared environment and timeout do not sandbox it. A failed run does not establish that descendants stopped or that no effects occurred. Inspect before retrying.

### Checker-originated candidate response

The [local preparation recipe](releasing.md#build-a-local-preparation-request) can select `candidate_policies` for exactly every requested site. Each value is a `CandidatePolicyRequirement`: `{policy: {id, revision, source_sha256}, command_sha256}`. These are independent expectations, not values learned from stdout. This mode requires `candidate_check`, the configured checker with an executable pin, and the retained `policy_source` described below. It cannot be combined with imported `review_responses` and never falls back to exit-only checking.

Before the first checker runs, Gazette constructs every selected review and bounded request, matches the complete command digest and verifies the selected policy's actual captured bytes. The candidate envelope carries `response: null`, the independent `requirement` and exact `policy_source`. Invocations then run in stable site-key order, before any Theme or resource compilation. Policy bytes remain opaque to Gazette; their presence and digest do not establish policy-owner authority.

The checker must return the existing strict [`gazette-review-response/1` response](render-inspection.md#execute-and-inspect) on stdout. Its `request_sha256` binds the inner review's exact digest; the invocation digest separately binds the entire stdin envelope. Normal exit zero and complete matching against the independently selected policy and every review occurrence are required. Any hold/exclude decision, malformed or incomplete response, changed binding or exhausted bound refuses the candidate phase without filtering or rendering. Per-response byte/decision limits and aggregate response-byte limits remain separate from request limits.

The private check retains the original matched stdout, its observed digest, review, requirement and policy source. Unlike an imported response, that stdout digest is an observation of the actual capture, not an independently supplied expectation. If a later site's candidate check fails, `completed_responses` preserves earlier successful sites' original matched responses alongside their completed invocations. The shared failure report retains the failing capture's digest, byte count and exit/custody evidence without exposing its raw stdout. It does not authorize retry.

Historical inspection independently reconstructs the original candidate envelope with `response: null`, verifies its invocation digest and retained policy file/tree binding, and reapplies the shared response matcher using the retained independent policy requirement. It neither reruns the checker nor substitutes current local policy bytes. `matched-unverified` and all-allow decisions establish no evaluator authenticity, evidence validity, freshness, rights or publication approval.

## Check the actual rendered bytes

Select an opaque policy document through the existing pinned source declarations:

```toml
[policy_source]
source = 'source:publication-policy'
entry = 'policy.json'
```

The source key must be declared and a checker must be selected. Publication acquisition captures this source as a shared required input, not a Theme manifest or authored document. Its regular-file bytes must be nonempty and at most 1 MiB; the complete checker request retains its separate transport ceiling. Gazette does not interpret the document's policy language. A missing file, exhausted bound or mismatch with an independently required policy digest returns `policy_source` before checker execution and before staging in local preparation. There is no live reread or fallback after capture.

`build::check_output_policy(&rendered, process::Options, requirement)` checks the complete immutable `PublicationRender`, independently of any earlier candidate check. The same native execution owner is used; no source, Theme or generated directory is reopened, and no rerender or filesystem export occurs.

Its compact JSON stdin has the following transport fields:

| Field | Retained value |
| --- | --- |
| `format` | `gazette-output-policy/1`, distinct from candidate-review input |
| `repository_source_provenance` | Embedded repository source provenance, not executable, feature or toolchain attestation |
| `mode` | `local`; this request never represents an already certified publication |
| `acquisition` | Existing site/configuration/source inspection evidence |
| `selection` | Complete candidate intent, including exclusions, or `null` for unfiltered output |
| `candidate_check` | The retained preceding check, including its invocation, exact review and optional original matched response, or `null`; never approval |
| `disclosure` | Exact required disclosure report during preparation checking; absent in standalone diagnostic output checking |
| `preview_urls` | Exact preview URL projection, or `null` when absent |
| `render` | Existing Theme, page, asset and resource render evidence |
| `inventory` | Complete output inventory, including directories and structural identity |
| `media_types` | Producer-declared formats, including extensionless output paths |
| `files_base64` | Every regular-file path and its exact bytes in standard padded Base64 |
| `policy_source` | When selected: exact `{binding: {source, entry}, tree, sha256, document_base64}` from retained acquisition |

Media declarations use `html`, `json`, `json-ld`, `xml`, `rss`, `atom`, `text`, `javascript`, `css`, `png` and `jpeg`. These identify producer representations, not HTTP headers or guessed file extensions; transport adapters retain their own MIME projection.

Binary and hidden files are not filtered, guessed as UTF-8 or replaced by hashes. Payload encoding streams through the bounded JSON writer without an intermediate encoded copy of each file. The stdin ceiling covers the entire wire representation, including Base64 expansion and provenance; exceeding it prevents process creation. The request is an Engine transport, not a schema for any owner's policy judgments.

### Stream complete output without a Base64 copy

Select `policy_output_transport = "framed-v2"` at the workspace root when the checker implements this protocol. Omission or `"inline-v1"` preserves the JSON envelope above. Candidate-review input remains JSON. The same SDK and preparation operations honor the selected output transport; an embedder without segmented native capture refuses it instead of falling back.

The stream contains, in order:

1. The ASCII bytes `gazette-output-policy/2\n`, ending with one line-feed byte.
2. An unsigned 64-bit big-endian metadata length, then exactly that many UTF-8 JSON bytes.
3. For each file in `inventory.entries` order, an unsigned 64-bit big-endian file length followed by its exact raw bytes. Directories have no frame.
4. EOF immediately after the final file.

The header retains the fields above, uses `format: "gazette-output-policy/2"` and omits `files_base64` entirely. Every directory and file remains in the inventory. Entries have unique, lexical UTF-8 relative paths with explicit directory parents. A directory is `{path, kind: "directory"}`; a file is `{path, kind: "file", bytes, sha256}`. The inventory also carries `total_bytes` and its `gazette.inventory.v1` identity. [The native inventory identity](../crates/gazette-core/src/source/tree_identity.rs) defines its length-framed encoding; neither filesystem paths nor timestamps enter it.

The checker must reject duplicate or invalid inventory entries, a wrong inventory identity or total, missing or extra frames, mismatched file lengths or SHA-256 values, truncation and trailing bytes. It must finish checking every member and EOF before returning an allow response. `gazette-output-response/1` remains the response format; `request_sha256` hashes every transmitted byte, including the magic, lengths, metadata and file payloads, rather than the header alone.

The metadata retains the caller's `max_input_bytes` ceiling: 16 MiB by default and at most 64 MiB. The framed profile separately supports at most 10,000 inventory entries, 64 MiB per file and 512 MiB of aggregate raw file bytes, matching the native source defaults. Framing adds eight bytes per file plus the magic and metadata-length prefix. The producer retains a bounded metadata buffer and borrowed file segments; it does not allocate the concatenated request or another Base64 copy. A checker can preserve whole-file analysis while retaining one bounded file at a time. These are explicit logical budgets, not a process-wide memory guarantee.

Input delivery, selected output capture, EOF and child exit share the existing deadline. Permission, command and executable-pin checks remain mandatory, including pin verification after execution. No publication tree is exported or reopened, and no source or output member is excluded to meet a limit.

A normal zero exit is necessary for `OutputPolicyCheck`. With no requirement, this is exit-only evidence and `response` is null. Its receipt contains `output-policy-check-passed-unverified`, invocation digests and the structural output identity. `candidate_check_sha256` binds the exact private candidate-check object delivered to the output checker, or is null when absent. During preparation checking it also includes `disclosure_sha256`, the digest of the exact serialized report delivered in the request. Standalone diagnostic checking has no disclosure report or disclosure digest. Rust callers can borrow the exact checked render through `rendered()`; serialized receipts contain neither that render nor its file payloads. Changing any delivered output, selection or evidence changes the request identity. Per-site checks do not settle a multi-site release set.

### Independently bound output response

`Some(&OutputPolicyRequirement)` additionally requires an explicitly pinned executable, the exact compact-JSON command digest, an independently selected `ReviewPolicy` (`id`, `revision`, `source_sha256`) and a complete stdout allowance `response_bytes` from 1 byte through 64 MiB. Command mismatch refuses before staging in local preparation. This mode also requires `policy_source`; Gazette hashes the actual retained file and matches `ReviewPolicy.source_sha256` before execution. A missing checker or policy source never falls back to exit-only mode. The pin covers the executable, not interpreter scripts, imports or policy-source authenticity; their qualification remains with the selecting owner.

The checker must return one closed, named JSON object:

```json
{
  "format": "gazette-output-response/1",
  "request_sha256": "<SHA-256 of exact stdin bytes>",
  "policy": {"id": "policy:site", "revision": "revision:selected", "source_sha256": "<policy-source SHA-256>"},
  "outcome": "allow",
  "evidence": ["evidence:policy-check"]
}
```

Policy identities, revision identifiers and evidence references are complete `namespace:local` keys. Revisions are strings, not numbers.

Expected response coordinates are not added to stdin for the checker to echo. The actual selected document is delivered; its own content may declare policy identity or revision, which the checker must interpret and evaluate independently. Early candidate checking is optional and its policy is never copied into this requirement. Gazette matches the actual output-request digest and the checker's reported policy against the independent requirement. Evidence references must be nonempty and unique; their existence or validity is not inferred. Hold, exclude, nonzero exit, malformed/duplicate/positional JSON and output overflow fail without modifying the rendered tree or proceeding to Git preparation.

The private `response` retains the exact original stdout text, its digest and the requirement under `checker-reported-policy-matched-unverified`. The same matcher verifies retained preparation history. Native failures retain the original capture and invocation; CLI/MCP diagnostics report its digest, byte count, exit and direct-child custody without exposing raw private stdout. Failure after staging preserves that stage's original receipt and never retries the checker. Complete capture is bounded by the native execution deadline; an overflow or incomplete pipe yields no purported complete response.

The private output-check evidence retains the selected policy document once, or an explicit null when none was selected. Historical inspection rechecks its canonical Base64, actual digest and exact file/tree coordinates against the original preparation's source inventory before matching the recorded response. Neither a new local file nor a different historical expectation rewrites that retained input. Source integrity is not source-owner authentication or approval.

This is matched checker-reported evidence, **not authenticated policy approval**. The delivered request binds output, provenance and disclosure, but does not independently qualify the Engine binary, scanner, checker dependencies or policy owner. Those acceptance obligations remain separate.

## Required preparation checks

`build::check_preparation(&rendered, &disclosure_policy, process_options, disclosure_limits, requirement)` joins the required disclosure scan and optional workspace checker over one immutable selected render. It first rejects preview or missing publication selection, validates the required policy and performs its bounded scan. Findings, unsupported decoding or exhausted scan bounds prevent checker execution. When a checker is configured, it receives that same report together with the complete output request above; the existing request digest covers the report as well as the output and provenance.

The returned `PreparationChecks` privately retains the exact report and optional `OutputPolicyCheck`. Its `disclosure()` and `policy()` accessors expose those actual results, not replaceable inputs. A generic workspace with no checker retains the required scan with `policy() == None`; it does not manufacture a passing invocation. An arbitrary diagnostic report cannot be supplied in place of the required scan, and a standalone `OutputPolicyCheck` cannot be promoted into this combined result.

`build::record_preparation_check(&checks, request, options)` records the retained results against the original staging intent. It neither rescans output nor invokes the checker. The durable disclosure bytes must match the report delivered to the checker, and the stored invocation binds their exact digest. A changed policy requires a new scan and, when configured, a new checker invocation; recording cannot append a new scan to an old invocation. Immutable-history conflicts remain explicit.

## Read the checker result {#what-the-results-prove}

A normal zero exit after complete stdin delivery returns a `PolicyCheck` with `policy-check-passed-unverified`. The private result contains the request and command digests, the review and any matched response. Nonzero or abnormal exit returns `policy_rejected` with the same invocation coordinates. A transport failure records its phase and whether the child was reaped. A response-producing candidate check also requires a complete all-allow match. When the exit or response fails, Gazette keeps the capture and invocation for inspection.

The command digest covers the bound native paths and arguments, literal arguments, environment and the executable pin when present. Independent candidate/output requirements compare this resolved native command, never the portable declaration; a machine relocation can therefore require a new independently selected command digest. A successful pinned check records the executable byte checks before and after the process. It does not verify dependencies or what the program did with stdin. An output check binds the rendered bytes and, during preparation, the required disclosure report. A passing invocation cannot supply rights, approval, a certificate or a release token.

For a custom checker, test an allowed result, an explicit rejection, malformed output and a process timeout. Gazette's process and publication tests cover the transport; your checker needs examples that express your publication's actual rules.

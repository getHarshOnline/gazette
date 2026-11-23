CREATE TABLE gazette_journal (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    journal_id BLOB NOT NULL CHECK (length(journal_id) = 32),
    journal_epoch INTEGER NOT NULL CHECK (journal_epoch > 0),
    writer_fence INTEGER NOT NULL CHECK (writer_fence >= 0)
) STRICT;

CREATE TABLE gazette_ref_observations (
    id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 32),
    payload BLOB NOT NULL CHECK (length(payload) > 0),
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32)
) STRICT;

CREATE TABLE gazette_local_commit_checkpoints (
    id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 32),
    repository TEXT NOT NULL CHECK (length(repository) > 0),
    commit_oid TEXT NOT NULL,
    output_sha256 BLOB NOT NULL CHECK (length(output_sha256) = 32),
    parent_oid TEXT,
    entries INTEGER NOT NULL CHECK (entries >= 0),
    bytes INTEGER NOT NULL CHECK (bytes >= 0),
    author_name TEXT NOT NULL,
    author_email TEXT NOT NULL,
    author_seconds INTEGER NOT NULL CHECK (author_seconds >= 0),
    author_offset INTEGER NOT NULL CHECK (author_offset BETWEEN -1439 AND 1439),
    committer_name TEXT NOT NULL,
    committer_email TEXT NOT NULL,
    committer_seconds INTEGER NOT NULL CHECK (committer_seconds >= 0),
    committer_offset INTEGER NOT NULL CHECK (committer_offset BETWEEN -1439 AND 1439),
    message TEXT NOT NULL CHECK (length(message) > 0),
    created_schema INTEGER NOT NULL CHECK (created_schema > 0),
    created_journal BLOB NOT NULL CHECK (length(created_journal) = 32),
    created_epoch INTEGER NOT NULL CHECK (created_epoch > 0),
    created_fence INTEGER NOT NULL CHECK (created_fence > 0)
) STRICT;

CREATE TABLE gazette_preparation_intents (
    id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 32),
    parent TEXT NOT NULL CHECK (length(parent) > 0),
    name TEXT NOT NULL CHECK (length(name) > 0),
    output_sha256 BLOB NOT NULL CHECK (length(output_sha256) = 32),
    entries INTEGER NOT NULL CHECK (entries >= 0),
    bytes INTEGER NOT NULL CHECK (bytes >= 0),
    evidence TEXT NOT NULL CHECK (length(evidence) > 0),
    evidence_sha256 BLOB NOT NULL CHECK (length(evidence_sha256) = 32),
    created_schema INTEGER NOT NULL CHECK (created_schema > 0),
    created_journal BLOB NOT NULL CHECK (length(created_journal) = 32),
    created_epoch INTEGER NOT NULL CHECK (created_epoch > 0),
    created_fence INTEGER NOT NULL CHECK (created_fence > 0),
    destination TEXT CHECK (destination IS NULL OR length(destination) > 0)
) STRICT;

CREATE TABLE gazette_preparation_retentions (
    intent_id BLOB PRIMARY KEY NOT NULL REFERENCES gazette_preparation_intents(id),
    output_sha256 BLOB NOT NULL CHECK (length(output_sha256) = 32),
    entries INTEGER NOT NULL CHECK (entries >= 0),
    bytes INTEGER NOT NULL CHECK (bytes >= 0),
    recorded_schema INTEGER NOT NULL CHECK (recorded_schema > 0),
    recorded_journal BLOB NOT NULL CHECK (length(recorded_journal) = 32),
    recorded_epoch INTEGER NOT NULL CHECK (recorded_epoch > 0),
    recorded_fence INTEGER NOT NULL CHECK (recorded_fence > 0)
) STRICT;

CREATE TABLE gazette_preparation_checks (
    intent_id BLOB PRIMARY KEY NOT NULL REFERENCES gazette_preparation_retentions(intent_id),
    evidence TEXT NOT NULL CHECK (length(evidence) > 0),
    evidence_sha256 BLOB NOT NULL CHECK (length(evidence_sha256) = 32),
    recorded_schema INTEGER NOT NULL CHECK (recorded_schema > 0),
    recorded_journal BLOB NOT NULL CHECK (length(recorded_journal) = 32),
    recorded_epoch INTEGER NOT NULL CHECK (recorded_epoch > 0),
    recorded_fence INTEGER NOT NULL CHECK (recorded_fence > 0),
    UNIQUE (intent_id, evidence_sha256)
) STRICT;
CREATE TABLE gazette_preparation_worktrees (
    intent_id BLOB PRIMARY KEY NOT NULL REFERENCES gazette_preparation_retentions(intent_id),
    metadata TEXT NOT NULL CHECK (length(metadata) > 0),
    parent_source TEXT CHECK (parent_source IS NULL OR length(parent_source) > 0),
    check_sha256 BLOB CHECK (check_sha256 IS NULL OR length(check_sha256) = 32),
    created_schema INTEGER NOT NULL CHECK (created_schema > 0),
    created_journal BLOB NOT NULL CHECK (length(created_journal) = 32),
    created_epoch INTEGER NOT NULL CHECK (created_epoch > 0),
    created_fence INTEGER NOT NULL CHECK (created_fence > 0),
    FOREIGN KEY (intent_id, check_sha256) REFERENCES gazette_preparation_checks(intent_id, evidence_sha256)
) STRICT;

CREATE TABLE gazette_preparation_commits (
    intent_id BLOB PRIMARY KEY NOT NULL REFERENCES gazette_preparation_worktrees(intent_id),
    commit_oid TEXT NOT NULL CHECK (length(commit_oid) = 40),
    output_sha256 BLOB NOT NULL CHECK (length(output_sha256) = 32),
    parent_oid TEXT CHECK (parent_oid IS NULL OR length(parent_oid) = 40),
    entries INTEGER NOT NULL CHECK (entries >= 0),
    bytes INTEGER NOT NULL CHECK (bytes >= 0),
    recorded_schema INTEGER NOT NULL CHECK (recorded_schema > 0),
    recorded_journal BLOB NOT NULL CHECK (length(recorded_journal) = 32),
    recorded_epoch INTEGER NOT NULL CHECK (recorded_epoch > 0),
    recorded_fence INTEGER NOT NULL CHECK (recorded_fence > 0)
) STRICT;

CREATE TABLE gazette_release_authorizations (
    record_id TEXT PRIMARY KEY NOT NULL CHECK (length(record_id) > 0),
    coordinator_id TEXT NOT NULL CHECK (length(coordinator_id) > 0),
    nonce TEXT NOT NULL CHECK (length(nonce) > 0),
    payload BLOB NOT NULL CHECK (length(payload) > 0),
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32),
    UNIQUE (coordinator_id, nonce)
) STRICT;

CREATE TABLE gazette_release_authorization_preparations (
    record_id TEXT NOT NULL REFERENCES gazette_release_authorizations(record_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    preparation_id BLOB NOT NULL REFERENCES gazette_preparation_commits(intent_id) CHECK (length(preparation_id) = 32),
    PRIMARY KEY (record_id, ordinal),
    UNIQUE (record_id, preparation_id)
) STRICT;

CREATE TABLE gazette_release_lease_sets (
    authorization_id TEXT PRIMARY KEY NOT NULL REFERENCES gazette_release_authorizations(record_id),
    payload BLOB NOT NULL CHECK (length(payload) > 0),
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32)
) STRICT;

CREATE TABLE gazette_release_lease_history (
    remote TEXT NOT NULL CHECK (length(remote) > 0),
    branch TEXT NOT NULL CHECK (length(branch) > 0),
    generation INTEGER NOT NULL CHECK (generation > 0),
    authorization_id TEXT NOT NULL REFERENCES gazette_release_lease_sets(authorization_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    PRIMARY KEY (remote, branch, generation),
    UNIQUE (authorization_id, ordinal),
    UNIQUE (authorization_id, ordinal, remote, branch, generation),
    FOREIGN KEY (authorization_id, ordinal) REFERENCES gazette_release_authorization_preparations(record_id, ordinal)
) STRICT;

CREATE TABLE gazette_release_consumptions (
    authorization_id TEXT PRIMARY KEY NOT NULL REFERENCES gazette_release_authorizations(record_id),
    payload BLOB NOT NULL CHECK (length(payload) > 0),
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32)
) STRICT;

CREATE TABLE gazette_release_target_intents (
    authorization_id TEXT NOT NULL REFERENCES gazette_release_consumptions(authorization_id),
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    remote TEXT NOT NULL,
    branch TEXT NOT NULL,
    lease_generation INTEGER NOT NULL CHECK (lease_generation > 0),
    payload BLOB NOT NULL CHECK (length(payload) > 0),
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32),
    PRIMARY KEY (authorization_id, ordinal),
    FOREIGN KEY (authorization_id, ordinal, remote, branch, lease_generation) REFERENCES gazette_release_lease_history(authorization_id, ordinal, remote, branch, generation)
) STRICT;

CREATE TABLE gazette_release_attempt_starts (
    authorization_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    number INTEGER NOT NULL CHECK (number > 0),
    payload BLOB NOT NULL CHECK (length(payload) > 0),
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32),
    PRIMARY KEY (authorization_id, ordinal, number),
    FOREIGN KEY (authorization_id, ordinal) REFERENCES gazette_release_target_intents(authorization_id, ordinal)
) STRICT;

CREATE TABLE gazette_release_attempt_results (
    authorization_id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    number INTEGER NOT NULL CHECK (number > 0),
    payload BLOB NOT NULL CHECK (length(payload) > 0),
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32),
    PRIMARY KEY (authorization_id, ordinal, number),
    FOREIGN KEY (authorization_id, ordinal, number) REFERENCES gazette_release_attempt_starts(authorization_id, ordinal, number)
) STRICT;

CREATE TABLE gazette_release_reconciliations (
    authorization_id TEXT NOT NULL REFERENCES gazette_release_consumptions(authorization_id),
    number INTEGER NOT NULL CHECK (number > 0),
    previous_sha256 BLOB CHECK (previous_sha256 IS NULL OR length(previous_sha256) = 32),
    payload BLOB NOT NULL CHECK (length(payload) > 0),
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32),
    PRIMARY KEY (authorization_id, number),
    UNIQUE (authorization_id, payload_sha256),
    UNIQUE (authorization_id, previous_sha256),
    CHECK ((number = 1 AND previous_sha256 IS NULL) OR (number > 1 AND previous_sha256 IS NOT NULL)),
    FOREIGN KEY (authorization_id, previous_sha256) REFERENCES gazette_release_reconciliations(authorization_id, payload_sha256)
) STRICT;

CREATE TABLE gazette_registry_head (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    semantic_sha256 BLOB NOT NULL CHECK (length(semantic_sha256) = 32),
    configuration_sha256 BLOB NOT NULL CHECK (length(configuration_sha256) = 32),
    rows_sha256 BLOB NOT NULL CHECK (length(rows_sha256) = 32),
    rows INTEGER NOT NULL CHECK (rows >= 0)
) STRICT;

CREATE TABLE gazette_registry_principals (
    id TEXT PRIMARY KEY NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('person', 'organization')),
    name TEXT
) STRICT;

CREATE TABLE gazette_registry_principal_websites (
    principal TEXT PRIMARY KEY NOT NULL REFERENCES gazette_registry_principals(id) DEFERRABLE INITIALLY DEFERRED,
    kind TEXT NOT NULL CHECK (kind IN ('site', 'external')),
    site TEXT REFERENCES gazette_registry_sites(id) DEFERRABLE INITIALLY DEFERRED,
    url TEXT,
    CHECK ((kind = 'site' AND site IS NOT NULL AND url IS NULL) OR (kind = 'external' AND site IS NULL AND url IS NOT NULL))
) STRICT;

CREATE TABLE gazette_registry_principal_same_as (
    principal TEXT NOT NULL REFERENCES gazette_registry_principals(id) DEFERRABLE INITIALLY DEFERRED,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    url TEXT NOT NULL CHECK (length(url) > 0),
    PRIMARY KEY (principal, ordinal),
    UNIQUE (principal, url)
) STRICT;

CREATE TABLE gazette_registry_projects (
    id TEXT PRIMARY KEY NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('planned', 'developing', 'active', 'paused', 'dormant', 'historical', 'retired')),
    visibility TEXT NOT NULL CHECK (visibility IN ('public', 'unlisted', 'private')),
    availability TEXT NOT NULL CHECK (availability IN ('not-applicable', 'unavailable', 'gated', 'available')),
    name TEXT,
    repository TEXT,
    license TEXT
) STRICT;

CREATE TABLE gazette_registry_project_resources (
    project TEXT PRIMARY KEY NOT NULL REFERENCES gazette_registry_projects(id) DEFERRABLE INITIALLY DEFERRED,
    learn TEXT,
    github TEXT,
    blogs TEXT,
    thoughts TEXT,
    mentions TEXT,
    code_documentation TEXT CHECK (length(code_documentation) > 0)
) STRICT;

CREATE TABLE gazette_registry_project_origins (
    project TEXT PRIMARY KEY NOT NULL REFERENCES gazette_registry_projects(id) DEFERRABLE INITIALLY DEFERRED,
    kind TEXT NOT NULL CHECK (kind IN ('original', 'successor-fork', 'convenience-mirror')),
    upstream TEXT REFERENCES gazette_registry_projects(id) DEFERRABLE INITIALLY DEFERRED,
    CHECK ((kind = 'original' AND upstream IS NULL) OR (kind IN ('successor-fork', 'convenience-mirror') AND upstream IS NOT NULL)),
    CHECK (upstream IS NULL OR upstream <> project)
) STRICT;

CREATE TABLE gazette_registry_project_discussions (
    project TEXT PRIMARY KEY NOT NULL REFERENCES gazette_registry_projects(id) DEFERRABLE INITIALLY DEFERRED,
    capability TEXT NOT NULL CHECK (length(capability) > 0),
    repository_id TEXT NOT NULL CHECK (length(repository_id) BETWEEN 1 AND 256),
    category_id TEXT NOT NULL CHECK (length(category_id) BETWEEN 1 AND 256),
    options_json TEXT NOT NULL CHECK (json_valid(options_json) AND json_type(options_json) = 'object')
) STRICT;

CREATE TABLE gazette_registry_brands (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    parent TEXT REFERENCES gazette_registry_brands(id) DEFERRABLE INITIALLY DEFERRED
        CHECK (parent IS NULL OR parent <> id)
) STRICT;

CREATE TABLE gazette_registry_sites (
    id TEXT PRIMARY KEY NOT NULL,
    publisher TEXT NOT NULL REFERENCES gazette_registry_principals(id) DEFERRABLE INITIALLY DEFERRED,
    rights_holder TEXT REFERENCES gazette_registry_principals(id) DEFERRABLE INITIALLY DEFERRED,
    default_author TEXT REFERENCES gazette_registry_principals(id) DEFERRABLE INITIALLY DEFERRED,
    deployable INTEGER NOT NULL CHECK (deployable IN (0, 1)),
    brand TEXT REFERENCES gazette_registry_brands(id) DEFERRABLE INITIALLY DEFERRED,
    public_base_url TEXT,
    repository TEXT
) STRICT;

CREATE TABLE gazette_registry_site_discussions (
    site TEXT PRIMARY KEY NOT NULL REFERENCES gazette_registry_sites(id) DEFERRABLE INITIALLY DEFERRED,
    capability TEXT NOT NULL CHECK (length(capability) > 0),
    repository_id TEXT NOT NULL CHECK (length(repository_id) BETWEEN 1 AND 256),
    category_id TEXT NOT NULL CHECK (length(category_id) BETWEEN 1 AND 256),
    options_json TEXT NOT NULL CHECK (json_valid(options_json) AND json_type(options_json) = 'object')
) STRICT;

CREATE TABLE gazette_registry_site_roles (
    site TEXT NOT NULL REFERENCES gazette_registry_sites(id) DEFERRABLE INITIALLY DEFERRED,
    principal TEXT NOT NULL REFERENCES gazette_registry_principals(id) DEFERRABLE INITIALLY DEFERRED,
    role TEXT NOT NULL CHECK (role IN ('canon-authority', 'data-controller', 'site-operator', 'data-processor')),
    PRIMARY KEY (site, principal, role)
) STRICT;

CREATE TABLE gazette_registry_placements (
    site TEXT NOT NULL REFERENCES gazette_registry_sites(id) DEFERRABLE INITIALLY DEFERRED,
    project TEXT NOT NULL REFERENCES gazette_registry_projects(id) DEFERRABLE INITIALLY DEFERRED,
    role TEXT NOT NULL CHECK (role IN ('primary', 'mirror')),
    render INTEGER NOT NULL CHECK (render IN (0, 1)),
    discoverable INTEGER NOT NULL CHECK (discoverable IN (0, 1)),
    PRIMARY KEY (site, project)
) STRICT;

CREATE TABLE gazette_registry_taxonomies (
    id TEXT PRIMARY KEY NOT NULL,
    label TEXT NOT NULL,
    language TEXT
) STRICT;

CREATE TABLE gazette_registry_terms (
    taxonomy TEXT NOT NULL REFERENCES gazette_registry_taxonomies(id) DEFERRABLE INITIALLY DEFERRED,
    id TEXT NOT NULL,
    label TEXT NOT NULL,
    PRIMARY KEY (taxonomy, id)
) STRICT;

CREATE TABLE gazette_registry_extensions (
    id TEXT PRIMARY KEY NOT NULL,
    schema_json TEXT NOT NULL CHECK (json_valid(schema_json))
) STRICT;

CREATE TABLE gazette_registry_content (
    id TEXT PRIMARY KEY NOT NULL,
    site TEXT NOT NULL REFERENCES gazette_registry_sites(id) DEFERRABLE INITIALLY DEFERRED,
    title TEXT NOT NULL,
    description TEXT,
    license TEXT,
    published TEXT,
    modified TEXT,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('draft', 'published', 'archived')),
    language TEXT,
    kind TEXT NOT NULL CHECK (kind IN ('page', 'collection', 'journey')),
    page_family TEXT CHECK (page_family IN ('article', 'documentation', 'project', 'index', 'utility', 'extension')),
    extension_schema TEXT REFERENCES gazette_registry_extensions(id) DEFERRABLE INITIALLY DEFERRED,
    extension_data TEXT CHECK (extension_data IS NULL OR json_valid(extension_data)),
    collection_parent TEXT REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    journey_collection TEXT REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    document_sha256 BLOB NOT NULL CHECK (length(document_sha256) = 32),
    declared_path TEXT,
    CHECK ((kind = 'page' AND page_family IS NOT NULL AND collection_parent IS NULL AND journey_collection IS NULL)
        OR (kind = 'collection' AND page_family IS NULL AND extension_schema IS NULL AND journey_collection IS NULL)
        OR (kind = 'journey' AND page_family IS NULL AND extension_schema IS NULL AND collection_parent IS NULL)),
    CHECK ((page_family = 'extension' AND extension_schema IS NOT NULL AND extension_data IS NOT NULL)
        OR (page_family IS NOT 'extension' AND extension_schema IS NULL AND extension_data IS NULL))
) STRICT;

CREATE TABLE gazette_registry_declared_aliases (
    content TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    position INTEGER NOT NULL CHECK (position >= 0),
    path TEXT NOT NULL,
    PRIMARY KEY (content, position),
    UNIQUE (content, path)
) STRICT;

CREATE TABLE gazette_registry_attributions (
    content TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    position INTEGER NOT NULL CHECK (position >= 0),
    principal TEXT NOT NULL REFERENCES gazette_registry_principals(id) DEFERRABLE INITIALLY DEFERRED,
    role TEXT NOT NULL CHECK (role IN ('author', 'rights-holder')),
    PRIMARY KEY (content, position),
    UNIQUE (content, principal, role)
) STRICT;

CREATE TABLE gazette_registry_classifications (
    content TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    taxonomy TEXT NOT NULL,
    term TEXT NOT NULL,
    PRIMARY KEY (content, taxonomy, term),
    FOREIGN KEY (taxonomy, term) REFERENCES gazette_registry_terms(taxonomy, id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE gazette_registry_memberships (
    content TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    collection TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    PRIMARY KEY (content, collection)
) STRICT;

CREATE TABLE gazette_registry_journeys (
    journey TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    position INTEGER NOT NULL CHECK (position >= 0),
    content TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    primary_member INTEGER NOT NULL CHECK (primary_member IN (0, 1)),
    PRIMARY KEY (journey, position),
    UNIQUE (journey, content)
) STRICT;

CREATE UNIQUE INDEX gazette_registry_primary_journey ON gazette_registry_journeys(content) WHERE primary_member = 1;

CREATE TABLE gazette_registry_related (
    source TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    target TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    PRIMARY KEY (source, target)
) STRICT;

CREATE TABLE gazette_registry_project_declarations (
    content TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    project TEXT NOT NULL REFERENCES gazette_registry_projects(id) DEFERRABLE INITIALLY DEFERRED,
    kind TEXT NOT NULL CHECK (kind IN ('about', 'mentions', 'context')),
    PRIMARY KEY (content, project, kind)
) STRICT;

CREATE TABLE gazette_registry_project_links (
    content TEXT NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    project TEXT NOT NULL REFERENCES gazette_registry_projects(id) DEFERRABLE INITIALLY DEFERRED,
    relation TEXT NOT NULL CHECK (relation IN ('about', 'mentions')),
    PRIMARY KEY (content, project)
) STRICT;

CREATE TABLE gazette_registry_project_evidence (
    content TEXT NOT NULL,
    project TEXT NOT NULL,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    PRIMARY KEY (content, project, source, kind),
    FOREIGN KEY (content, project) REFERENCES gazette_registry_project_links(content, project) DEFERRABLE INITIALLY DEFERRED,
    FOREIGN KEY (source, project, kind) REFERENCES gazette_registry_project_declarations(content, project, kind) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE gazette_registry_routes (
    content TEXT PRIMARY KEY NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    url TEXT NOT NULL UNIQUE,
    canonical_content TEXT,
    canonical_url TEXT NOT NULL,
    output TEXT NOT NULL,
    UNIQUE (content, url),
    CHECK ((canonical_content IS NULL AND canonical_url = url)
        OR (canonical_content IS NOT NULL AND canonical_content != content)),
    FOREIGN KEY (canonical_content, canonical_url) REFERENCES gazette_registry_routes(content, url) DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE gazette_registry_aliases (
    content TEXT NOT NULL REFERENCES gazette_registry_routes(content) DEFERRABLE INITIALLY DEFERRED,
    url TEXT NOT NULL UNIQUE,
    output TEXT NOT NULL,
    PRIMARY KEY (content, url)
) STRICT;

CREATE TABLE gazette_registry_provenance (
    content TEXT PRIMARY KEY NOT NULL REFERENCES gazette_registry_content(id) DEFERRABLE INITIALLY DEFERRED,
    source TEXT NOT NULL,
    entry TEXT NOT NULL,
    project TEXT REFERENCES gazette_registry_projects(id) DEFERRABLE INITIALLY DEFERRED
) STRICT;

-- Cross-site redirect files retain one content identity and canonical route.
-- The canonical home is joined through content, never duplicated on each mirror.
CREATE TABLE gazette_registry_mirrors (
    site TEXT NOT NULL REFERENCES gazette_registry_sites(id) DEFERRABLE INITIALLY DEFERRED,
    output TEXT NOT NULL,
    content TEXT NOT NULL REFERENCES gazette_registry_routes(content) DEFERRABLE INITIALLY DEFERRED,
    url TEXT NOT NULL UNIQUE,
    PRIMARY KEY (site, output)
) STRICT;

-- Configured-output history is separate from staging and publication authority.
-- Only a completed head can establish the preceding generated subset; an active
-- intent is uncertain filesystem history and never permission to repeat effects.
CREATE TABLE gazette_materialization_intents (
    id BLOB PRIMARY KEY NOT NULL CHECK (length(id) = 32),
    root TEXT NOT NULL,
    payload TEXT NOT NULL,
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32)
) STRICT;

CREATE INDEX gazette_materialization_by_root ON gazette_materialization_intents(root, id);

CREATE TABLE gazette_materialization_completions (
    intent_id BLOB PRIMARY KEY NOT NULL REFERENCES gazette_materialization_intents(id),
    payload TEXT NOT NULL,
    payload_sha256 BLOB NOT NULL CHECK (length(payload_sha256) = 32)
) STRICT;

CREATE TABLE gazette_materialization_frontiers (
    root TEXT PRIMARY KEY NOT NULL,
    head BLOB REFERENCES gazette_materialization_completions(intent_id),
    active BLOB REFERENCES gazette_materialization_intents(id),
    CHECK (head IS NOT NULL OR active IS NOT NULL),
    CHECK (head IS NULL OR active IS NULL OR head != active)
) STRICT;

-- Replaceable publication artifacts. These rows are derived from exact retained
-- inputs and can be discarded without changing authored truth or release history.
CREATE TABLE gazette_cache_head (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    rows_sha256 BLOB NOT NULL CHECK (length(rows_sha256) = 32),
    sites INTEGER NOT NULL CHECK (sites >= 0),
    dependencies INTEGER NOT NULL CHECK (dependencies >= 0),
    artifacts INTEGER NOT NULL CHECK (artifacts >= 0)
) STRICT;

CREATE TABLE gazette_cache_sites (
    site TEXT PRIMARY KEY NOT NULL,
    dependency_sha256 BLOB NOT NULL CHECK (length(dependency_sha256) = 32),
    output_sha256 BLOB NOT NULL CHECK (length(output_sha256) = 32),
    entries INTEGER NOT NULL CHECK (entries >= 0),
    bytes INTEGER NOT NULL CHECK (bytes >= 0)
) STRICT;

CREATE TABLE gazette_cache_dependencies (
    site TEXT NOT NULL REFERENCES gazette_cache_sites(site) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    kind TEXT NOT NULL CHECK (kind IN ('source', 'configuration', 'theme-template', 'build-mechanism', 'selected-semantic-input')),
    identity TEXT NOT NULL CHECK (length(identity) BETWEEN 1 AND 512),
    sha256 BLOB NOT NULL CHECK (length(sha256) = 32),
    PRIMARY KEY (site, kind, identity)
) STRICT;

CREATE TABLE gazette_cache_artifacts (
    site TEXT NOT NULL REFERENCES gazette_cache_sites(site) ON DELETE CASCADE DEFERRABLE INITIALLY DEFERRED,
    path TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('directory', 'file')),
    sha256 BLOB CHECK (sha256 IS NULL OR length(sha256) = 32),
    media_type TEXT CHECK (media_type IN ('html', 'json', 'json-ld', 'xml', 'rss', 'atom', 'text', 'markdown', 'javascript', 'css', 'png', 'jpeg', 'woff2')),
    payload BLOB,
    PRIMARY KEY (site, path),
    CHECK ((kind = 'directory' AND sha256 IS NULL AND media_type IS NULL AND payload IS NULL)
        OR (kind = 'file' AND sha256 IS NOT NULL AND payload IS NOT NULL))
) STRICT;

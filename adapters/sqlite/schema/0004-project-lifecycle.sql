-- Keep the registry's project states aligned with the typed publication model.
-- Initialization applies this only to a fresh store; existing journals are not upgraded.
-- Earlier steps have inserted no registry data. Reuse the empty table's pages
-- so initialization leaves no spare pages that could mask bounded-write growth.
DROP TABLE gazette_registry_projects;
CREATE TABLE gazette_registry_projects (
    id TEXT PRIMARY KEY NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('planned', 'active', 'paused', 'done', 'closed')),
    visibility TEXT NOT NULL CHECK (visibility IN ('public', 'unlisted', 'private')),
    availability TEXT NOT NULL CHECK (availability IN ('not-applicable', 'unavailable', 'gated', 'available')),
    name TEXT,
    repository TEXT,
    license TEXT
) STRICT;

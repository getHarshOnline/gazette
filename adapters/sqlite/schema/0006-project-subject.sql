-- Preserve explicit subject meaning independently from state and repository coordinates.
ALTER TABLE gazette_registry_projects ADD COLUMN kind TEXT CHECK (
    kind IN ('software-application', 'software-source-code', 'creative-work', 'collection', 'project', 'research-project')
);
ALTER TABLE gazette_registry_projects ADD COLUMN identity TEXT;

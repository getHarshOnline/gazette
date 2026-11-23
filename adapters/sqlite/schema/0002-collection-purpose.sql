-- Preserve collection reader purpose independently of the leaf-page family.
ALTER TABLE gazette_registry_content ADD COLUMN collection_purpose TEXT
    CHECK (collection_purpose IS NULL
        OR (kind = 'collection' AND collection_purpose IN ('editorial', 'documentation')));

-- Retain authored documentation order without creating new containment relations.
ALTER TABLE gazette_registry_content ADD COLUMN collection_navigation_json TEXT
    CHECK (collection_navigation_json IS NULL
        OR (kind = 'collection' AND json_valid(collection_navigation_json)
            AND json_type(collection_navigation_json) = 'array'));

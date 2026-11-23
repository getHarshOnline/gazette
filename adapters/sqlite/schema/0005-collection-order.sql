-- Preserve display ordering separately from membership and documentation outlines.
ALTER TABLE gazette_registry_content ADD COLUMN collection_member_order_json TEXT
    CHECK (collection_member_order_json IS NULL
        OR (kind = 'collection' AND json_valid(collection_member_order_json)
            AND json_type(collection_member_order_json) = 'array'));

-- Keep exact version identities and display metadata even after library removal.
CREATE TABLE queue_plans (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    items_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

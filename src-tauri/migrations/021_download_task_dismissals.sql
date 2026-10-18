-- Keep offline file bookkeeping after completed tasks leave the task list.
CREATE TABLE IF NOT EXISTS download_task_dismissals (
    task_id TEXT PRIMARY KEY NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
    cleared_at TEXT NOT NULL
);

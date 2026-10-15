-- Repair sources left by the former "remove root" implementation. Imported media
-- with a relative path always belonged to a mounted root; downloads are excluded.
CREATE TEMP TABLE IF NOT EXISTS detached_root_works (work_id TEXT PRIMARY KEY);
INSERT OR IGNORE INTO detached_root_works
SELECT DISTINCT t.work_id FROM media_sources ms
JOIN track_versions tv ON tv.id = ms.version_id JOIN tracks t ON t.id = tv.track_id
WHERE ms.source_kind = 'local_imported' AND ms.root_id IS NULL
  AND ms.relative_path IS NOT NULL AND ms.absolute_path IS NOT NULL;

DELETE FROM media_sources WHERE source_kind = 'local_imported' AND root_id IS NULL
  AND relative_path IS NOT NULL AND absolute_path IS NOT NULL;

-- Keep attachments backed by another local source, including offline downloads.
DELETE FROM attachments WHERE root_id IS NULL AND provider IS NULL
  AND relative_path IS NOT NULL AND absolute_path IS NOT NULL
  AND work_id IN (SELECT work_id FROM detached_root_works)
  AND NOT EXISTS (SELECT 1 FROM tracks t JOIN track_versions tv ON tv.track_id = t.id
    JOIN media_sources ms ON ms.version_id = tv.id
    WHERE t.work_id = attachments.work_id AND ms.source_kind IN ('local_imported', 'local_downloaded'));

-- Retain metadata, favorites, progress, bookmarks and queue version identities.
DELETE FROM collected_works WHERE work_id IN (SELECT work_id FROM detached_root_works)
  AND NOT EXISTS (SELECT 1 FROM library_entries le WHERE le.work_id = collected_works.work_id)
  AND NOT EXISTS (SELECT 1 FROM work_sources ws WHERE ws.work_id = collected_works.work_id)
  AND NOT EXISTS (SELECT 1 FROM tracks t JOIN track_versions tv ON tv.track_id = t.id
    JOIN media_sources ms ON ms.version_id = tv.id WHERE t.work_id = collected_works.work_id);
DROP TABLE detached_root_works;

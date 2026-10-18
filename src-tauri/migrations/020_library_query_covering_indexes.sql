-- T1515: Extend existing indexes instead of keeping duplicate prefix indexes.
-- Stream source aggregation by work, and count tracks in a listening directory.
DROP INDEX IF EXISTS idx_tracks_work_id;
CREATE INDEX idx_tracks_work_id ON tracks(work_id, logical_dir_path, id);
DROP INDEX IF EXISTS idx_track_versions_track_id;
CREATE INDEX idx_track_versions_track_id ON track_versions(track_id, id);

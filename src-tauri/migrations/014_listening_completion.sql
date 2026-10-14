ALTER TABLE playback_positions ADD COLUMN ever_completed INTEGER NOT NULL DEFAULT 0 CHECK(ever_completed IN (0,1));
UPDATE playback_positions SET ever_completed = 1 WHERE completed = 1;

-- Completion history survives a new listening pass; current progress remains independent.
CREATE TRIGGER playback_completed_insert AFTER INSERT ON playback_positions
WHEN NEW.completed = 1 AND NEW.ever_completed = 0
BEGIN
  UPDATE playback_positions SET ever_completed = 1 WHERE version_id = NEW.version_id;
END;
CREATE TRIGGER playback_completed_update AFTER UPDATE OF completed ON playback_positions
WHEN NEW.completed = 1 AND NEW.ever_completed = 0
BEGIN
  UPDATE playback_positions SET ever_completed = 1 WHERE version_id = NEW.version_id;
END;

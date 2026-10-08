-- Which gateway process opened an alert episode, and when it last said it was
-- alive: an error-rate or circuit episode rests on what that process saw, so
-- another process resolves it only once the owner has been silent for three
-- ticks. Rows from before have neither and are adopted by the first tick.
ALTER TABLE alert_state ADD COLUMN owner TEXT COLLATE "C";
ALTER TABLE alert_state ADD COLUMN seen_at TEXT COLLATE "C";

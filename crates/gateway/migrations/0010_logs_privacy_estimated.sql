-- User and team ids are given out again after a delete. What a deleted
-- user or team left in the request logs must not pass to the next one that
-- gets the id, so the rows are detached from it (the indexes on user_id and
-- team_id make this cheap). The rows stay, with no owner.
CREATE TRIGGER request_logs_user_deleted AFTER DELETE ON users
BEGIN
    UPDATE request_logs SET user_id = NULL WHERE user_id = OLD.id;
END;

CREATE TRIGGER request_logs_team_deleted AFTER DELETE ON teams
BEGIN
    UPDATE request_logs SET team_id = NULL WHERE team_id = OLD.id;
END;

-- 1 when the tokens and cost of the row are an estimate: a stream that
-- ended without a usage report (the caller went away, or an error after
-- content was sent).
ALTER TABLE request_logs ADD COLUMN estimated INTEGER NOT NULL DEFAULT 0;

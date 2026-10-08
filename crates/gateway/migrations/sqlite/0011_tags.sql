-- Tags: a JSON object of strings, NULL for none. A key's tags are fixed by
-- whoever made or manages the key; a call's tags are those it sent, overlaid
-- by its key's.
ALTER TABLE virtual_keys ADD COLUMN tags TEXT;
ALTER TABLE request_logs ADD COLUMN tags TEXT;

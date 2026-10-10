-- Whether a route is for every user. Before this a route with no grant rows
-- was open, so deleting its last team opened it to everyone.
ALTER TABLE routes ADD COLUMN everyone INTEGER NOT NULL DEFAULT 1;

-- A route that was restricted to teams stays restricted.
UPDATE routes SET everyone = 0 WHERE id IN (SELECT route_id FROM route_grants);

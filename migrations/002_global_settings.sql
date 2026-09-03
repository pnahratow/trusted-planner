-- "Move completed to bottom" is a property of how the board reads, not of who
-- is reading it: two people looking at the same shared column should see the
-- same order. Move it off the user row and make it app-wide.

CREATE TABLE app_settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO app_settings (key, value) VALUES ('move_completed_to_bottom', '1');

ALTER TABLE users DROP COLUMN move_completed_to_bottom;

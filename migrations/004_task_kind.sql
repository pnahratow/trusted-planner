-- A task is owed until it is done; an appointment merely happens and then is
-- past. That difference is the overdue sweep: an unticked task on a day gone by
-- is still owed and follows the rule, an appointment on the same day is a
-- record of a time that passed and stays where it was written.
--
-- "appointment" rather than "event", which in this codebase already means a
-- change on the board that other browsers have to hear about (src/events.rs).
--
-- Everything that exists was written as something owed, so 'task' is the
-- default and the backfill is the default.

ALTER TABLE tasks ADD COLUMN kind TEXT NOT NULL DEFAULT 'task';

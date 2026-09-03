-- Trusted Planner initial schema.
-- A day is a list that has a date; a custom list is a list that doesn't.

CREATE TABLE users (
    id                       INTEGER PRIMARY KEY,
    name                     TEXT    NOT NULL,
    colour                   TEXT    NOT NULL,
    theme                    TEXT    NOT NULL DEFAULT 'system',
    move_completed_to_bottom INTEGER NOT NULL DEFAULT 1,
    created_at               TEXT    NOT NULL,
    deleted_at               TEXT
);

CREATE TABLE boards (
    id         INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    created_at TEXT    NOT NULL,
    deleted_at TEXT
);

CREATE TABLE board_members (
    board_id INTEGER NOT NULL REFERENCES boards(id),
    user_id  INTEGER NOT NULL REFERENCES users(id),
    PRIMARY KEY (board_id, user_id)
);

CREATE TABLE lists (
    id         INTEGER PRIMARY KEY,
    board_id   INTEGER NOT NULL REFERENCES boards(id),
    name       TEXT,
    date       TEXT,
    position   INTEGER NOT NULL DEFAULT 0,
    created_at TEXT    NOT NULL,
    deleted_at TEXT
);

-- One day-list per board per date. Custom lists (date IS NULL) are exempt.
CREATE UNIQUE INDEX lists_board_date_uniq
    ON lists (board_id, date)
    WHERE date IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX lists_board_idx ON lists (board_id, date);

CREATE TABLE tasks (
    id         INTEGER PRIMARY KEY,
    list_id    INTEGER NOT NULL REFERENCES lists(id),
    title      TEXT    NOT NULL,
    notes      TEXT    NOT NULL DEFAULT '',
    done       INTEGER NOT NULL DEFAULT 0,
    author_id  INTEGER NOT NULL REFERENCES users(id),
    position   INTEGER NOT NULL DEFAULT 0,
    version    INTEGER NOT NULL DEFAULT 1,
    created_at TEXT    NOT NULL,
    updated_at TEXT    NOT NULL,
    deleted_at TEXT
);

CREATE INDEX tasks_list_position_idx ON tasks (list_id, position);

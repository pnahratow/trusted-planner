-- Which grid each person last read a board in.
--
-- Per user rather than app-wide: unlike ordering (D20), the view changes
-- nothing about the data, only how much of it fits on the screen in front of
-- you. Two people on a laptop and a big monitor should be able to disagree,
-- the same way they already can about the theme.
--
-- Rows for a deleted board or user are never read again and are left alone;
-- there is no cascade anywhere else in this schema either.

CREATE TABLE board_views (
    user_id  INTEGER NOT NULL REFERENCES users(id),
    board_id INTEGER NOT NULL REFERENCES boards(id),
    view     TEXT    NOT NULL,
    PRIMARY KEY (user_id, board_id)
);

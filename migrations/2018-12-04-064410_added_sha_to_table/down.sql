CREATE TABLE entries_backup(
    id integer PRIMARY KEY NOT NULL,
    requested TIMESTAMP NOT NULL,
    created TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,
    name TEXT NOT NULL,
    revision INTEGER NOT NULL,
    sha TEXT NOT NULL
    path TEXT NOT NULL,
    file_kind TEXT NOT NULL,
    stat_kind TEXT NOT NULL,
    value TEXT NOT NULL
);
INSERT INTO entries_backup SELECT id, requested, created, name, revision, sha, path, file_kind, stat_kind, value FROM entries;
DROP TABLE entries;
ALTER TABLE entries_backup RENAME TO entries;
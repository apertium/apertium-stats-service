CREATE TABLE entries (
    id integer PRIMARY KEY NOT NULL,
    requested TIMESTAMP NOT NULL,
    created TIMESTAMP DEFAULT CURRENT_TIMESTAMP NOT NULL,
    name TEXT NOT NULL,
    revision INTEGER NOT NULL,
    path TEXT NOT NULL,
    file_kind TEXT NOT NULL,
    stat_kind TEXT NOT NULL,
    value TEXT NOT NULL
);
CREATE INDEX name_index ON entries (name);
CREATE INDEX name_stat_kind_index ON entries (name, file_kind);
CREATE INDEX name_file_kind_index ON entries (name, stat_kind);

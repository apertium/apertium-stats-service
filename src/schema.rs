table! {
    use diesel::sql_types::{Integer, Timestamp, Text};
    use models::{FileKindMapping, StatKindMapping};

    entries (id) {
        id -> Integer,
        requested -> Timestamp,
        created -> Timestamp,
        name -> Text,
        revision -> Integer,
        path -> Text,
        file_kind -> FileKindMapping,
        stat_kind -> StatKindMapping,
        value -> Text,
    }
}

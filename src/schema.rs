table! {
    use diesel::sql_types::{Integer, Timestamp, Text};
    use crate::models::{FileKindMapping, StatKindMapping};
    use crate::util::JsonType;

    entries (id) {
        id -> Integer,
        requested -> Timestamp,
        created -> Timestamp,
        name -> Text,
        revision -> Integer,
        sha -> Text,
        path -> Text,
        last_changed -> Timestamp,
        last_author -> Text,
        size -> Integer,
        file_kind -> FileKindMapping,
        stat_kind -> StatKindMapping,
        value -> JsonType,
    }
}

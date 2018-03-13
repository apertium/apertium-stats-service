table! {
    entries (id) {
        id -> Integer,
        requested -> Timestamp,
        created -> Timestamp,
        name -> Text,
        revision -> Integer,
        path -> Text,
        file_kind -> Text,
        stat_kind -> Text,
        value -> Text,
    }
}

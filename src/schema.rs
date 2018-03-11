table! {
    entries (id) {
        id -> Integer,
        requested -> Timestamp,
        created -> Timestamp,
        name -> Text,
        revision -> Integer,
        path -> Text,
        kind -> Text,
        value -> Text,
    }
}

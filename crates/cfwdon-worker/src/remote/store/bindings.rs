use worker::d1::D1Type;

pub(super) fn optional_text_binding(value: Option<&str>) -> D1Type<'_> {
    match value {
        Some(value) => D1Type::Text(value),
        None => D1Type::Null,
    }
}

pub(super) fn bool_binding(value: bool) -> D1Type<'static> {
    D1Type::Integer(i32::from(value))
}

pub(super) fn remote_status_id_bindings(status_id: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(status_id)]
}

pub(super) fn remote_status_object_uri_bindings(object_uri: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(object_uri)]
}

pub(super) fn remote_status_lookup_value_bindings(value: &str) -> [D1Type<'_>; 1] {
    [D1Type::Text(value)]
}

pub(super) fn remote_status_quote_state_update_bindings<'a>(
    quote_state: &'a str,
    status_id: &'a str,
) -> [D1Type<'a>; 2] {
    [D1Type::Text(quote_state), D1Type::Text(status_id)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_status_id_bindings_keep_sql_slot_order_stable() {
        let bindings = remote_status_id_bindings("remote-status-id");

        assert!(matches!(bindings[0], D1Type::Text("remote-status-id")));
    }

    #[test]
    fn remote_status_object_uri_bindings_keep_sql_slot_order_stable() {
        let bindings =
            remote_status_object_uri_bindings("https://remote.example/users/alice/statuses/1");

        assert!(matches!(
            bindings[0],
            D1Type::Text("https://remote.example/users/alice/statuses/1")
        ));
    }

    #[test]
    fn remote_status_lookup_value_bindings_keep_sql_slot_order_stable() {
        let bindings = remote_status_lookup_value_bindings("https://remote.example/@alice/1");

        assert!(matches!(
            bindings[0],
            D1Type::Text("https://remote.example/@alice/1")
        ));
    }

    #[test]
    fn remote_status_quote_state_update_bindings_keep_sql_slot_order_stable() {
        let bindings = remote_status_quote_state_update_bindings("revoked", "remote-status-id");

        assert!(matches!(bindings[0], D1Type::Text("revoked")));
        assert!(matches!(bindings[1], D1Type::Text("remote-status-id")));
    }
}

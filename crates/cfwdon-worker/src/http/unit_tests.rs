use crate::http::{
    parse_activitypub_request_date_ms, parse_signature_header, signed_get_signing_string,
    validate_activitypub_signature_headers,
};

#[test]
fn signed_get_signing_string_uses_get_request_target() {
    assert_eq!(
        signed_get_signing_string(
            "/users/alice/followers",
            "remote.example",
            "Sun, 19 Jul 2026 12:00:00 GMT"
        ),
        "(request-target): get /users/alice/followers\nhost: remote.example\ndate: Sun, 19 Jul 2026 12:00:00 GMT"
    );
}

#[test]
fn validate_activitypub_signature_headers_requires_body_integrity_headers() {
    let missing_target = parse_signature_header(
        "keyId=\"https://remote.example/users/bob#main-key\",headers=\"date digest\",signature=\"YQ==\"",
    )
    .unwrap();
    let error = validate_activitypub_signature_headers(&missing_target).unwrap_err();
    assert!(error.to_string().contains("(request-target)"));

    let complete = parse_signature_header(
        "keyId=\"https://remote.example/users/bob#main-key\",headers=\"(request-target) host date digest\",signature=\"YQ==\"",
    )
    .unwrap();
    validate_activitypub_signature_headers(&complete).unwrap();
}

#[test]
fn parse_activitypub_request_date_accepts_common_http_date_variants() {
    let expected = 784_889_377_000.0;
    for value in [
        "Tue, 15 Nov 1994 08:49:37 GMT",
        "Tue, 15 Nov 1994 08:49:37 +0000",
        "1994-11-15T08:49:37Z",
        "Tuesday, 15-Nov-94 08:49:37 GMT",
        "Tue Nov 15 08:49:37 1994",
    ] {
        assert_eq!(
            parse_activitypub_request_date_ms(value),
            Some(expected),
            "{value}"
        );
    }
}

use worker::console_log;

pub(crate) fn observability_started_at_ms() -> f64 {
    js_sys::Date::now()
}

pub(crate) fn observability_duration_ms(started_at_ms: f64) -> u64 {
    (js_sys::Date::now() - started_at_ms).max(0.0).round() as u64
}

pub(crate) fn log_json_event(mut payload: serde_json::Value) {
    if let Some(object) = payload.as_object_mut() {
        object.insert("source".to_owned(), serde_json::json!("cfwdon"));
    }

    match serde_json::to_string(&payload) {
        Ok(message) => console_log!("{}", message),
        Err(error) => console_log!("cfwdon_observability_log_error={}", error),
    }
}

pub(crate) fn log_observed_operation(
    component: &str,
    operation: &str,
    outcome: &str,
    started_at_ms: f64,
    details: serde_json::Value,
) {
    let mut payload = serde_json::json!({
        "event": "cfwdon_operation",
        "component": component,
        "operation": operation,
        "outcome": outcome,
        "duration_ms": observability_duration_ms(started_at_ms),
    });

    if let (Some(payload), Some(details)) = (payload.as_object_mut(), details.as_object()) {
        for (key, value) in details {
            payload.insert(key.clone(), value.clone());
        }
    }

    log_json_event(payload);
}

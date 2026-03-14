use rocket::http::Status;
use rocket::local::blocking::Client;
use rocket::serde::json::serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn fixture(path: &str) -> String {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    fs::read_to_string(base.join("..").join(path)).expect("read fixture")
}

fn create_session_id(client: &Client, assembly: &str) -> String {
    let response = client
        .post("/api/v1/sessions")
        .json(&json!({ "assembly": assembly }))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    let body: Value = response.into_json().expect("response JSON");
    body.get("session_id")
        .and_then(Value::as_str)
        .expect("session_id string")
        .to_string()
}

#[test]
fn echo_program_round_trip_via_api() {
    let client =
        Client::tracked(misch_api::build_rocket()).expect("rocket client");
    let assembly = fixture("examples/echo.mixal");
    let session_id = create_session_id(&client, &assembly);

    let response = client
        .post(format!("/api/v1/sessions/{session_id}/io/input/text"))
        .json(&json!({ "unit": 16, "text": "HELLO" }))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    let response = client
        .post(format!("/api/v1/sessions/{session_id}/run"))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let run_body: Value = response.into_json().expect("run response JSON");
    assert_eq!(run_body.get("halted").and_then(Value::as_bool), Some(true));
    assert_eq!(
        run_body.get("reached_step_limit").and_then(Value::as_bool),
        Some(false)
    );

    let response = client
        .get(format!(
            "/api/v1/sessions/{session_id}/io/output/text?unit=18"
        ))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let output_body: Value =
        response.into_json().expect("output response JSON");
    let text = output_body
        .get("units")
        .and_then(Value::as_object)
        .and_then(|units| units.get("18"))
        .and_then(Value::as_str)
        .expect("unit 18 text output");
    assert_eq!(text, "HELLO");
}

#[test]
fn primes_program_runs_and_emits_expected_count() {
    let client =
        Client::tracked(misch_api::build_rocket()).expect("rocket client");
    let assembly = fixture("examples/primes.mixal");
    let session_id = create_session_id(&client, &assembly);

    let response = client
        .post(format!("/api/v1/sessions/{session_id}/io/input/text"))
        .json(&json!({ "unit": 16, "text": "00017" }))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    let response = client
        .post(format!("/api/v1/sessions/{session_id}/run"))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let run_body: Value = response.into_json().expect("run response JSON");
    assert_eq!(run_body.get("halted").and_then(Value::as_bool), Some(true));
    assert_eq!(
        run_body.get("reached_step_limit").and_then(Value::as_bool),
        Some(false)
    );

    let response = client
        .get(format!(
            "/api/v1/sessions/{session_id}/io/output/text?unit=18"
        ))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let output_body: Value =
        response.into_json().expect("output response JSON");
    let text = output_body
        .get("units")
        .and_then(Value::as_object)
        .and_then(|units| units.get("18"))
        .and_then(Value::as_str)
        .expect("unit 18 text output");

    let primes: Vec<&str> = text.split_whitespace().collect();
    assert_eq!(primes.len(), 17);
    assert_eq!(primes.first().copied(), Some("00002"));
    assert_eq!(primes.last().copied(), Some("00059"));
}

#[test]
fn vigenere_program_encrypts_text_from_paper_tape() {
    let client =
        Client::tracked(misch_api::build_rocket()).expect("rocket client");
    let assembly = fixture("examples/vigenere.mixal");
    let session_id = create_session_id(&client, &assembly);

    let response = client
        .post(format!("/api/v1/sessions/{session_id}/io/input/text"))
        .json(&json!({
            "unit": 16,
            "text": "LEMON     ATTACK AT DAWN. 123"
        }))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);

    let response = client
        .post(format!("/api/v1/sessions/{session_id}/run"))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let run_body: Value = response.into_json().expect("run response JSON");
    assert_eq!(run_body.get("halted").and_then(Value::as_bool), Some(true));
    assert_eq!(
        run_body.get("reached_step_limit").and_then(Value::as_bool),
        Some(false)
    );

    let response = client
        .get(format!(
            "/api/v1/sessions/{session_id}/io/output/text?unit=18"
        ))
        .dispatch();
    assert_eq!(response.status(), Status::Ok);
    let output_body: Value =
        response.into_json().expect("output response JSON");
    let text = output_body
        .get("units")
        .and_then(Value::as_object)
        .and_then(|units| units.get("18"))
        .and_then(Value::as_str)
        .expect("unit 18 text output");

    assert_eq!(text.trim_end(), "LXFOPV EF RNHR. 123");
}

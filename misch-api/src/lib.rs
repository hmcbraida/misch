use misch_core::{
    MixState, assemble, decode_words_to_text, encode_text_to_words,
};
use rocket::State;
use rocket::http::Status;
use rocket::response::content::RawHtml;
use rocket::response::status;
use rocket::serde::json::serde_json::{Value, json};
use rocket::serde::{Deserialize, Serialize, json::Json};
use rocket::{delete, get, post, routes};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const MAX_RUN_STEPS: usize = 100_000;
const DEFAULT_INPUT_UNIT: u8 = 16;
const DEFAULT_OUTPUT_UNIT: u8 = 18;

type Sessions = Mutex<HashMap<Uuid, Session>>;
type ApiResult<T> = Result<Json<T>, status::Custom<Json<ErrorResponse>>>;

struct Session {
    machine: MixState,
    io_buffers: Arc<Mutex<IoBuffers>>,
    input_devices: HashMap<u8, usize>,
    output_devices: HashMap<u8, usize>,
}

#[derive(Debug, Default)]
struct IoBuffers {
    input_queues: HashMap<u8, VecDeque<i64>>,
    output_words: HashMap<u8, Vec<i64>>,
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct ErrorResponse {
    error: String,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct DeviceConfig {
    unit: u8,
    block_size: usize,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct CreateSessionRequest {
    assembly: String,
    input_devices: Option<Vec<DeviceConfig>>,
    output_devices: Option<Vec<DeviceConfig>>,
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct CreateSessionResponse {
    session_id: Uuid,
    halted: bool,
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct RunResponse {
    halted: bool,
    steps_executed: usize,
    reached_step_limit: bool,
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct SnapshotResponse {
    halted: bool,
    ic: u16,
    overflow: bool,
    comparison: String,
    a: i64,
    x: i64,
    i: [i32; 6],
    j: i32,
    memory_start: usize,
    memory: Vec<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct InputTextRequest {
    unit: u8,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(crate = "rocket::serde")]
struct InputRawRequest {
    unit: u8,
    words: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct InputAcceptedResponse {
    queued_words: usize,
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct OutputRawResponse {
    units: HashMap<u8, Vec<i64>>,
}

#[derive(Debug, Serialize)]
#[serde(crate = "rocket::serde")]
struct OutputTextResponse {
    units: HashMap<u8, String>,
}

fn error(
    status: Status,
    message: impl Into<String>,
) -> status::Custom<Json<ErrorResponse>> {
    status::Custom(
        status,
        Json(ErrorResponse {
            error: message.into(),
        }),
    )
}

fn parse_session_id(
    id: &str,
) -> Result<Uuid, status::Custom<Json<ErrorResponse>>> {
    Uuid::parse_str(id).map_err(|_| {
        error(Status::BadRequest, format!("invalid session id `{id}`"))
    })
}

fn build_session(req: CreateSessionRequest) -> Result<Session, String> {
    let mut machine = assemble(&req.assembly).map_err(|err| err.to_string())?;

    let mut input_devices = HashMap::new();
    let mut output_devices = HashMap::new();

    if let Some(devices) = req.input_devices {
        for device in devices {
            if device.block_size == 0 {
                return Err(format!(
                    "input device {} block size must be greater than 0",
                    device.unit
                ));
            }
            input_devices.insert(device.unit, device.block_size);
        }
    }
    if let Some(devices) = req.output_devices {
        for device in devices {
            if device.block_size == 0 {
                return Err(format!(
                    "output device {} block size must be greater than 0",
                    device.unit
                ));
            }
            output_devices.insert(device.unit, device.block_size);
        }
    }

    if input_devices.is_empty() {
        input_devices.insert(DEFAULT_INPUT_UNIT, 1);
    }
    if output_devices.is_empty() {
        output_devices.insert(DEFAULT_OUTPUT_UNIT, 1);
    }

    let io_buffers = Arc::new(Mutex::new(IoBuffers::default()));
    {
        let mut io = io_buffers
            .lock()
            .map_err(|_| "session I/O lock poisoned".to_string())?;
        for &unit in input_devices.keys() {
            io.input_queues.entry(unit).or_default();
        }
        for &unit in output_devices.keys() {
            io.output_words.entry(unit).or_default();
        }
    }

    for (&unit, &block_size) in &input_devices {
        let io = Arc::clone(&io_buffers);
        machine
            .attach_input_callback(unit, block_size, move || {
                let mut buffers = io.lock().map_err(|_| {
                    misch_core::MixError::DeviceNotAttached(unit)
                })?;
                let queue = buffers.input_queues.entry(unit).or_default();
                let mut block = Vec::with_capacity(block_size);
                for _ in 0..block_size {
                    block.push(queue.pop_front().unwrap_or(0));
                }
                Ok(block)
            })
            .map_err(|err| err.to_string())?;
    }

    for (&unit, &block_size) in &output_devices {
        let io = Arc::clone(&io_buffers);
        machine
            .attach_output_callback(unit, block_size, move |block| {
                let mut buffers = io.lock().map_err(|_| {
                    misch_core::MixError::DeviceNotAttached(unit)
                })?;
                buffers
                    .output_words
                    .entry(unit)
                    .or_default()
                    .extend_from_slice(block);
                Ok(())
            })
            .map_err(|err| err.to_string())?;
    }

    Ok(Session {
        machine,
        io_buffers,
        input_devices,
        output_devices,
    })
}

#[post("/sessions", data = "<req>")]
fn create_session(
    sessions: &State<Sessions>,
    req: Json<CreateSessionRequest>,
) -> ApiResult<CreateSessionResponse> {
    let session = build_session(req.into_inner())
        .map_err(|msg| error(Status::BadRequest, msg))?;
    let halted = session.machine.is_halted();
    let id = Uuid::new_v4();
    let mut map = sessions.lock().map_err(|_| {
        error(Status::InternalServerError, "session store lock poisoned")
    })?;
    map.insert(id, session);
    Ok(Json(CreateSessionResponse {
        session_id: id,
        halted,
    }))
}

#[post("/sessions/<id>/run")]
fn run_session(sessions: &State<Sessions>, id: &str) -> ApiResult<RunResponse> {
    let id = parse_session_id(id)?;
    let mut map = sessions.lock().map_err(|_| {
        error(Status::InternalServerError, "session store lock poisoned")
    })?;
    let session = map.get_mut(&id).ok_or_else(|| {
        error(Status::NotFound, format!("unknown session id `{id}`"))
    })?;

    let mut steps_executed = 0usize;
    while !session.machine.is_halted() && steps_executed < MAX_RUN_STEPS {
        session
            .machine
            .advance_state()
            .map_err(|err| error(Status::BadRequest, err.to_string()))?;
        steps_executed += 1;
    }

    let halted = session.machine.is_halted();
    Ok(Json(RunResponse {
        halted,
        steps_executed,
        reached_step_limit: !halted && steps_executed == MAX_RUN_STEPS,
    }))
}

#[get("/sessions/<id>?<memory_start>&<memory_length>")]
fn get_session(
    sessions: &State<Sessions>,
    id: &str,
    memory_start: Option<usize>,
    memory_length: Option<usize>,
) -> ApiResult<SnapshotResponse> {
    let id = parse_session_id(id)?;
    let map = sessions.lock().map_err(|_| {
        error(Status::InternalServerError, "session store lock poisoned")
    })?;
    let session = map.get(&id).ok_or_else(|| {
        error(Status::NotFound, format!("unknown session id `{id}`"))
    })?;

    let memory_start = memory_start.unwrap_or(0);
    let memory_length = memory_length.unwrap_or(64);
    let memory = session
        .machine
        .memory_window(memory_start, memory_length)
        .map_err(|err| error(Status::BadRequest, err.to_string()))?;

    let i = [
        session.machine.index_register(1).unwrap_or(0),
        session.machine.index_register(2).unwrap_or(0),
        session.machine.index_register(3).unwrap_or(0),
        session.machine.index_register(4).unwrap_or(0),
        session.machine.index_register(5).unwrap_or(0),
        session.machine.index_register(6).unwrap_or(0),
    ];

    Ok(Json(SnapshotResponse {
        halted: session.machine.is_halted(),
        ic: session.machine.instruction_counter(),
        overflow: session.machine.overflow_flag(),
        comparison: session.machine.comparison_indicator().to_string(),
        a: session.machine.register_a(),
        x: session.machine.register_x(),
        i,
        j: session.machine.register_j(),
        memory_start,
        memory,
    }))
}

#[post("/sessions/<id>/io/input/text", data = "<req>")]
fn append_input_text(
    sessions: &State<Sessions>,
    id: &str,
    req: Json<InputTextRequest>,
) -> ApiResult<InputAcceptedResponse> {
    let id = parse_session_id(id)?;
    let mut map = sessions.lock().map_err(|_| {
        error(Status::InternalServerError, "session store lock poisoned")
    })?;
    let session = map.get_mut(&id).ok_or_else(|| {
        error(Status::NotFound, format!("unknown session id `{id}`"))
    })?;

    if !session.input_devices.contains_key(&req.unit) {
        return Err(error(
            Status::BadRequest,
            format!("input device unit {} is not configured", req.unit),
        ));
    }

    let words = encode_text_to_words(&req.text)
        .map_err(|err| error(Status::BadRequest, err.to_string()))?;
    let queued_words = words.len();
    let mut io = session.io_buffers.lock().map_err(|_| {
        error(Status::InternalServerError, "session I/O lock poisoned")
    })?;
    io.input_queues.entry(req.unit).or_default().extend(words);

    Ok(Json(InputAcceptedResponse { queued_words }))
}

#[post("/sessions/<id>/io/input/raw", data = "<req>")]
fn append_input_raw(
    sessions: &State<Sessions>,
    id: &str,
    req: Json<InputRawRequest>,
) -> ApiResult<InputAcceptedResponse> {
    let id = parse_session_id(id)?;
    let mut map = sessions.lock().map_err(|_| {
        error(Status::InternalServerError, "session store lock poisoned")
    })?;
    let session = map.get_mut(&id).ok_or_else(|| {
        error(Status::NotFound, format!("unknown session id `{id}`"))
    })?;

    if !session.input_devices.contains_key(&req.unit) {
        return Err(error(
            Status::BadRequest,
            format!("input device unit {} is not configured", req.unit),
        ));
    }

    let queued_words = req.words.len();
    let mut io = session.io_buffers.lock().map_err(|_| {
        error(Status::InternalServerError, "session I/O lock poisoned")
    })?;
    io.input_queues
        .entry(req.unit)
        .or_default()
        .extend(req.words.iter().copied());

    Ok(Json(InputAcceptedResponse { queued_words }))
}

#[get("/sessions/<id>/io/output/raw?<unit>&<drain>")]
fn get_output_raw(
    sessions: &State<Sessions>,
    id: &str,
    unit: Option<u8>,
    drain: Option<bool>,
) -> ApiResult<OutputRawResponse> {
    let id = parse_session_id(id)?;
    let mut map = sessions.lock().map_err(|_| {
        error(Status::InternalServerError, "session store lock poisoned")
    })?;
    let session = map.get_mut(&id).ok_or_else(|| {
        error(Status::NotFound, format!("unknown session id `{id}`"))
    })?;
    let drain = drain.unwrap_or(false);

    let mut io = session.io_buffers.lock().map_err(|_| {
        error(Status::InternalServerError, "session I/O lock poisoned")
    })?;
    let mut units = HashMap::new();

    if let Some(target_unit) = unit {
        if !session.output_devices.contains_key(&target_unit) {
            return Err(error(
                Status::BadRequest,
                format!("output device unit {} is not configured", target_unit),
            ));
        }
        let data = if drain {
            std::mem::take(io.output_words.entry(target_unit).or_default())
        } else {
            io.output_words
                .get(&target_unit)
                .cloned()
                .unwrap_or_default()
        };
        units.insert(target_unit, data);
    } else {
        for &configured in session.output_devices.keys() {
            let data = if drain {
                std::mem::take(io.output_words.entry(configured).or_default())
            } else {
                io.output_words
                    .get(&configured)
                    .cloned()
                    .unwrap_or_default()
            };
            units.insert(configured, data);
        }
    }

    Ok(Json(OutputRawResponse { units }))
}

#[get("/sessions/<id>/io/output/text?<unit>&<drain>")]
fn get_output_text(
    sessions: &State<Sessions>,
    id: &str,
    unit: Option<u8>,
    drain: Option<bool>,
) -> ApiResult<OutputTextResponse> {
    let raw = get_output_raw(sessions, id, unit, drain)?.into_inner();
    let mut units = HashMap::new();
    for (unit, words) in raw.units {
        units.insert(unit, decode_words_to_text(&words));
    }
    Ok(Json(OutputTextResponse { units }))
}

#[delete("/sessions/<id>")]
fn delete_session(
    sessions: &State<Sessions>,
    id: &str,
) -> Result<status::NoContent, status::Custom<Json<ErrorResponse>>> {
    let id = parse_session_id(id)?;
    let mut map = sessions.lock().map_err(|_| {
        error(Status::InternalServerError, "session store lock poisoned")
    })?;
    if map.remove(&id).is_some() {
        Ok(status::NoContent)
    } else {
        Err(error(
            Status::NotFound,
            format!("unknown session id `{id}`"),
        ))
    }
}

#[get("/openapi.json")]
fn openapi_spec() -> Json<Value> {
    Json(json!({
        "openapi": "3.0.3",
        "info": {
            "title": "misch API",
            "version": "0.1.0",
            "description": "Stateful API for running MIX assembly sessions."
        },
        "servers": [{ "url": "/api/v1" }],
        "paths": {
            "/sessions": {
                "post": {
                    "summary": "Create session",
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateSessionRequest" } } } },
                    "responses": {
                        "200": { "description": "Session created", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/CreateSessionResponse" } } } },
                        "400": { "description": "Invalid request", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
                    }
                }
            },
            "/sessions/{id}": {
                "get": {
                    "summary": "Get session snapshot",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } },
                        { "name": "memory_start", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 0 } },
                        { "name": "memory_length", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 0 } }
                    ],
                    "responses": {
                        "200": { "description": "Session snapshot", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/SnapshotResponse" } } } },
                        "400": { "description": "Invalid request", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
                        "404": { "description": "Session not found", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
                    }
                },
                "delete": {
                    "summary": "Delete session",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }
                    ],
                    "responses": {
                        "204": { "description": "Session deleted" },
                        "404": { "description": "Session not found", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
                    }
                }
            },
            "/sessions/{id}/run": {
                "post": {
                    "summary": "Run session until halt or step limit",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }
                    ],
                    "responses": {
                        "200": { "description": "Run complete", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/RunResponse" } } } },
                        "400": { "description": "Execution error", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
                        "404": { "description": "Session not found", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
                    }
                }
            },
            "/sessions/{id}/io/input/text": {
                "post": {
                    "summary": "Append text input",
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/InputTextRequest" } } } },
                    "responses": {
                        "200": { "description": "Input queued", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/InputAcceptedResponse" } } } },
                        "400": { "description": "Invalid request", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
                        "404": { "description": "Session not found", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
                    }
                }
            },
            "/sessions/{id}/io/input/raw": {
                "post": {
                    "summary": "Append raw word input",
                    "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } }],
                    "requestBody": { "required": true, "content": { "application/json": { "schema": { "$ref": "#/components/schemas/InputRawRequest" } } } },
                    "responses": {
                        "200": { "description": "Input queued", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/InputAcceptedResponse" } } } },
                        "400": { "description": "Invalid request", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } },
                        "404": { "description": "Session not found", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/ErrorResponse" } } } }
                    }
                }
            },
            "/sessions/{id}/io/output/raw": {
                "get": {
                    "summary": "Read raw output",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } },
                        { "name": "unit", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 0, "maximum": 20 } },
                        { "name": "drain", "in": "query", "required": false, "schema": { "type": "boolean" } }
                    ],
                    "responses": {
                        "200": { "description": "Output words", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/OutputRawResponse" } } } }
                    }
                }
            },
            "/sessions/{id}/io/output/text": {
                "get": {
                    "summary": "Read output as decoded MIX text",
                    "parameters": [
                        { "name": "id", "in": "path", "required": true, "schema": { "type": "string", "format": "uuid" } },
                        { "name": "unit", "in": "query", "required": false, "schema": { "type": "integer", "minimum": 0, "maximum": 20 } },
                        { "name": "drain", "in": "query", "required": false, "schema": { "type": "boolean" } }
                    ],
                    "responses": {
                        "200": { "description": "Output text", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/OutputTextResponse" } } } }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "ErrorResponse": {
                    "type": "object",
                    "properties": { "error": { "type": "string" } },
                    "required": ["error"]
                },
                "DeviceConfig": {
                    "type": "object",
                    "properties": {
                        "unit": { "type": "integer", "minimum": 0, "maximum": 20 },
                        "block_size": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["unit", "block_size"]
                },
                "CreateSessionRequest": {
                    "type": "object",
                    "properties": {
                        "assembly": { "type": "string" },
                        "input_devices": { "type": "array", "items": { "$ref": "#/components/schemas/DeviceConfig" } },
                        "output_devices": { "type": "array", "items": { "$ref": "#/components/schemas/DeviceConfig" } }
                    },
                    "required": ["assembly"]
                },
                "CreateSessionResponse": {
                    "type": "object",
                    "properties": {
                        "session_id": { "type": "string", "format": "uuid" },
                        "halted": { "type": "boolean" }
                    },
                    "required": ["session_id", "halted"]
                },
                "RunResponse": {
                    "type": "object",
                    "properties": {
                        "halted": { "type": "boolean" },
                        "steps_executed": { "type": "integer" },
                        "reached_step_limit": { "type": "boolean" }
                    },
                    "required": ["halted", "steps_executed", "reached_step_limit"]
                },
                "SnapshotResponse": {
                    "type": "object",
                    "properties": {
                        "halted": { "type": "boolean" },
                        "ic": { "type": "integer" },
                        "overflow": { "type": "boolean" },
                        "comparison": { "type": "string" },
                        "a": { "type": "integer" },
                        "x": { "type": "integer" },
                        "i": { "type": "array", "items": { "type": "integer" }, "minItems": 6, "maxItems": 6 },
                        "j": { "type": "integer" },
                        "memory_start": { "type": "integer" },
                        "memory": { "type": "array", "items": { "type": "integer" } }
                    },
                    "required": ["halted", "ic", "overflow", "comparison", "a", "x", "i", "j", "memory_start", "memory"]
                },
                "InputTextRequest": {
                    "type": "object",
                    "properties": {
                        "unit": { "type": "integer", "minimum": 0, "maximum": 20 },
                        "text": { "type": "string" }
                    },
                    "required": ["unit", "text"]
                },
                "InputRawRequest": {
                    "type": "object",
                    "properties": {
                        "unit": { "type": "integer", "minimum": 0, "maximum": 20 },
                        "words": { "type": "array", "items": { "type": "integer" } }
                    },
                    "required": ["unit", "words"]
                },
                "InputAcceptedResponse": {
                    "type": "object",
                    "properties": { "queued_words": { "type": "integer" } },
                    "required": ["queued_words"]
                },
                "OutputRawResponse": {
                    "type": "object",
                    "properties": {
                        "units": { "type": "object", "additionalProperties": { "type": "array", "items": { "type": "integer" } } }
                    },
                    "required": ["units"]
                },
                "OutputTextResponse": {
                    "type": "object",
                    "properties": {
                        "units": { "type": "object", "additionalProperties": { "type": "string" } }
                    },
                    "required": ["units"]
                }
            }
        }
    }))
}

#[get("/docs")]
fn swagger_ui() -> RawHtml<&'static str> {
    RawHtml(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <title>misch API docs</title>
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css" />
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    window.ui = SwaggerUIBundle({
      url: '/api/v1/openapi.json',
      dom_id: '#swagger-ui',
      deepLinking: true
    });
  </script>
</body>
</html>
"#,
    )
}

pub fn build_rocket() -> rocket::Rocket<rocket::Build> {
    rocket::build()
        .manage(Mutex::new(HashMap::<Uuid, Session>::new()))
        .mount(
            "/api/v1",
            routes![
                create_session,
                run_session,
                get_session,
                append_input_text,
                append_input_raw,
                get_output_raw,
                get_output_text,
                delete_session,
                openapi_spec,
                swagger_ui,
            ],
        )
}

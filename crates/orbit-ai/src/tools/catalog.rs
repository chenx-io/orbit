//! v1 tool catalog: names, authorization tiers, model-facing descriptions and parameter schemas.
//!
//! For **strongly typed entities** (request definitions / scenarios / suites / data sets) the parameter schema
//! deliberately only gives `type: object` + key field notes; the model fills the rest from the project model,
//! and [`super::validate`] is the real gate before persistence (validation errors are fed back to the model to retry).
//! This avoids hand-writing and maintaining a huge JSON Schema in Rust.

use serde_json::{json, Value};

use super::{ToolKind, ToolSpec};

/// Read tools (execute directly, no confirmation).
pub const READ_TOOLS: &[&str] = &[
    "list_collections",
    "list_requests",
    "get_request",
    "list_environments",
    "list_models",
    "list_scenarios",
    "list_action_templates",
    "list_data_sources",
    "recent_history",
    "list_reports",
];

/// Write tools (propose and await confirmation by default; persisted directly once the session enables "auto-apply").
pub const WRITE_TOOLS: &[&str] = &[
    "create_collection",
    "create_request",
    "update_request",
    "update_action",
    "insert_action",
    "delete_action",
    "move_action",
    "save_action_template",
    "delete_action_template",
    "create_scenario_folder",
    "create_scenario",
    "update_scenario",
    "create_suite",
    "create_data_set",
];

/// Execute tools (always require confirmation; load tests also confirm the parameter caps).
pub const EXECUTE_TOOLS: &[&str] = &["run_request", "run_scenario", "run_load_test"];

/// Plan tools (visible only in Plan mode; produce no side effects).
pub const PLAN_TOOLS: &[&str] = &["present_plan"];

/// The **complete** shape description of an action (an element of the pre/post action lists).
///
/// Database action fields must be written in full: earlier prompts mentioned only `{type,datasource,sql}`,
/// so when editing an action the model dropped the whole `command/args/target/extract_var/columns/retry` block
/// (especially fatal when the action list has whole-array replacement semantics).
const ACTION_SHAPE: &str = "Action shapes (only these four): \
script {type:\"script\",code:\"…\",name?:\"display name\",enabled?:true};\
database query {type:\"db\",datasource:\"datasource id\",sql?:\"SQL\",command?:\"REDIS command\",args?:[\"…\"],\
target?:{type:\"scalar\"|\"row\"|\"column\"|\"row_count\"|\"path\",row?:0,column?:\"column name\",path?:\"a.b\"},\
extract_var?:\"variable name to write\",columns?:[{column:\"column name\",var:\"variable name\"}],row?:0,\
retry?:{interval_ms?:200,max_attempts?:3,timeout_ms?:5000},name?,enabled?};\
script library ref {type:\"ref\",library_id:\"library item id\",name?:\"optional alias\",enabled?} (edit the item -> all references take effect immediately);\
built-in interpolation node {type:\"interpolate\"} (always present and unique; cannot be added / edited / deleted / moved).";

/// Indexing and timing notes for action lists (shared by the four action-level tools).
const ACTION_LIST_SHAPE: &str = "list=\"pre\" = pre-request action list (one ordered list, order = execution order, containing the built-in interpolation node: \
actions before it = **pre-interpolation**, after it = **post-interpolation**), list=\"post\" = post-response action list (runs after the response arrives).\
index **starts at 0** and must be an index into the **current** list - read first with get_request (it returns the exact execution order,\
including the built-in interpolation node); indices shift as actions are added/removed, so call get_request again before editing the next one.";

/// Shape description of a script library item.
const ACTION_TEMPLATE_SHAPE: &str =
    "Library item object: name (display name; used as the action display name after expansion), \
description? (notes), action (**must** be {type:\"script\",…} or {type:\"db\",…}:\
it cannot be the built-in interpolation node, nor reference another library item).";

/// List of **protocol-specific fields** outside HTTP (common fields are in [`request_shape`] and [`ACTION_SHAPE`]).
///
/// Earlier prompts only said "websocket/grpc/tcp/udp/sse/graphql need protocol and url",
/// so the model could only guess field names for these protocols (gRPC's `service`/`streaming`, WS's `messages[]`...);
/// wrong guesses are swallowed by the untagged union, reporting only the useless "data did not match any variant".
/// All field names and values are copied from the authoritative structs in `orbit-data::model`; do not change them from memory.
const PROTOCOL_SHAPES: &str = "\
Per-protocol fields (all also have id/name/protocol/url + preActions/postActions; actions work exactly as with HTTP):\
grpc: service (service name), method (rpc name), packageName, serviceName, inputType, outputType,\
message (request body JSON string), messageTemplate, messageFormat / responseFormat,\
streaming (`server_streaming`|`client_streaming`|`bidirectional`), metadata[{key,value,enabled}], headers[], auth;\
websocket: headers[], closeAfter (close after that many messages),\
messages[{payload,payloadType:`text`|`base64`|`hex`,messageType:`text`|`binary`,preScript,postScript,waitMs}];\
sse: headers[], maxEvents (max events to receive);\
tcp: payload (single segment), payloadType, framing{mode:`delimiter`|`fixed`|`read_until_close`|`length_prefix`,delimiter?,fixedLen?,bigEndian?},\
messages[{payload,payloadType,preScript,postScript,waitMs}];\
udp: payload, payloadType, messages[...];\
graphql: query (GraphQL document), variables (JSON string), operationName, headers[], auth;\
http additionally supports: responses[{id,name,status,contentType,body,description?,schema?}] (response examples/models),\
cookies[{name,value,domain,path,enabled}], bodyByMode{\"json\":\"…\"} (request body stored per mode).\
A wrong field name is silently ignored or only reported as 'shape mismatch' - when unsure, read a similar request with get_request and copy it.";

/// Key field notes for a request definition object (a "shape hint" for the model).
///
/// The "minimal valid shape" at the end is deliberate: the model follows a **concrete example** far better than a field list,
/// and without it the most common failure is omitting `method` (then it reports "an HTTP request needs at least method and url").
fn request_shape() -> String {
    format!(
        "Request definition object. Common fields: \
name (request name), protocol (defaults to http; one of http/websocket/grpc/tcp/udp/sse/graphql),\
method, url, headers[{{key,value,enabled}}], queryParams[], pathParams[],\
body (string), bodyMode (none/json/xml/form-data/x-www-form-urlencoded/raw/binary),\
contentType, formParams[], auth{{type:none|bearer|basic|api-key,token/username/password/key/value,addTo}},\
preActions[...] (**one ordered list**, order = execution order), postActions[...] (runs after the response arrives),\
assertions[{{...}}] (assertions). Script and database actions share the same pm.* API.\
{PROTOCOL_SHAPES}\
{ACTION_SHAPE}\
**{{type:\"interpolate\"}}** is the built-in 'interpolation' node (turns the request template into the final payload; cannot be deleted/edited):\
actions **before it** = **pre-interpolation** (scripts/DB queries write variables first, then variable interpolation runs; variables set here are usable in this very request),\
actions **after it** = **post-interpolation** (they see the final payload; good for signing/encryption; edits to pm.request.* are exactly what gets sent).\
prereqScript / postreqScript are **legacy single-script fields** (folded into the action list: prereqScript means post-interpolation);\
new configurations must use preActions / postActions, and **a single action is edited with update_action / insert_action / delete_action / move_action**\
(addressed by index; do not resend the whole array - that loses fields easily).\
URL and script text may use project variable syntax {{{{var}}}} or dynamic values {{{{$category.method}}}}.\
Minimal valid shape of an HTTP request (always provide the required fields exactly like this):\
{{\"name\":\"login\",\"method\":\"POST\",\"url\":\"https://api.example.com/login\",\"headers\":[{{\"key\":\"Content-Type\",\"value\":\"application/json\",\"enabled\":true}}],\"body\":\"{{\\\"user\\\":\\\"{{{{username}}}}\\\"}}\",\"bodyMode\":\"json\",\"preActions\":[{{\"type\":\"script\",\"code\":\"pm.environment.set('t', '1')\"}},{{\"type\":\"interpolate\"}}],\"assertions\":[{{\"type\":\"status\",\"value\":200}}]}}"
    )
}

/// Key field notes for a scenario object.
const SCENARIO_SHAPE: &str = "Automation scenario object. Fields: name, description, priority (p0-p3), \
envId, folderId (owning folder, default = root), iterations, onError (stop/continue/next-loop),\
dataSetId, useDataSet, steps[]: each step {type, name, disabled?, ...}, executed in order. Step types:\
request{requestId, extractPath?, extractVar?, extractType?},\
loop{count, children[]}, condition{expr, children[], elseChildren[]},\
wait{ms}, group{children[]}, setvar{varKey, varValue}.\
(id is auto-generated when omitted; requestId must be an existing request id - get it with list_requests first;\
extractType is one of jsonpath (default) / jmespath / header / regex / cookie, and extractPath's meaning follows accordingly.)";

/// Key field notes for a suite object.
const SUITE_SHAPE: &str = "Test suite object. Fields: name, description, envId, \
runMode (serial/parallel), concurrency (1-10, effective with parallel), memberIds (array of scenario ids, required).";

/// Key field notes for a data set object.
const DATASET_SHAPE: &str = "CSV data set object. Fields: name, csv (CSV text whose first row is the header), \
mode (sequential/random/shuffle, default sequential). columns and rowCount are derived automatically.";

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    })
}

fn string_prop(desc: &str) -> Value {
    json!({ "type": "string", "description": desc })
}

fn number_prop(desc: &str) -> Value {
    json!({ "type": "number", "description": desc })
}

fn entity_prop(desc: &str) -> Value {
    json!({ "type": "object", "description": desc, "additionalProperties": true })
}

/// Return all tools (the order is the display order in the prompt).
pub fn all_tools() -> Vec<ToolSpec> {
    vec![
        // ─── read ───
        ToolSpec {
            name: "list_collections".into(),
            kind: ToolKind::Read,
            description: "List the collection tree of the current workspace (collection -> folder -> request id/name/method/URL summary).\
Use it to determine collectionId and parentId before creating or editing a request.".into(),
            parameters: object_schema(json!({}), &[]),
        },
        ToolSpec {
            name: "list_requests".into(),
            kind: ToolKind::Read,
            description: "List request summaries (id, name, protocol, method, URL). Can be filtered by collectionId.".into(),
            parameters: object_schema(
                json!({ "collectionId": string_prop("collection id; default = all collections") }),
                &[],
            ),
        },
        ToolSpec {
            name: "get_request".into(),
            kind: ToolKind::Read,
            description: "Read the full definition of a request (including action lists and assertions): preActions / postActions are returned in\
**execution order** (the built-in interpolation node is in place and legacy single-script fields are folded in), and index in the action-level tools refers to this list.\
Call it before editing a request or an action.".into(),
            parameters: object_schema(json!({ "requestId": string_prop("request id") }), &["requestId"]),
        },
        ToolSpec {
            name: "list_environments".into(),
            kind: ToolKind::Read,
            description: "List environments with their variable names and secret names (**no secret values**).".into(),
            parameters: object_schema(json!({}), &[]),
        },
        ToolSpec {
            name: "list_models".into(),
            kind: ToolKind::Read,
            description: "List data models (names and field summaries) to align request/response structures when generating requests.".into(),
            parameters: object_schema(json!({}), &[]),
        },
        ToolSpec {
            name: "list_scenarios".into(),
            kind: ToolKind::Read,
            description: "List automation scenarios (id, name, priority, folder, step count) and suites.".into(),
            parameters: object_schema(json!({}), &[]),
        },
        ToolSpec {
            name: "list_action_templates".into(),
            kind: ToolKind::Read,
            description: format!(
                "List the script library items of the current workspace (id, name, notes and full action).\
Call it to get real ids before reusing an item via `{{type:\"ref\",library_id:\"…\"}}` (editing the item -> all references take effect immediately).\
{ACTION_TEMPLATE_SHAPE}"
            ),
            parameters: object_schema(json!({}), &[]),
        },
        ToolSpec {
            name: "list_data_sources".into(),
            kind: ToolKind::Read,
            description: "List available data sources (id, name, type; **no credentials**).\
The datasource of a database action must be one of these ids - a made-up id only fails at runtime.".into(),
            parameters: object_schema(json!({}), &[]),
        },
        ToolSpec {
            name: "recent_history".into(),
            kind: ToolKind::Read,
            description: "Read recently sent request records (method, URL, status code, duration) to explain failures or generate regression scenarios.".into(),
            parameters: object_schema(
                json!({ "limit": number_prop("number of entries to return, default 20, max 100") }),
                &[],
            ),
        },
        ToolSpec {
            name: "list_reports".into(),
            kind: ToolKind::Read,
            description: "List recent automation run and load test report summaries (pass rate, error count, P95).".into(),
            parameters: object_schema(
                json!({ "limit": number_prop("number of entries to return, default 10, max 50") }),
                &[],
            ),
        },
        // ─── write (proposal) ───
        ToolSpec {
            name: "create_collection".into(),
            kind: ToolKind::Write,
            description: "Create a collection (a request group).".into(),
            parameters: object_schema(
                json!({
                    "name": string_prop("collection name"),
                    "description": string_prop("collection notes (optional)"),
                }),
                &["name"],
            ),
        },
        ToolSpec {
            name: "create_request".into(),
            kind: ToolKind::Write,
            description: format!(
                "Create a request in the given collection (pre/post actions and assertions can be generated at the same time). {}",
                request_shape()
            ),
            parameters: object_schema(
                json!({
                    "collectionId": string_prop("target collection id (get it with list_collections first)"),
                    "parentId": string_prop("target folder id; default = collection root"),
                    "request": entity_prop(&request_shape()),
                }),
                &["collectionId", "request"],
            ),
        },
        ToolSpec {
            name: "update_request".into(),
            kind: ToolKind::Write,
            description: "Edit the **fields** of an existing request. patch is a partial field set (JSON Merge Patch: null deletes the field);\
it is merged with the current definition before validation, so only the fields to change are needed.\
To edit a **single action** (e.g. the SQL of a database action or the code of a script) use update_action - doing it here means resending the whole array\
preActions/postActions, which loses fields easily.".into(),
            parameters: object_schema(
                json!({
                    "requestId": string_prop("request id"),
                    "patch": entity_prop("fields to override, e.g. {\"url\":\"https://new\",\"method\":\"POST\"}"),
                }),
                &["requestId", "patch"],
            ),
        },
        ToolSpec {
            name: "update_action".into(),
            kind: ToolKind::Write,
            description: format!(
                "Edit **one** action of a request (only the given fields change, the rest keep their values).\
Typical use: change the SQL of a database action in the pre-request list - patch gives {{\"sql\":\"SELECT …\"}}.\
{ACTION_LIST_SHAPE} An action type cannot be switched via patch (to change it use delete_action + insert_action).\
{ACTION_SHAPE}"
            ),
            parameters: object_schema(
                json!({
                    "requestId": string_prop("request id"),
                    "list": string_prop("action list: \"pre\" (pre-request, includes the built-in interpolation node) or \"post\" (post-response)"),
                    "index": number_prop("action index (0-based, includes the built-in interpolation node; from get_request)"),
                    "patch": entity_prop("fields to override, e.g. {\"sql\":\"SELECT id FROM users\"} or {\"code\":\"sign()\"}"),
                }),
                &["requestId", "list", "index", "patch"],
            ),
        },
        ToolSpec {
            name: "insert_action".into(),
            kind: ToolKind::Write,
            description: format!(
                "Insert an action into a request's action list (omit index = append to the end;\
appending in the pre-request list means post-interpolation; to place it pre-interpolation point index before the built-in interpolation node).\
{ACTION_LIST_SHAPE} {ACTION_SHAPE}"
            ),
            parameters: object_schema(
                json!({
                    "requestId": string_prop("request id"),
                    "list": string_prop("action list: \"pre\" or \"post\""),
                    "index": number_prop("index of the insertion position (0-based; omit = append to the end)"),
                    "action": entity_prop("action to insert, e.g. {\"type\":\"db\",\"datasource\":\"ds-1\",\"sql\":\"SELECT 1\"}"),
                }),
                &["requestId", "list", "action"],
            ),
        },
        ToolSpec {
            name: "delete_action".into(),
            kind: ToolKind::Write,
            description: format!(
                "Delete one action from a request's action list (the built-in interpolation node cannot be deleted). {ACTION_LIST_SHAPE}"
            ),
            parameters: object_schema(
                json!({
                    "requestId": string_prop("request id"),
                    "list": string_prop("action list: \"pre\" or \"post\""),
                    "index": number_prop("index of the action to delete (0-based; from get_request)"),
                }),
                &["requestId", "list", "index"],
            ),
        },
        ToolSpec {
            name: "move_action".into(),
            kind: ToolKind::Write,
            description: format!(
                "Move an action within the same action list (the built-in interpolation node cannot be moved).\
Crossing the built-in interpolation node changes the timing: moving before it = pre-interpolation, after it = post-interpolation. {ACTION_LIST_SHAPE}"
            ),
            parameters: object_schema(
                json!({
                    "requestId": string_prop("request id"),
                    "list": string_prop("action list: \"pre\" or \"post\""),
                    "from": number_prop("source index (0-based)"),
                    "to": number_prop("target index (0-based)"),
                }),
                &["requestId", "list", "from", "to"],
            ),
        },
        ToolSpec {
            name: "save_action_template".into(),
            kind: ToolKind::Write,
            description: format!(
                "Create or update a **script library item** (a workspace-level reusable action). Omit templateId = create (returns the new id);\
passing it = update that item. An update **immediately affects every request referencing it**: confirm the blast radius with list_action_templates / get_request first.\
{ACTION_TEMPLATE_SHAPE}"
            ),
            parameters: object_schema(
                json!({
                    "templateId": string_prop("library item id to update (omit = create)"),
                    "name": string_prop("library item name (e.g. \"compute signature\")"),
                    "description": string_prop("notes (optional)"),
                    "action": entity_prop("library item action: {\"type\":\"script\",\"code\":\"…\"} or {\"type\":\"db\",\"datasource\":\"…\",\"sql\":\"…\"}"),
                }),
                &["name", "action"],
            ),
        },
        ToolSpec {
            name: "delete_action_template".into(),
            kind: ToolKind::Write,
            description: "Delete a script library item. **Requests referencing it are not cleaned up automatically**, leaving dangling refs\
(an error is logged at execution time but the request is not interrupted) - confirm the blast radius before deleting, and update it with save_action_template instead when appropriate.".into(),
            parameters: object_schema(
                json!({ "templateId": string_prop("library item id") }),
                &["templateId"],
            ),
        },
        ToolSpec {
            name: "create_scenario_folder".into(),
            kind: ToolKind::Write,
            description: "Create a scenario folder (to organize automation scenarios).".into(),
            parameters: object_schema(
                json!({
                    "name": string_prop("folder name"),
                    "parentId": string_prop("parent folder id; default = root"),
                }),
                &["name"],
            ),
        },
        ToolSpec {
            name: "create_scenario".into(),
            kind: ToolKind::Write,
            description: format!("Create an automation scenario. {SCENARIO_SHAPE}"),
            parameters: object_schema(
                json!({
                    "scenario": entity_prop(SCENARIO_SHAPE),
                    "folderId": string_prop("owning folder id (optional)"),
                }),
                &["scenario"],
            ),
        },
        ToolSpec {
            name: "update_scenario".into(),
            kind: ToolKind::Write,
            description: "Edit an existing scenario (patch has JSON Merge Patch semantics).".into(),
            parameters: object_schema(
                json!({
                    "scenarioId": string_prop("scenario id"),
                    "patch": entity_prop("fields to override, e.g. {\"steps\":[...]}"),
                }),
                &["scenarioId", "patch"],
            ),
        },
        ToolSpec {
            name: "create_suite".into(),
            kind: ToolKind::Write,
            description: format!("Create a test suite (runs several scenarios together). {SUITE_SHAPE}"),
            parameters: object_schema(
                json!({ "suite": entity_prop(SUITE_SHAPE) }),
                &["suite"],
            ),
        },
        ToolSpec {
            name: "create_data_set".into(),
            kind: ToolKind::Write,
            description: format!("Create a CSV data-driven data set. {DATASET_SHAPE}"),
            parameters: object_schema(
                json!({ "dataSet": entity_prop(DATASET_SHAPE) }),
                &["dataSet"],
            ),
        },
        // ─── execute (confirm each time) ───
        ToolSpec {
            name: "run_request".into(),
            kind: ToolKind::Execute,
            description: "Actually send an existing request (goes through the engine for variable interpolation, pre/post actions and assertions)\
and returns status code, duration, response body summary and assertion results.".into(),
            parameters: object_schema(
                json!({
                    "requestId": string_prop("request id"),
                    "environmentId": string_prop("environment id (optional; defaults to the active environment)"),
                }),
                &["requestId"],
            ),
        },
        ToolSpec {
            name: "run_scenario".into(),
            kind: ToolKind::Execute,
            description: "Run an automation scenario and return step results and the pass rate.".into(),
            parameters: object_schema(
                json!({
                    "scenarioId": string_prop("scenario id"),
                    "environmentId": string_prop("environment id (optional)"),
                }),
                &["scenarioId"],
            ),
        },
        ToolSpec {
            name: "run_load_test".into(),
            kind: ToolKind::Execute,
            description: "Start a load test against a request (it generates real load and requires user confirmation first).\
Returns average/P95/P99 latency, RPS and error rate.".into(),
            parameters: object_schema(
                json!({
                    "requestId": string_prop("request id under load test"),
                    "vus": number_prop("virtual users (1-500)"),
                    "durationSec": number_prop("duration in seconds (1-600)"),
                    "rampUpSec": number_prop("ramp-up time in seconds (optional)"),
                    "thresholds": {
                        "type": "array",
                        "description": "thresholds (optional), e.g. [{\"metric\":\"p95\",\"op\":\"lt\",\"value\":500}]",
                        "items": { "type": "object", "additionalProperties": true }
                    },
                }),
                &["requestId", "vus", "durationSec"],
            ),
        },
        // ─── plan (Plan mode only) ───
        ToolSpec {
            name: "present_plan".into(),
            kind: ToolKind::Plan,
            description: "Submit an implementation plan for user confirmation (the only Plan mode output).\
Investigate first, then call it; the plan can be revised repeatedly, each call overwrites the previous version.\
Titles and steps must be concrete (which requests/scenarios, what changes, how to verify); no fluff.\
After submitting, wait for the user to click 'start implementation' and do not start changing data on your own.".into(),
            parameters: object_schema(
                json!({
                    "title": string_prop("plan title, e.g. \"add automation scenarios for the login request\""),
                    "summary": string_prop("overall description (optional, 1-2 sentences)"),
                    "steps": {
                        "type": "array",
                        "description": "implementation steps (in execution order, 1-30 steps)",
                        "items": {
                            "type": "object",
                            "properties": {
                                "title": string_prop("what this step does"),
                                "detail": string_prop("additional notes (optional): affected objects and key parameters"),
                            },
                            "required": ["title"],
                            "additionalProperties": false,
                        },
                    },
                    "notes": {
                        "type": "array",
                        "description": "risks and caveats (optional)",
                        "items": { "type": "string" },
                    },
                }),
                &["title", "steps"],
            ),
        },
    ]
}

/// Look up a tool's tier by name.
pub fn kind_of(name: &str) -> Option<ToolKind> {
    all_tools()
        .into_iter()
        .find(|t| t.name == name)
        .map(|t| t.kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The single pre-request action list field names and the "built-in interpolation node" semantics must appear in the shape hint -
    /// the AI can only write from the prompt, and one omission prevents it from expressing "create data pre-interpolation" or "sign post-interpolation".
    #[test]
    fn request_shape_documents_single_list_with_interpolate_node() {
        let shape = request_shape();
        for token in [
            "preActions",
            "postActions",
            "prereqScript",
            "\"interpolate\"",
            "pre-interpolation",
            "post-interpolation",
        ] {
            assert!(
                shape.contains(token),
                "request shape hint is missing `{token}`: the AI will not be able to write that field"
            );
        }
    }

    /// Database action fields must be **written in full**: when only `{type,datasource,sql}` is mentioned, the model drops
    /// the whole command/args/target/extract_var/columns/retry block (one of the measured root causes of field loss).
    #[test]
    fn action_shape_documents_full_db_action_and_library_ref() {
        for token in [
            "datasource",
            "sql",
            "command",
            "args",
            "target",
            "extract_var",
            "columns",
            "row",
            "retry",
            "ref",
            "library_id",
            "script",
        ] {
            assert!(
                ACTION_SHAPE.contains(token),
                "action shape hint is missing `{token}`: the AI will drop that field when editing a database action"
            );
        }
        // the request shape hint must also expose the action shape (both must come from one source, no duplicate copies)
        assert!(request_shape().contains(ACTION_SHAPE));
        // the library item shape and the action list index notes must be readable as well
        assert!(ACTION_TEMPLATE_SHAPE.contains("cannot be the built-in interpolation node"));
        assert!(ACTION_LIST_SHAPE.contains("starts at 0"));
    }

    /// Non-HTTP protocol fields must be documented in the shape hint: with only "protocol and url are needed",
    /// the model can only guess field names when editing gRPC / WS / SSE / TCP / UDP / GraphQL requests.
    #[test]
    fn protocol_shapes_document_non_http_fields() {
        for token in [
            "grpc",
            "serviceName",
            "streaming",
            "server_streaming",
            "messages",
            "closeAfter",
            "maxEvents",
            "framing",
            "read_until_close",
            "payloadType",
            "graphql",
            "operationName",
            "variables",
            "responses",
            "cookies",
        ] {
            assert!(
                PROTOCOL_SHAPES.contains(token),
                "protocol field hint is missing `{token}`: the AI cannot correctly rewrite requests of that protocol"
            );
        }
        assert!(
            request_shape().contains(PROTOCOL_SHAPES),
            "the request shape hint must include the protocol field list (both from one source)"
        );
    }

    /// The scenario shape must cover step-level details (otherwise the model cannot write "skip a step" or "extract by header").
    #[test]
    fn scenario_shape_documents_step_details() {
        for token in [
            "disabled",
            "folderId",
            "extractType",
            "jmespath",
            "elseChildren",
            "varKey",
            "onError",
        ] {
            assert!(
                SCENARIO_SHAPE.contains(token),
                "scenario shape is missing `{token}`"
            );
        }
    }

    /// The action-level tools must exist and be visible: this is the only reliable way to "change the SQL of one database action".
    #[test]
    fn action_level_tools_are_registered() {
        for name in [
            "update_action",
            "insert_action",
            "delete_action",
            "move_action",
            "list_action_templates",
            "save_action_template",
            "delete_action_template",
            "list_data_sources",
        ] {
            assert!(kind_of(name).is_some(), "missing tool {name}");
        }
        let update = all_tools()
            .into_iter()
            .find(|t| t.name == "update_action")
            .expect("update_action must exist");
        let required: Vec<&str> = update.parameters["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(required, vec!["requestId", "list", "index", "patch"]);
    }

    #[test]
    fn every_tool_has_unique_name_and_object_schema() {
        let tools = all_tools();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        let total = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), total, "tool names must be unique");
        for t in &tools {
            assert_eq!(
                t.parameters["type"], "object",
                "{} parameters should be object",
                t.name
            );
        }
    }

    #[test]
    fn catalog_matches_kind_constants() {
        for name in READ_TOOLS {
            assert_eq!(kind_of(name), Some(ToolKind::Read), "{name}");
        }
        for name in WRITE_TOOLS {
            assert_eq!(kind_of(name), Some(ToolKind::Write), "{name}");
        }
        for name in EXECUTE_TOOLS {
            assert_eq!(kind_of(name), Some(ToolKind::Execute), "{name}");
        }
        for name in PLAN_TOOLS {
            assert_eq!(kind_of(name), Some(ToolKind::Plan), "{name}");
        }
        assert_eq!(
            READ_TOOLS.len() + WRITE_TOOLS.len() + EXECUTE_TOOLS.len() + PLAN_TOOLS.len(),
            all_tools().len(),
            "every tool must belong to exactly one tier"
        );
    }

    #[test]
    fn plan_tool_is_only_visible_in_plan_mode() {
        use crate::mode::AiMode;
        let plan_tool = all_tools()
            .into_iter()
            .find(|t| t.name == "present_plan")
            .expect("present_plan must exist");
        assert!(AiMode::Plan.allows(plan_tool.kind));
        assert!(
            !AiMode::Agent.allows(plan_tool.kind),
            "Agent mode should no longer plan"
        );
        assert!(!AiMode::Ask.allows(plan_tool.kind));
    }

    #[test]
    fn required_params_are_declared() {
        let tools = all_tools();
        let get = tools.iter().find(|t| t.name == "get_request").unwrap();
        assert_eq!(get.parameters["required"][0], "requestId");
        let run = tools.iter().find(|t| t.name == "run_load_test").unwrap();
        let required = run.parameters["required"].as_array().unwrap();
        assert_eq!(required.len(), 3);
    }

    #[test]
    fn unknown_tool_has_no_kind() {
        assert_eq!(kind_of("teleport"), None);
    }
}

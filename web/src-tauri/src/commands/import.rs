//! Import commands (Tauri channel) -- all go through orbit-config::exchange; this file only bridges

use orbit_config::importer_openapi::ImportParseResult;

#[tauri::command]
pub fn parse_import(format: String, input: String) -> Result<ImportParseResult, String> {
    orbit_config::exchange::import_endpoints(&format, &input).map_err(|e| e.to_string())
}

// ─── Single file read (for drag-drop) ────────────────

#[tauri::command]
pub fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read file: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Postman v2.1: prerequest / test scripts in item.event are extracted into pre_script / post_script.
    #[test]
    fn test_parse_postman_scripts() {
        let collection = r##"{
          "info": {"name": "Example", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},
          "item": [
            {
              "name": "Login",
              "event": [
                {
                  "listen": "prerequest",
                  "script": {"type": "text/javascript", "exec": [
                    "pm.environment.set(\"token\", \"abc123\");",
                    "console.log(\"pre done\");"
                  ]}
                },
                {
                  "listen": "test",
                  "script": {"type": "text/javascript", "exec": [
                    "pm.test(\"status is 200\", function () {",
                    "  pm.response.to.have.status(200);",
                    "});"
                  ]}
                }
              ],
              "request": {"method": "POST", "url": {"raw": "https://api.example.com/login"}},
              "response": []
            }
          ]
        }"##;
        let r = parse_import("postman".to_string(), collection.to_string()).unwrap();
        assert_eq!(r.endpoints.len(), 1);
        let ep = &r.endpoints[0];
        assert!(
            ep.pre_script.contains("pm.environment.set"),
            "pre_script: {}",
            ep.pre_script
        );
        assert!(
            ep.pre_script.contains("console.log(\"pre done\")"),
            "pre_script multiline join: {}",
            ep.pre_script
        );
        assert!(
            ep.post_script.contains("pm.test(\"status is 200\""),
            "post_script: {}",
            ep.post_script
        );
    }

    /// Real user scenario: a collection exported by official Postman (script contains extra packages/requests fields,
    /// single-line exec, no body/header, GET request) -- scripts must be extracted.
    #[test]
    fn test_parse_postman_user_export_preserves_scripts() {
        let collection = r##"{
          "info": {
            "_postman_id": "3fe15860-dfc9-41c3-8725-b4efc9b27b94",
            "name": "New Collection",
            "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
            "_exporter_id": "12154110",
            "_collection_link": "https://go.postman.co/collection/12154110-3fe15860-dfc9-41c3-8725-b4efc9b27b94?source=collection_link"
          },
          "item": [
            {
              "name": "aaaa",
              "event": [
                {
                  "listen": "prerequest",
                  "script": {
                    "exec": [
                      "var aa = pm.variables.get('abc');"
                    ],
                    "type": "text/javascript",
                    "packages": {},
                    "requests": {}
                  }
                },
                {
                  "listen": "test",
                  "script": {
                    "exec": [
                      "pm.variables.set('aaa', \"bbb\");"
                    ],
                    "type": "text/javascript",
                    "packages": {},
                    "requests": {}
                  }
                }
              ],
              "request": {
                "method": "GET",
                "header": [],
                "url": {
                  "raw": "https://echo.apifox.com/get",
                  "protocol": "https",
                  "host": ["echo", "apifox", "com"],
                  "path": ["get"]
                }
              },
              "response": []
            }
          ]
        }"##;
        let r = parse_import("postman".to_string(), collection.to_string()).unwrap();
        assert_eq!(r.endpoints.len(), 1, "should parse out 1 endpoint");
        let ep = &r.endpoints[0];
        assert_eq!(ep.name, "aaaa");
        assert_eq!(ep.method, "GET");
        assert_eq!(ep.url, "https://echo.apifox.com/get");
        assert!(
            ep.pre_script.contains("pm.variables.get('abc')"),
            "pre_script lost, actual: {:?}",
            ep.pre_script
        );
        assert!(
            ep.post_script.contains("pm.variables.set('aaa'"),
            "post_script lost, actual: {:?}",
            ep.post_script
        );

        // End to end: call the Tauri command parse_import directly; the serialized JSON must contain pre_script / post_script
        // (the ImportDialog frontend reads exactly these field names of this JSON)
        let result = parse_import("postman".to_string(), collection.to_string()).unwrap();
        let json = serde_json::to_string(&result).unwrap();
        assert!(
            json.contains("pre_script"),
            "Tauri-returned JSON is missing the pre_script field, actual: {}",
            json
        );
        assert!(
            json.contains("pm.variables.get"),
            "Tauri-returned JSON is missing the script content, actual: {}",
            json
        );
    }

    #[test]
    fn test_parse_oas3_security_body_responses() {
        let spec = r##"{
          "openapi": "3.0.0",
          "info": {"title": "Test", "version": "1.0"},
          "servers": [{"url": "https://api.example.com"}],
          "components": {
            "securitySchemes": {"bearerAuth": {"type": "http", "scheme": "bearer"}},
            "schemas": {
              "User": {"type": "object", "required": ["id","name"], "properties": {"id": {"type": "integer"}, "name": {"type": "string"}}}
            }
          },
          "security": [{"bearerAuth": []}],
          "paths": {
            "/users": {
              "get": {"summary": "List users", "responses": {"200": {"description": "ok", "content": {"application/json": {"schema": {"type": "array", "items": {"$ref": "#/components/schemas/User"}}}}}}},
              "post": {"summary": "Create user", "requestBody": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/User"}}}}, "responses": {"201": {"description": "created", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/User"}}}}}}
            }
          }
        }"##;
        let r = orbit_config::importer_openapi::parse_openapi(spec).unwrap();
        assert_eq!(r.endpoints.len(), 2);
        let get = &r.endpoints[0];
        assert_eq!(get.method, "GET");
        assert_eq!(
            get.content_type, "",
            "GET without body: content_type must be empty"
        );
        assert_eq!(get.body, "");
        assert_eq!(
            get.auth_type, "bearer",
            "global security should map to bearer"
        );
        assert_eq!(get.model_ref, "");
        assert_eq!(get.responses.len(), 1);
        assert!(
            get.responses[0].body.contains("\"id\""),
            "response example should resolve User schema: {}",
            get.responses[0].body
        );
        let post = &r.endpoints[1];
        assert_eq!(post.auth_type, "bearer");
        assert_eq!(
            post.model_ref, "User",
            "requestBody schema $ref should become model name"
        );
        assert_eq!(post.content_type, "application/json");
        assert!(
            post.body.contains("\"id\""),
            "body example should resolve User schema: {}",
            post.body
        );
        assert_eq!(post.responses.len(), 1);
        assert_eq!(post.responses[0].status, 201);
    }

    #[test]
    fn test_parse_sw2_security_api_key() {
        let spec = r##"{
          "swagger": "2.0",
          "info": {"title": "S", "version": "1.0"},
          "host": "api.example.com",
          "basePath": "/v1",
          "schemes": ["https"],
          "securityDefinitions": {
            "api_key": {"type": "apiKey", "name": "X-API-Key", "in": "header"}
          },
          "security": [{"api_key": []}],
          "definitions": {
            "Pet": {"type": "object", "properties": {"id": {"type": "integer"}, "name": {"type": "string"}}}
          },
          "paths": {
            "/pets": {
              "post": {
                "summary": "add pet",
                "consumes": ["application/json"],
                "parameters": [{"name": "body", "in": "body", "schema": {"$ref": "#/definitions/Pet"}}],
                "responses": {"200": {"description": "ok", "schema": {"$ref": "#/definitions/Pet"}}}
              }
            }
          }
        }"##;
        let r = orbit_config::importer_openapi::parse_openapi(spec).unwrap();
        assert_eq!(r.endpoints.len(), 1);
        let ep = &r.endpoints[0];
        assert_eq!(
            ep.auth_type, "apikey",
            "Swagger2 apiKey should map to apikey"
        );
        assert_eq!(ep.auth_key_name, "X-API-Key");
        assert_eq!(ep.auth_add_to, "header");
        assert_eq!(ep.model_ref, "Pet");
        assert_eq!(ep.content_type, "application/json");
        assert!(
            ep.body.contains("\"id\""),
            "body example should contain Pet fields: {}",
            ep.body
        );
        assert_eq!(ep.responses.len(), 1);
        assert_eq!(ep.responses[0].status, 200);
    }

    #[test]
    fn test_parse_sw2_no_body_content_type_empty() {
        let spec = r##"{
          "swagger": "2.0",
          "info": {"title": "S", "version": "1.0"},
          "paths": {
            "/ping": {
              "get": {"summary": "ping", "responses": {"200": {"description": "ok"}}}
            }
          }
        }"##;
        let r = orbit_config::importer_openapi::parse_openapi(spec).unwrap();
        assert_eq!(r.endpoints.len(), 1);
        assert_eq!(
            r.endpoints[0].content_type, "",
            "no body param: content_type must be empty"
        );
        assert_eq!(r.endpoints[0].auth_type, "none");
    }
    #[test]
    fn test_parse_oas3_multiple_responses() {
        // One operation defines multiple status-code responses (200 / 201 / 400); all should be imported
        let spec = r##"{
          "openapi": "3.0.0",
          "info": {"title": "M", "version": "1.0"},
          "components": {
            "schemas": {
              "User": {"type": "object", "properties": {"id": {"type": "integer"}, "name": {"type": "string"}}},
              "Error": {"type": "object", "properties": {"code": {"type": "integer"}, "message": {"type": "string"}}}
            }
          },
          "paths": {
            "/users/{id}": {
              "put": {
                "summary": "update user",
                "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                "responses": {
                  "200": {"description": "ok", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/User"}}}},
                  "201": {"description": "created", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/User"}}}},
                  "400": {"description": "bad request", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/Error"}}}}
                }
              }
            }
          }
        }"##;
        let r = orbit_config::importer_openapi::parse_openapi(spec).unwrap();
        assert_eq!(r.endpoints.len(), 1);
        let ep = &r.endpoints[0];
        assert_eq!(
            ep.responses.len(),
            3,
            "should import the three responses 200/201/400"
        );
        let statuses: Vec<u16> = ep.responses.iter().map(|x| x.status).collect();
        assert_eq!(
            statuses,
            vec![200, 201, 400],
            "status codes should be complete and ordered"
        );
        let err = ep.responses.iter().find(|x| x.status == 400).unwrap();
        assert!(
            err.body.contains("\"code\""),
            "the 400 response should dereference the Error schema: {}",
            err.body
        );
        assert_eq!(ep.responses[0].name, "ok");
    }

    #[test]
    fn test_parse_oas3_sole_scheme_auto_apply() {
        // Only securitySchemes declared, no top-level security: the sole scheme should be applied automatically to all requests
        let spec = r##"{
          "openapi": "3.0.0",
          "info": {"title": "T", "version": "1.0"},
          "components": {
            "securitySchemes": {"bearerAuth": {"type": "http", "scheme": "bearer"}}
          },
          "paths": {
            "/users": {"get": {"summary": "List", "responses": {"200": {"description": "ok"}}}}
          }
        }"##;
        let r = orbit_config::importer_openapi::parse_openapi(spec).unwrap();
        assert_eq!(r.endpoints.len(), 1);
        assert_eq!(
            r.endpoints[0].auth_type, "bearer",
            "the sole securityScheme should be applied automatically"
        );
    }

    #[test]
    fn test_parse_sw2_bearer_api_key_definition() {
        // Real user scenario: Swagger2 has no http/bearer; BearerAuth is expressed with apiKey+Authorization/header
        // -> should map to bearer (not apikey)
        let spec = r##"{
          "swagger": "2.0",
          "info": {"title": "S", "version": "1.0"},
          "securityDefinitions": {
            "BearerAuth": {"type": "apiKey", "name": "Authorization", "in": "header"}
          },
          "security": [{"BearerAuth": []}],
          "paths": {
            "/channels/{id}": {
              "get": {
                "summary": "Channel details",
                "parameters": [{"name": "id", "in": "path", "required": true, "type": "integer"}],
                "produces": ["application/json"],
                "responses": {"200": {"description": "OK"}}
              }
            }
          }
        }"##;
        let r = orbit_config::importer_openapi::parse_openapi(spec).unwrap();
        assert_eq!(r.endpoints.len(), 1);
        assert_eq!(
            r.endpoints[0].auth_type, "bearer",
            "apiKey+Authorization/header should map to bearer"
        );
    }

    #[test]
    fn test_parse_sw2_bearer_def_missing_name_fallback() {
        // securityDefinitions lacks the definition, but the reference name contains bearer -> infer bearer by name
        let spec = r##"{
          "swagger": "2.0",
          "info": {"title": "S", "version": "1.0"},
          "securityDefinitions": {},
          "paths": {
            "/channels": {
              "delete": {
                "summary": "Delete channel",
                "security": [{"BearerAuth": []}],
                "responses": {"200": {"description": "OK"}}
              }
            }
          }
        }"##;
        let r = orbit_config::importer_openapi::parse_openapi(spec).unwrap();
        assert_eq!(r.endpoints.len(), 1);
        assert_eq!(
            r.endpoints[0].auth_type, "bearer",
            "a reference name containing bearer should be inferred by name"
        );
    }
}

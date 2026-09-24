use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
}

#[derive(Debug, Serialize)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
}

/// Simple schema field from frontend model definition
#[derive(Debug, Deserialize)]
struct SchemaFieldDef {
    name: String,
    #[serde(rename = "type")]
    field_type: String,
    required: Option<bool>,
}

#[tauri::command]
pub fn validate_response_against_model(
    response_body: String,
    model_fields: String,
) -> Result<ValidationResult, String> {
    // Parse the model fields JSON (frontend SchemaField[])
    let fields: Vec<SchemaFieldDef> = serde_json::from_str(&model_fields)
        .map_err(|e| format!("Invalid model fields JSON: {}", e))?;

    // Parse the response body
    let body_value: serde_json::Value = serde_json::from_str(&response_body)
        .map_err(|e| format!("Invalid response body JSON: {}", e))?;

    let mut errors = Vec::new();

    // Simple field presence and type check
    for field in &fields {
        if field.required.unwrap_or(false) {
            match &body_value {
                serde_json::Value::Object(map) => {
                    if !map.contains_key(&field.name) {
                        errors.push(ValidationError {
                            path: field.name.clone(),
                            message: format!("Required field '{}' is missing", field.name),
                        });
                    } else if let Some(val) = map.get(&field.name) {
                        // Check type
                        let type_ok = match field.field_type.as_str() {
                            "string" => val.is_string(),
                            "integer" => val.is_i64() || val.is_u64(),
                            "number" => val.is_number(),
                            "boolean" => val.is_boolean(),
                            "object" => val.is_object(),
                            "array" => val.is_array(),
                            "null" => val.is_null(),
                            _ => true, // unknown type, skip
                        };
                        if !type_ok {
                            errors.push(ValidationError {
                                path: field.name.clone(),
                                message: format!(
                                    "Field '{}' should be type '{}'",
                                    field.name, field.field_type
                                ),
                            });
                        }
                    }
                }
                _ => {
                    errors.push(ValidationError {
                        path: "root".into(),
                        message: "Expected a JSON object".into(),
                    });
                    break;
                }
            }
        }
    }

    Ok(ValidationResult {
        valid: errors.is_empty(),
        errors,
    })
}

#[tauri::command]
pub fn generate_dynamic_value(
    category: String,
    method: String,
    args: Option<String>,
) -> Result<String, String> {
    orbit_dynamic::generate(&category, &method, args.as_deref().unwrap_or(""))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resolve_dynamic_values(input: String) -> Result<String, String> {
    orbit_dynamic::resolve(&input).map_err(|e| e.to_string())
}

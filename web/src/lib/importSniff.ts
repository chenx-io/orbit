// Import format sniffing: auto-detects the source format from its content (JSON / YAML / XML / JS).
import yaml from "js-yaml";
import type { ImportFormat } from "@/components/import/FileDropZone";

/**
 * Auto-detect the import format from the content.
 * Detection order: OpenAPI/Swagger (openapi/swagger fields, JSON or YAML) → Postman (info+item)
 *   → HAR (log.entries, JSON only) → JMeter (jmeterTestPlan/HTTPSamplerProxy)
 *   → k6 (import http from 'k6 / http.get) → curl as the fallback.
 *
 * Note: OpenAPI/Swagger specs are often distributed as YAML (starting with openapi: 3.x / swagger: "2.0"),
 * so JSON-only detection is not enough; the YAML branch parses with js-yaml and then checks key fields to avoid false positives.
 */
export function sniffImportFormat(content: string): ImportFormat {
  const trimmed = content.trimStart();

  // JSON document
  if (trimmed.startsWith("{")) {
    try {
      const obj = JSON.parse(trimmed);
      if (obj && typeof obj === "object") {
        if (obj.openapi || obj.swagger) return "openapi";
        if (obj.info && Array.isArray(obj.item)) return "postman";
        if (obj.log && Array.isArray(obj.log.entries)) return "har";
      }
    } catch {
      /* not JSON, fall through to the rules below */
    }
  }

  // YAML document (OpenAPI/Swagger are often distributed as YAML; `---` document headers are supported);
  // parsing XML / JS scripts always fails, which the try/catch absorbs
  if (!trimmed.startsWith("<")) {
    try {
      const obj = yaml.load(trimmed) as
        Record<string, unknown> | null | undefined;
      if (obj && typeof obj === "object" && !Array.isArray(obj)) {
        if (obj.openapi || obj.swagger) return "openapi";
        if (obj.info && Array.isArray(obj.item)) return "postman";
      }
    } catch {
      /* not YAML, fall through to the rules below */
    }
  }

  if (
    trimmed.startsWith("<") &&
    (trimmed.includes("jmeterTestPlan") || trimmed.includes("HTTPSamplerProxy"))
  ) {
    return "jmeter";
  }

  if (
    trimmed.includes("import http from") ||
    trimmed.includes("require('k6") ||
    /http\.(get|post|put|delete|patch)\(/.test(trimmed)
  ) {
    return "k6";
  }

  return "curl";
}

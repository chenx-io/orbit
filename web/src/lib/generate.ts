import type { DataModel, SchemaField } from "@/data/types";
import { generateDynamicValue } from "@/lib/bridge/request";

// Sample data generation always goes through the backend orbit-dynamic engine (Tauri command / HTTP /api/dynamic),
// the same source as the {{fake(...)}} template used at execution time; the frontend no longer bundles faker.

/** Generate a string dynamic value */
function dvStr(
  category: string,
  method: string,
  args?: string,
): Promise<string> {
  return generateDynamicValue(category, method, args);
}

/** Generate a numeric dynamic value (the backend returns a string, converted by field type) */
async function dvNumber(
  category: string,
  method: string,
  min: number,
  max: number,
): Promise<number> {
  const s = await dvStr(category, method, `min=${min}, max=${max}`);
  const n = parseFloat(s);
  return Number.isNaN(n) ? min : n;
}

/** Generate a boolean */
async function dvBool(): Promise<boolean> {
  return (await dvStr("datatype", "boolean")) === "true";
}

/**
 * Infer more meaningful fake data from field-name keywords (better than pure randomness).
 * Returning null means no match, falling back to type-based generation.
 */
function byName(name: string): (() => Promise<unknown>) | null {
  const n = name.toLowerCase();
  if (/(^|_)id$|uuid|guid/.test(n)) {
    return /uuid|guid/.test(n)
      ? () => dvStr("string", "uuid")
      : () => dvNumber("number", "int", 1, 100000);
  }
  if (/email/.test(n)) return () => dvStr("internet", "email");
  if (/(user_?name|login|account)/.test(n))
    return () => dvStr("internet", "userName");
  if (/(first|last)?name/.test(n)) return () => dvStr("person", "fullName");
  if (/phone|mobile|tel/.test(n)) return () => dvStr("phone", "number");
  if (/password|passwd|pwd/.test(n))
    return () => dvStr("internet", "password", "length=12");
  if (/token|secret|apikey|api_key/.test(n))
    return () => dvStr("string", "alphanumeric", "length=32");
  if (/avatar|image|img|photo|picture/.test(n))
    return () => dvStr("image", "avatar");
  if (/url|link|href|website/.test(n)) return () => dvStr("internet", "url");
  if (/(^ip$|ip_?addr|ip_address)/.test(n))
    return () => dvStr("internet", "ip");
  if (/color|colour/.test(n)) return () => dvStr("color", "name");
  if (/(city|town)/.test(n)) return () => dvStr("location", "city");
  if (/(address|street)/.test(n)) return () => dvStr("location", "address");
  if (/(country|region)/.test(n)) return () => dvStr("location", "country");
  if (/(price|amount|cost|fee|total|sum|money|salary)/.test(n))
    return () => dvStr("commerce", "price");
  if (/(count|quantity|qty|stock|num|number)/.test(n))
    return () => dvNumber("number", "int", 1, 100);
  if (/(date|time|created|updated|at$)/.test(n))
    return () => dvStr("date", "pastRandom", "days=30");
  if (
    /description|remark|comment|note|summary|title|content|text|bio|message/.test(
      n,
    )
  )
    return () => dvStr("lorem", "sentence");
  if (/status/.test(n))
    return () =>
      dvStr("helpers", "arrayElement", "active, inactive, pending, disabled");
  if (/gender|sex/.test(n))
    return () => dvStr("helpers", "arrayElement", "male, female, other");
  if (/age/.test(n)) return () => dvNumber("number", "int", 18, 80);
  if (/code|sku/.test(n))
    return async () =>
      (await dvStr("string", "alphanumeric", "length=8")).toUpperCase();
  return null;
}

async function genField(field: SchemaField): Promise<unknown> {
  // 1) An existing example value takes precedence
  if (field.example !== undefined && field.example !== "") {
    try {
      return JSON.parse(field.example);
    } catch {
      return field.example;
    }
  }
  // 2) Take one enum entry
  if (field.enumValues && field.enumValues.length) {
    return dvStr("helpers", "arrayElement", field.enumValues.join(", "));
  }
  // 3) The format hint
  switch (field.format) {
    case "uuid":
      return dvStr("string", "uuid");
    case "email":
      return dvStr("internet", "email");
    case "uri":
    case "url":
      return dvStr("internet", "url");
    case "date-time":
    case "date":
      return dvStr("date", "pastRandom", "days=30");
  }
  // 4) Infer from the name
  const byNameFn = byName(field.name);
  if (byNameFn) return byNameFn();
  // 5) Generate by type
  switch (field.type) {
    case "string":
      return dvStr("lorem", "words", "count=2");
    case "integer":
      return dvNumber("number", "int", 0, 1000);
    case "number":
      return dvNumber("number", "float", 0, 1000);
    case "boolean":
      return dvBool();
    case "object": {
      const obj: Record<string, unknown> = {};
      for (const c of field.children ?? []) {
        obj[c.name] = await genField(c);
      }
      return obj;
    }
    case "array": {
      const item = (field.children ?? [])[0];
      const len = await dvNumber("number", "int", 1, 3);
      const out: unknown[] = [];
      for (let i = 0; i < len; i++) {
        out.push(item ? await genField(item) : await dvStr("lorem", "word"));
      }
      return out;
    }
    case "null":
      return null;
    default:
      return null;
  }
}

/** Generate a formatted sample JSON string from a data model. */
export async function generateModelBody(model: DataModel): Promise<string> {
  const obj: Record<string, unknown> = {};
  for (const f of model.fields) {
    obj[f.name] = await genField(f);
  }
  return JSON.stringify(obj, null, 2);
}

// Conversion helpers from a data model (DataModel) to a schema-like structure.
import type { DataModel } from "@/data/types";

/** Convert a DataModel (a fields structure) into a schema-like structure (properties), reused by the model viewer dialog. */
export function modelToSchema(m: DataModel): any {
  const properties: Record<string, any> = {};
  for (const f of m.fields) {
    const prop: any = {
      type: f.type,
      required: f.required,
      example: f.example,
      description: f.description,
    };
    if (f.children?.length) {
      prop.properties = modelToSchema({
        id: "",
        name: "",
        fields: f.children,
      } as DataModel).properties;
    }
    properties[f.name] = prop;
  }
  return { properties };
}

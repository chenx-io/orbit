// gRPC collection bridge layer: proto import / reflection import / message templates.
// The backend returns `{ packages: [...] }` or `{ error: string }`, consistent with the HTTP import shape.
import type { GrpcPackageNode } from "@/data/types";
import { apiPost, isTauri, tauriInvoke } from "./client";

/** gRPC descriptor returned by the backend: a package/service/rpc tree plus descriptor bytes (base64, for message templates) */
export interface GrpcDescriptor {
  packages: GrpcPackageNode[];
  descriptorFiles?: string[];
}

/** proto import: `files` is a list of { name, content } → a package/service/rpc tree */
export async function importProto(
  files: { name: string; content: string }[],
): Promise<GrpcDescriptor> {
  if (isTauri()) {
    return tauriInvoke<GrpcDescriptor>("grpc_import_proto", {
      files: files as unknown as Record<string, unknown>[],
    });
  }
  return apiPost<GrpcDescriptor>("/api/grpc/import-proto", { files });
}

/** Reflection import: target is the server address → a package/service/rpc tree */
export async function reflect(target: string): Promise<GrpcDescriptor> {
  if (isTauri()) {
    return tauriInvoke<GrpcDescriptor>("grpc_reflection", { target });
  }
  return apiPost<GrpcDescriptor>("/api/grpc/reflection", { target });
}

/**
 * Message template: `files` is FileDescriptorProto encoded bytes (base64),
 * `inputType` is the fully-qualified input message name → { template }.
 * Always returns `{ template }`: the Tauri command returns the template value directly and HTTP returns `{ template }`,
 * normalized here so callers can always read `.template`.
 */
export async function messageTemplate(
  files: string[],
  inputType: string,
): Promise<{ template: unknown }> {
  if (isTauri()) {
    const raw = await tauriInvoke<unknown>("grpc_message_template", {
      files,
      inputType,
    });
    return normalizeTemplate(raw);
  }
  const res = await apiPost<{ template: unknown } | unknown>(
    "/api/grpc/schema",
    {
      files,
      input_type: inputType,
    },
  );
  return normalizeTemplate(res);
}

/** Accepts both shapes: Tauri (returns the template value itself) and HTTP (returns `{ template }`) */
function normalizeTemplate(raw: unknown): { template: unknown } {
  if (raw && typeof raw === "object" && "template" in (raw as object)) {
    return { template: (raw as { template: unknown }).template };
  }
  return { template: raw };
}

/**
 * Message schema (model definition): `files` is FileDescriptorProto encoded bytes (base64),
 * `messageType` is the fully-qualified message name → `{ properties: { field: { type, ... } } }`,
 * with a structure matching the HTTP response schema, ready for ResponseSchemaDialog to render.
 */
export async function messageSchema(
  files: string[],
  messageType: string,
): Promise<{ properties: Record<string, unknown> }> {
  if (isTauri()) {
    return tauriInvoke<{ properties: Record<string, unknown> }>(
      "grpc_message_schema",
      {
        files,
        messageType,
      },
    );
  }
  return apiPost<{ properties: Record<string, unknown> }>(
    "/api/grpc/schema-def",
    {
      files,
      message_type: messageType,
    },
  );
}

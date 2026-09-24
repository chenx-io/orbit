import type { KeyValue, SentRequest } from "@/data/types";
import { t } from "@/lib/localeDict";
import { stripBodyComments } from "@/lib/requestBody";

/** Supported command-line HTTP clients */
export type RequestCodeTool = "curl" | "wget" | "xh" | "httpie" | "powershell";

export const REQUEST_CODE_TOOLS: { id: RequestCodeTool; label: string }[] = [
  { id: "curl", label: "cURL" },
  { id: "wget", label: "Wget" },
  { id: "xh", label: "xh" },
  { id: "httpie", label: "HTTPie" },
  { id: "powershell", label: "PowerShell" },
];

/** Wrap in POSIX shell single quotes, escaping inner ' as '\'' */
function shQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** PowerShell single-quoted string, escaping inner ' as '' */
function psQuote(s: string): string {
  return `'${s.replace(/'/g, `''`)}'`;
}

/** Strip the line-continuation marker from the last line of a multi-line command (POSIX uses \, Windows uses ^) */
function dropContinuation(lines: string[], cont: string): string[] {
  const suffix = ` ${cont}`;
  const last = lines[lines.length - 1];
  if (last.endsWith(suffix)) {
    lines[lines.length - 1] = last.slice(0, -suffix.length);
  }
  return lines;
}

// ─── Structured body description (for each tool to build its command) ─────────────────────
type FormField = {
  key: string;
  value?: string;
  filename?: string;
  type?: string;
};
type BodySpec =
  | { kind: "none" }
  | { kind: "text"; content: string }
  | { kind: "binary"; filename: string }
  | { kind: "multipart"; fields: FormField[] };

/**
 * Normalize the body into a structured description a command can consume directly, per bodyMode:
 * - json/xml/raw            → text body (variables already resolved)
 * - x-www-form-urlencoded   → re-urlencoded from formParams (restorable even when the live snapshot's body is empty)
 * - form-data               → multipart field list (text and file kinds)
 * - binary                  → local reference (referenced in the command as @path/@filename; Tauri uses the real absolute path, the browser the file name)
 * File fields prefer `path` (the real absolute path on Tauri) and fall back to `name` (in the browser).
 */
function getBodySpec(req: SentRequest): BodySpec {
  const mode = req.bodyMode ?? "json";
  switch (mode) {
    case "none":
      return { kind: "none" };

    case "binary": {
      // The Tauri desktop stores the real (absolute) path, so prefer @absolute-path; the browser falls back to the file name
      const ref = req.binaryFile?.path ?? req.binaryFile?.name;
      return ref ? { kind: "binary", filename: ref } : { kind: "none" };
    }

    case "form-data": {
      const fields: FormField[] = (req.formParams ?? [])
        .filter((p: KeyValue) => p.enabled && p.key)
        .map((p) =>
          p.file
            ? {
                key: p.key,
                filename: p.file.path ?? p.file.name,
                type: p.file.type,
              }
            : { key: p.key, value: p.value },
        );
      return fields.length ? { kind: "multipart", fields } : { kind: "none" };
    }

    case "x-www-form-urlencoded": {
      const parts = (req.formParams ?? [])
        .filter((p: KeyValue) => p.enabled && p.key)
        .map(
          (p) =>
            `${encodeURIComponent(p.key)}=${encodeURIComponent(p.value ?? "")}`,
        );
      return parts.length
        ? { kind: "text", content: parts.join("&") }
        : { kind: "none" };
    }

    case "json":
    case "xml":
    case "raw":
    default:
      // Strip comments (JSON // and /* */, XML <!-- -->) before exporting the command, keeping only valid data
      return req.body
        ? {
            kind: "text",
            content: stripBodyComments(req.body, req.bodyMode ?? "json"),
          }
        : { kind: "none" };
  }
}

export function generateRequestCode(
  req: SentRequest,
  tool: RequestCodeTool,
  wrap = true,
): string {
  const { method, url, headers } = req;
  const bodySpec = getBodySpec(req);
  const multipart = bodySpec.kind === "multipart";

  // Header splitting:
  // - Accept-Encoding is not sent as a plain -H (it would make curl/wget receive compressed data without
  //   decompressing it and warn about binary output); the client's compression flag handles it instead;
  // - Content-Length is always removed — every client (curl/wget/xh/httpie/PS) computes it from the
  //   actual body, and sending it explicitly easily disagrees with the real byte count (multi-byte characters) and fails;
  // - for multipart the Content-Type (with its boundary) is set by each client automatically, so it is dropped explicitly,
  //   avoiding Orbit's internal random boundary leaking into the command and clashing with the client's actual one.
  let acceptEncoding = false;
  const headerList = Object.entries(headers).filter(([k]) => {
    const kl = k.toLowerCase();
    if (kl === "accept-encoding") {
      acceptEncoding = true;
      return false;
    }
    if (kl === "content-length") return false;
    if (multipart && kl === "content-type") return false;
    return true;
  });

  // Multi-line commands (with continuation markers) collapse onto one line when line-wrapping is off.
  // - non-PowerShell: strip the trailing continuation marker (POSIX `\` and Windows cmd `^`) and the
  //   indentation, rejoining with spaces; no continuation markers remain, so it works in bash / cmd / PowerShell alike.
  // - PowerShell: statements are separated by `;`; inside an `@{ ... }` hashtable block they join with spaces,
  //   because joining with spaces would break the statement after `{` and cause a syntax error.
  const finalize = (lines: string[]): string => {
    if (wrap) return lines.join("\n");
    if (tool === "powershell") {
      const out: string[] = [];
      let i = 0;
      while (i < lines.length) {
        const line = lines[i].trim();
        if (!line) {
          i++;
          continue;
        }
        if (line.startsWith("$headers = @{")) {
          i++;
          const entries: string[] = [];
          while (i < lines.length && !lines[i].trim().startsWith("}")) {
            const inner = lines[i].trim();
            if (inner) entries.push(inner);
            i++;
          }
          if (i < lines.length) i++; // skip the closing }
          out.push(`$headers = @{ ${entries.join(" ; ")} }`);
        } else {
          out.push(line);
          i++;
        }
      }
      return out.join(" ; ");
    }
    return lines
      .map((l) => l.replace(/ \\$/, "").replace(/ \^$/, "").trim())
      .filter(Boolean)
      .join(" ");
  };

  switch (tool) {
    case "curl": {
      const lines: string[] = [];
      let first = `curl -X ${method} ${shQuote(url)}`;
      if (acceptEncoding) first += " --compressed";
      lines.push(`${first} \\`);
      for (const [k, v] of headerList) {
        lines.push(`  -H ${shQuote(`${k}: ${v}`)} \\`);
      }
      const bodyLines: string[] = [];
      if (bodySpec.kind === "text") {
        bodyLines.push(`  -d ${shQuote(bodySpec.content)}`);
      } else if (bodySpec.kind === "binary") {
        bodyLines.push(`  --data-binary ${shQuote(`@${bodySpec.filename}`)}`);
      } else if (bodySpec.kind === "multipart") {
        for (const f of bodySpec.fields) {
          if (f.filename) {
            const fileSpec = f.type
              ? `${f.filename};type=${f.type}`
              : f.filename;
            bodyLines.push(`  -F ${shQuote(`${f.key}=@${fileSpec}`)}`);
          } else {
            bodyLines.push(`  -F ${shQuote(`${f.key}=${f.value ?? ""}`)}`);
          }
        }
      }
      if (bodyLines.length === 0) {
        dropContinuation(lines, "\\");
      } else {
        bodyLines.forEach((bl, i) => {
          lines.push(bl + (i < bodyLines.length - 1 ? " \\" : ""));
        });
      }
      return finalize(lines);
    }

    case "wget": {
      const lines: string[] = [`wget --method=${method}\\`];
      if (acceptEncoding)
        lines[0] = `wget --method=${method} --compression=auto \\`;
      for (const [k, v] of headerList) {
        lines.push(`  --header=${shQuote(`${k}: ${v}`)} \\`);
      }
      if (bodySpec.kind === "text") {
        lines.push(`  --body-data=${shQuote(bodySpec.content)} \\`);
      } else if (bodySpec.kind === "binary") {
        lines.push(`  --body-file=${shQuote(bodySpec.filename)} \\`);
      } else if (bodySpec.kind === "multipart") {
        // wget has no native multipart file upload (no -F equivalent).
        // Text fields fall back to urlencoded form data; file fields require curl/httpie instead.
        if (wrap) {
          lines.push(`  # ${t("requestCode.wgetMultipartNote")}`);
        }
        const textParts = bodySpec.fields
          .filter((f) => !f.filename)
          .map(
            (f) =>
              `${encodeURIComponent(f.key)}=${encodeURIComponent(f.value ?? "")}`,
          )
          .join("&");
        if (textParts) {
          lines.push(`  --body-data=${shQuote(textParts)} \\`);
          lines.push(
            `  --header=${shQuote(
              "Content-Type: application/x-www-form-urlencoded",
            )} \\`,
          );
        }
      }
      lines.push(`  ${shQuote(url)}`);
      return finalize(lines);
    }

    case "xh": {
      const lines: string[] = [];
      if (bodySpec.kind === "multipart") {
        lines.push(`xh --form ${method} ${shQuote(url)} \\`);
      } else {
        lines.push(`xh ${method} ${shQuote(url)} \\`);
      }
      for (const [k, v] of headerList) {
        lines.push(`  ${shQuote(`${k}: ${v}`)} \\`);
      }
      const bodyLines: string[] = [];
      if (bodySpec.kind === "text") {
        bodyLines.push(`  --raw ${shQuote(bodySpec.content)}`);
      } else if (bodySpec.kind === "binary") {
        // xh treats a positional `@file` argument as a raw body, reading the local file content and sending it;
        // note that --raw @file must not be used (--raw takes a literal string and does not expand the @ file syntax,
        // so it would send the literal "@path" as the body and the file content would never be transferred).
        bodyLines.push(`  ${shQuote(`@${bodySpec.filename}`)}`);
      } else if (bodySpec.kind === "multipart") {
        for (const f of bodySpec.fields) {
          if (f.filename) {
            bodyLines.push(`  ${shQuote(`${f.key}@${f.filename}`)}`);
          } else {
            bodyLines.push(`  ${shQuote(`${f.key}=${f.value ?? ""}`)}`);
          }
        }
      }
      if (bodyLines.length === 0) {
        dropContinuation(lines, "\\");
      } else {
        bodyLines.forEach((bl, i) => {
          lines.push(bl + (i < bodyLines.length - 1 ? " \\" : ""));
        });
      }
      return finalize(lines);
    }

    case "httpie": {
      const lines: string[] = [];
      if (bodySpec.kind === "multipart") {
        lines.push(`http -f ${method} ${shQuote(url)} \\`);
      } else {
        lines.push(`http ${method} ${shQuote(url)} \\`);
      }
      for (const [k, v] of headerList) {
        lines.push(`  ${shQuote(`${k}: ${v}`)} \\`);
      }
      const bodyLines: string[] = [];
      if (bodySpec.kind === "text") {
        bodyLines.push(`  --raw ${shQuote(bodySpec.content)}`);
      } else if (bodySpec.kind === "binary") {
        // httpie reads a local file as the raw body via @file
        bodyLines.push(`  ${shQuote(`@${bodySpec.filename}`)}`);
      } else if (bodySpec.kind === "multipart") {
        for (const f of bodySpec.fields) {
          if (f.filename) {
            bodyLines.push(`  ${shQuote(`${f.key}@${f.filename}`)}`);
          } else {
            bodyLines.push(`  ${shQuote(`${f.key}=${f.value ?? ""}`)}`);
          }
        }
      }
      if (bodyLines.length === 0) {
        dropContinuation(lines, "\\");
      } else {
        bodyLines.forEach((bl, i) => {
          lines.push(bl + (i < bodyLines.length - 1 ? " \\" : ""));
        });
      }
      return finalize(lines);
    }

    case "powershell": {
      const lines: string[] = [];
      if (headerList.length) {
        lines.push("$headers = @{");
        for (const [k, v] of headerList) {
          lines.push(`  ${psQuote(k)} = ${psQuote(v)}`);
        }
        lines.push("}");
        lines.push("");
      }
      let cmd = `$response = Invoke-RestMethod -Uri ${psQuote(url)} -Method ${method}`;
      if (headerList.length) cmd += " -Headers $headers";
      if (bodySpec.kind === "text") {
        cmd += ` -Body ${psQuote(bodySpec.content)}`;
      } else if (bodySpec.kind === "binary") {
        // -InFile uses the file content directly as the body (suited to application/octet-stream)
        cmd += ` -InFile ${psQuote(bodySpec.filename)}`;
      } else if (bodySpec.kind === "multipart") {
        const entries = bodySpec.fields.map((f) =>
          f.filename
            ? `${psQuote(f.key)} = Get-Item ${psQuote(f.filename)}`
            : `${psQuote(f.key)} = ${psQuote(f.value ?? "")}`,
        );
        cmd += ` -Form @{ ${entries.join(" ; ")} }`;
      }
      lines.push(cmd);
      lines.push("$response | ConvertTo-Json");
      return finalize(lines);
    }

    default:
      return "";
  }
}

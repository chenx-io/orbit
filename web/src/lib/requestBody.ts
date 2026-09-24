import type { BodyMode, HttpRequest, KeyValue } from "@/data/types";
import { isTauri } from "@/lib/bridge";

// ─── Base64 encoding/decoding (browser / Tauri webview / Node) ──────────────

/** Uint8Array → standard Base64 (with `=` padding) */
export function bytesToBase64(bytes: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  if (typeof btoa === "function") return btoa(binary);
  if (typeof Buffer !== "undefined") {
    return Buffer.from(binary, "binary").toString("base64");
  }
  throw new Error("base64 encoding not supported");
}

/** Standard Base64 (without the `data:` prefix) → Uint8Array */
export function base64ToBytes(b64: string): Uint8Array {
  const clean = b64.replace(/\s/g, "");
  const bin =
    typeof atob === "function"
      ? atob(clean)
      : Buffer.from(clean, "base64").toString("binary");
  const len = bin.length;
  const bytes = new Uint8Array(len);
  for (let i = 0; i < len; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

/** Read a user-picked file as { name, data(base64), type } */
export function readFileAsBase64(
  file: File,
): Promise<{ name: string; data: string; type: string }> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string; // data URL: data:<mime>;base64,<data>
      const comma = result.indexOf(",");
      const data = comma >= 0 ? result.slice(comma + 1) : result;
      resolve({
        name: file.name,
        data,
        type: file.type || "application/octet-stream",
      });
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(file);
  });
}

// ─── multipart/form-data assembly ──────────────────────────────────────

/** Escape double quotes in a multipart disposition */
function escapeDisp(s: string): string {
  return s.replace(/"/g, '\\"');
}

/** Assemble form-data key/values (files included) into a multipart byte stream */
function buildMultipart(
  params: KeyValue[],
  boundary: string,
): { bytes: Uint8Array; contentType: string } {
  const enc = new TextEncoder();
  const parts: Uint8Array[] = [];
  const sep = (s: string) => parts.push(enc.encode(s));

  for (const p of params) {
    if (!p.enabled || !p.key) continue;
    if (p.file && p.file.data) {
      const fileBytes = base64ToBytes(p.file.data);
      sep(
        `--${boundary}\r\n` +
          `Content-Disposition: form-data; name="${escapeDisp(p.key)}"; filename="${escapeDisp(p.file.name)}"\r\n` +
          `Content-Type: ${p.file.type || "application/octet-stream"}\r\n\r\n`,
      );
      parts.push(fileBytes);
      sep("\r\n");
    } else {
      sep(
        `--${boundary}\r\n` +
          `Content-Disposition: form-data; name="${escapeDisp(p.key)}"\r\n\r\n`,
      );
      sep(p.value);
      sep("\r\n");
    }
  }
  sep(`--${boundary}--\r\n`);

  let total = 0;
  for (const p of parts) total += p.length;
  const out = new Uint8Array(total);
  let off = 0;
  for (const p of parts) {
    out.set(p, off);
    off += p.length;
  }
  return {
    bytes: out,
    contentType: `multipart/form-data; boundary=${boundary}`,
  };
}

// ─── Build the final request body per bodyMode ────────────────────────────────────

export interface FormFieldSpec {
  key: string;
  /** Text value (empty for file fields) */
  value?: string;
  /** Real absolute path of the file in Tauri path mode (no pre-read; Rust reads it from disk when sending) */
  filePath?: string;
  /** File MIME type */
  fileType?: string;
  /** Original file name (used by Content-Disposition) */
  filename?: string;
}

export interface BuiltBody {
  /** Text request body (used when bodyBinary is null) */
  body: string;
  /** base64 of a binary / multipart body (bytes already assembled by the frontend); uses `body` when null.
   *  Null in Tauri path mode (the real content is read by path on the sending side, never through base64). */
  bodyBinary: string | null;
  /** Final Content-Type (may be empty, meaning it is not attached) */
  contentType: string;
  /** Tauri path mode (binary): the file's real absolute path; Rust reads it from disk and sends it */
  binaryFilePath?: string | null;
  /** Tauri path mode (form-data): structured fields (files carry a real path); Rust builds the multipart body */
  formFields?: FormFieldSpec[] | null;
}

export interface BuildBodyInput {
  bodyMode: BodyMode;
  /** Parsed text for json / xml / raw */
  body: string;
  contentType: string;
  binaryFile: HttpRequest["binaryFile"];
  /** Parsed key/values for form-data / urlencoded */
  formParams: KeyValue[];
}

export function buildRequestBody(input: BuildBodyInput): BuiltBody {
  const { bodyMode, body, contentType, binaryFile, formParams } = input;
  switch (bodyMode) {
    case "none":
      return { body: "", bodyBinary: null, contentType: "" };

    case "json":
      return { body, bodyBinary: null, contentType: "application/json" };

    case "xml":
      return { body, bodyBinary: null, contentType: "application/xml" };

    case "raw":
      // Only raw mode lets the user override Content-Type through the contentType field;
      // every other mode (json/xml/binary/urlencoded/form-data) is derived strictly from bodyMode,
      // so a leftover default contentType on the request (e.g. application/json) cannot override it wrongly.
      return {
        body,
        bodyBinary: null,
        contentType: contentType || "text/plain",
      };

    case "binary": {
      // Tauri path mode: files store the real absolute path, nothing is pre-read, and the sending side reads from disk
      if (binaryFile && binaryFile.path && isTauri()) {
        return {
          body: "",
          bodyBinary: null,
          contentType: "application/octet-stream",
          binaryFilePath: binaryFile.path,
        };
      }
      if (binaryFile && binaryFile.data) {
        return {
          body: "",
          bodyBinary: binaryFile.data,
          contentType: "application/octet-stream",
        };
      }
      return { body: "", bodyBinary: null, contentType: "" };
    }

    case "x-www-form-urlencoded": {
      const parts = formParams
        .filter((p) => p.enabled && p.key)
        .map(
          (p) => `${encodeURIComponent(p.key)}=${encodeURIComponent(p.value)}`,
        );
      return {
        body: parts.join("&"),
        bodyBinary: null,
        contentType: "application/x-www-form-urlencoded",
      };
    }

    case "form-data": {
      // Tauri path mode: when every file row carries a real path, do not pre-read/assemble bytes in the frontend;
      // instead hand the structured fields (including filePath) to the sending side (Rust) to read from disk and build multipart.
      // Note: contentType is left empty here and Rust injects the boundary-bearing Content-Type during assembly.
      const hasPathFiles = formParams.some(
        (p) => p.type === "file" && p.file?.path && isTauri(),
      );
      if (hasPathFiles) {
        const formFields: FormFieldSpec[] = formParams
          .filter((p) => p.enabled && p.key)
          .map((p) =>
            p.file?.path
              ? {
                  key: p.key,
                  filePath: p.file.path,
                  fileType: p.file.type,
                  filename: p.file.name,
                }
              : { key: p.key, value: p.value },
          );
        return {
          body: "",
          bodyBinary: null,
          contentType: "",
          formFields,
        };
      }
      // Browser side (or fallback): the frontend reads base64 and assembles the multipart body
      const boundary = `----orbitFormBoundary${Math.random()
        .toString(36)
        .slice(2)}${Date.now().toString(36)}`;
      const { bytes, contentType: ct } = buildMultipart(formParams, boundary);
      return { body: "", bodyBinary: bytesToBase64(bytes), contentType: ct };
    }
  }
}

/**
 * Get the text body for the current bodyMode (json/xml/raw are stored independently).
 * Prefer the bodyByMode[mode] slot; when missing, only json mode falls back to the legacy single req.body field
 * (legacy data compat); the other modes (xml/raw) are empty until edited, so they never interfere.
 */
export function getActiveBody(req: HttpRequest): string {
  const slot = req.bodyByMode?.[req.bodyMode];
  if (slot !== undefined) return slot;
  if (req.bodyMode === "json") return req.body ?? "";
  return "";
}

// ─── Body comment stripping (used when sending / exporting; the editor keeps the original) ──────────────

/**
 * Strip comments from the request body, keeping only valid data (called before sending and before exporting curl / scenario YAML).
 * - json: supports `//` line comments and `/* ... *\/` block comments (a `//` or `/*` inside a string is untouched);
 * - xml: strips `<!-- ... -->` comments (kept inside CDATA);
 * - raw and others: returned as-is (raw is arbitrary text with no comment semantics).
 */
export function stripBodyComments(text: string, mode: string): string {
  if (!text) return text;
  if (mode === "json") return stripJsonComments(text).text;
  if (mode === "xml") return stripXmlComments(text);
  return text;
}

/**
 * JSON comment stripping (state machine: tracks strings and escapes, so a `//` / `/*` inside a string is not mistaken for a comment).
 * Returns { text, map } where map[i] = the offset in the original text of the i-th stripped character (so validation errors can be mapped back).
 * When a comment is removed and occupies a whole line (only whitespace before it), the newline is removed too, avoiding a blank line.
 */
export function stripJsonComments(json: string): {
  text: string;
  map: number[];
} {
  let out = "";
  const map: number[] = [];
  let inString = false;
  let escape = false;
  let i = 0;
  const n = json.length;
  const lineStartOf = (pos: number) => json.lastIndexOf("\n", pos - 1) + 1;
  while (i < n) {
    const ch = json[i];
    if (inString) {
      out += ch;
      map.push(i);
      if (escape) escape = false;
      else if (ch === "\\") escape = true;
      else if (ch === '"') inString = false;
      i++;
      continue;
    }
    if (ch === '"') {
      inString = true;
      out += ch;
      map.push(i);
      i++;
      continue;
    }
    if (ch === "/" && json[i + 1] === "/") {
      // Line comment: if only whitespace precedes it → delete the leading whitespace and newline too (no blank line); otherwise delete only the comment text
      const lineStart = lineStartOf(i);
      const onlyCommentOnLine = /^\s*$/.test(json.slice(lineStart, i));
      while (i < n && json[i] !== "\n") i++;
      if (onlyCommentOnLine) {
        // Roll back this line's leading whitespace (already written to out; the comment line's indent must not remain)
        while (out.length > 0 && out[out.length - 1] !== "\n") {
          out = out.slice(0, -1);
          map.pop();
        }
        if (i < n && json[i] === "\n") i++; // skip the newline
        while (i < n && json[i] === "\n") i++; // also clear the blank lines right after
      }
      continue;
    }
    if (ch === "/" && json[i + 1] === "*") {
      // Block comment: a whole line (whitespace on both sides) → delete the leading whitespace and newline too; otherwise delete only the block text
      const lineStart = lineStartOf(i);
      const onlyCommentOnLine = /^\s*$/.test(json.slice(lineStart, i));
      i += 2;
      while (i < n && !(json[i] === "*" && json[i + 1] === "/")) i++;
      if (i < n) i += 2; // skip */
      // Whether only whitespace follows the block to the end of the line
      let j = i;
      while (j < n && (json[j] === " " || json[j] === "\t" || json[j] === "\r"))
        j++;
      const blankAfter = j >= n || json[j] === "\n";
      if (onlyCommentOnLine && blankAfter) {
        while (out.length > 0 && out[out.length - 1] !== "\n") {
          out = out.slice(0, -1);
          map.pop();
        }
        if (j < n && json[j] === "\n") i = j + 1;
        while (i < n && json[i] === "\n") i++;
      }
      continue;
    }
    out += ch;
    map.push(i);
    i++;
  }
  return { text: out, map };
}

/** XML comment stripping: `<!-- ... -->` is dropped; CDATA / declarations / processing instructions are kept as-is */
function stripXmlComments(xml: string): string {
  let out = "";
  let i = 0;
  const n = xml.length;
  while (i < n) {
    if (xml.startsWith("<![CDATA[", i)) {
      const end = xml.indexOf("]]>", i + 9);
      if (end < 0) {
        out += xml.slice(i);
        break;
      }
      out += xml.slice(i, end + 3);
      i = end + 3;
      continue;
    }
    if (xml.startsWith("<!--", i)) {
      const end = xml.indexOf("-->", i + 4);
      if (end < 0) break; // unclosed comment: drop to the end
      i = end + 3;
      continue;
    }
    out += xml[i];
    i++;
  }
  return out;
}

// ─── Formatting (tolerant of {{var}} dynamic values)─────────────────────────────────

/**
 * JSON prettifier tolerant of {{var}} dynamic values.
 *
 * The hard part: {{var}} may appear inside string values (most common) or bare in an array/object value position
 * (e.g. `[1,2,{{x}}]`). Approach:
 * - track JSON string state while scanning; **{{...}} inside strings is kept as-is** (it is already valid text);
 * - only when {{...}} appears outside a string is it replaced by a quoted placeholder (a valid JSON value);
 * after a normal JSON.parse / stringify, restore the placeholders back to {{var}}.
 */

/** Replace bare `{{var}}` with quoted placeholders, returning parseable text plus a restore function. */
function maskJsonDynamics(src: string): {
  masked: string;
  restore: (s: string) => string;
} {
  const placeholders: Record<string, string> = {};
  let idx = 0;
  let masked = "";
  let inStr = false;
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    if (inStr) {
      masked += c;
      if (c === "\\") {
        masked += src[i + 1] ?? "";
        i += 2;
        continue;
      }
      if (c === '"') inStr = false;
      i++;
      continue;
    }
    if (c === '"') {
      inStr = true;
      masked += c;
      i++;
      continue;
    }
    if (c === "{" && src[i + 1] === "{") {
      const end = src.indexOf("}}", i + 2);
      if (end === -1) {
        masked += c;
        i++;
        continue;
      }
      const token = src.slice(i, end + 2);
      const key = `__ORBIT_DYN_${idx++}__`;
      placeholders[key] = token;
      masked += `"${key}"`;
      i = end + 2;
      continue;
    }
    masked += c;
    i++;
  }
  const restore = (s: string) =>
    s.replace(/"__ORBIT_DYN_\d+__"/g, (m) => placeholders[m.slice(1, -1)] ?? m);
  return { masked, restore };
}

/** Loose JSON token: structural char / string / comment / value run (whitespace included, trimmed on output). */
type JsonLooseTok =
  | { t: "punct"; v: string }
  | { t: "str"; v: string }
  | { t: "comment"; v: string }
  | { t: "value"; v: string };

function tokenizeJsonLoose(src: string): JsonLooseTok[] {
  const toks: JsonLooseTok[] = [];
  let i = 0;
  const n = src.length;
  while (i < n) {
    const c = src[i];
    if (c === '"') {
      let j = i + 1;
      let esc = false;
      while (j < n) {
        const ch = src[j];
        if (esc) esc = false;
        else if (ch === "\\") esc = true;
        else if (ch === '"') {
          j++;
          break;
        }
        j++;
      }
      toks.push({ t: "str", v: src.slice(i, j) });
      i = j;
      continue;
    }
    // Bare {{var}} / {{$dynamic}}: taken as one value token (so the `{` structural char does not split it)
    if (c === "{" && src[i + 1] === "{") {
      const end = src.indexOf("}}", i + 2);
      if (end >= 0) {
        toks.push({ t: "value", v: src.slice(i, end + 2) });
        i = end + 2;
        continue;
      }
    }
    if (c === "/" && src[i + 1] === "/") {
      let j = i;
      while (j < n && src[j] !== "\n") j++;
      toks.push({ t: "comment", v: src.slice(i, j) });
      i = j;
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      let j = i + 2;
      while (j < n && !(src[j] === "*" && src[j + 1] === "/")) j++;
      j = Math.min(j + 2, n);
      toks.push({ t: "comment", v: src.slice(i, j) });
      i = j;
      continue;
    }
    if (
      c === "{" ||
      c === "}" ||
      c === "[" ||
      c === "]" ||
      c === "," ||
      c === ":"
    ) {
      toks.push({ t: "punct", v: c });
      i++;
      continue;
    }
    // Value run: gathered until the next structural char / string / comment (whitespace included, trimmed on output)
    let j = i;
    while (
      j < n &&
      !"{}[],:".includes(src[j]) &&
      src[j] !== '"' &&
      !(src[j] === "/" && (src[j + 1] === "/" || src[j + 1] === "*"))
    ) {
      j++;
    }
    toks.push({ t: "value", v: src.slice(i, j) });
    i = j;
  }
  return toks;
}

/**
 * Comment-aware JSON prettifier: keeps comments (line // and block, standalone or trailing) and {{var}} dynamic values.
 * Rearranges a token stream (no reliance on JSON.parse), so commented input / bare {{var}} still works.
 * Strip first, then parse to validate: only prettifies input that is valid after stripping; invalid input returns null (leaving the original intact).
 */
export function formatJsonTolerant(src: string): string | null {
  const { text: clean } = stripJsonComments(src);
  if (clean.trim()) {
    const { masked } = maskJsonDynamics(clean);
    try {
      JSON.parse(masked);
    } catch {
      return null; // still invalid after stripping comments → do not prettify
    }
  }
  const tokens = tokenizeJsonLoose(src);
  const lines: string[] = [];
  let indent = 0;
  let cur = "";
  const pad = () => "  ".repeat(indent);
  const flush = () => {
    if (cur.trim()) lines.push(cur);
    cur = "";
  };

  for (let i = 0; i < tokens.length; i++) {
    const tok = tokens[i];
    if (tok.t === "comment") {
      // Comment: a trailing comment attaches to the current line (newline); otherwise a standalone comment line (indent-aligned)
      if (cur.trim()) {
        cur += " " + tok.v;
        flush();
      } else {
        lines.push(pad() + tok.v);
      }
      continue;
    }
    const v = tok.v.trim();
    if (!v) continue; // purely whitespace token
    if (v === "{" || v === "[") {
      // Empty structures {} / [] stay on one line
      const next = tokens[i + 1]?.v.trim();
      if (next === "}" || next === "]") {
        cur += (cur.trim() ? (cur.endsWith(": ") ? "" : " ") : "") + v + next;
        i++;
        continue;
      }
      // `:` is already followed by a space, so no extra space is appended
      cur += (cur.trim() ? (cur.endsWith(": ") ? "" : " ") : "") + v;
      flush();
      indent++;
      continue;
    }
    if (v === "}" || v === "]") {
      indent = Math.max(0, indent - 1);
      // Emit the current line first (e.g. the last property `"age": 18`), then the closing bracket on its own line
      flush();
      lines.push(pad() + v);
      continue;
    }
    if (v === ",") {
      // If the current line is empty (a closing `]` / `}` was just flushed), append the comma to the previous line (`],` / `},`)
      if (!cur.trim() && lines.length > 0) {
        lines[lines.length - 1] += ",";
      } else {
        cur += ",";
      }
      flush();
      continue;
    }
    if (v === ":") {
      cur = cur.replace(/\s+$/, "") + ": ";
      continue;
    }
    // Value (string / number / boolean / null / bare {{var}})
    if (cur.endsWith(": ")) cur += v;
    else if (cur.trim()) cur += " " + v;
    else cur = pad() + v;
  }
  flush();
  return lines.join("\n");
}

/**
 * JSON minifier (collapse to one line) that **keeps comments** (editor actions must not delete user content):
 * - block comments `/* ... *\/` are kept inline;
 * - line comments `// ...` get their own line (a line comment swallows what follows, so a newline is required);
 * - tolerant of {{var}} dynamic values; only minifies input valid after stripping comments, otherwise returns null.
 */
export function minifyJsonTolerant(src: string): string | null {
  const { text: clean } = stripJsonComments(src);
  if (!clean.trim()) return "";
  const { masked } = maskJsonDynamics(clean);
  try {
    JSON.parse(masked);
  } catch {
    return null; // still invalid after stripping comments → do not minify
  }

  const tokens = tokenizeJsonLoose(src);
  let out = "";
  let afterLineComment = false;
  for (const tok of tokens) {
    const v = tok.v.trim();
    if (!v) continue; // whitespace
    if (tok.t === "comment") {
      if (v.startsWith("//")) {
        // Line comment: on its own line (following content starts a new line so it is not swallowed)
        out += (out ? "\n" : "") + v;
        afterLineComment = true;
      } else {
        // Block comment: inline; add a space when the previous char needs separating
        if (out && !/[ ,:[{("]$/.test(out)) out += " ";
        out += v;
        afterLineComment = false;
      }
      continue;
    }
    if (afterLineComment && out) {
      out += "\n";
      afterLineComment = false;
    }
    if (v === "{" || v === "[" || v === "," || v === ":") {
      out += v;
    } else if (v === "}" || v === "]") {
      out += v;
    } else {
      // Value / string: concatenate directly when the previous char is a separator (, : [ { or newline); otherwise add a space (e.g. before {{var}})
      if (out && !/[,[{: \n]$/.test(out) && out[out.length - 1] !== " ")
        out += " ";
      out += v;
    }
  }
  return out;
}

/** XML token kinds (used by formatXml) */
type XmlTokKind =
  | "decl" // declaration / doctype / processing instruction
  | "comment"
  | "cdata"
  | "open"
  | "close"
  | "self"
  | "text";

interface XmlTok {
  k: XmlTokKind;
  v: string;
}

/** Split by comment / CDATA / tag / text (a fresh regex per call to avoid a shared lastIndex) */
function tokenizeXml(xml: string): XmlTok[] {
  const re =
    /<!--[\s\S]*?-->|<!\[CDATA\[[\s\S]*?\]\]>|<\/?[^\s>]+(?:\s[^>]*?)?\/?>|[^<]+/g;
  const toks: XmlTok[] = [];
  let m: RegExpExecArray | null;
  while ((m = re.exec(xml)) !== null) {
    const v = m[0];
    let k: XmlTokKind;
    if (v.startsWith("<!--")) k = "comment";
    else if (v.startsWith("<![CDATA[")) k = "cdata";
    else if (v.startsWith("<?") || v.startsWith("<!")) k = "decl";
    else if (v.startsWith("</")) k = "close";
    else if (v.endsWith("/>")) k = "self";
    else if (v.startsWith("<")) k = "open";
    else k = "text";
    toks.push({ k, v });
  }
  return toks;
}

/** Extract the tag name: `<name attr="1">` / `</name>` → name */
function xmlTagName(tag: string): string {
  const m = tag.match(/^<\/?([^\s/>]+)/);
  return m ? m[1] : "";
}

/**
 * Lightweight XML prettifier (2-space indent), tolerant of {{var}} in text and attribute values.
 * - comments / CDATA / declarations / doctype / self-closing tags each take one line;
 * - **leaf elements (content is only text / CDATA) collapse onto one line**: `<name>json</name>`, the value no longer wraps;
 * - when child elements exist (mixed content) it degrades to one node per line, with text nodes trimmed onto their own line.
 */
export function formatXml(input: string): string | null {
  const xml = input.trim();
  if (!xml) return "";
  try {
    const toks = tokenizeXml(xml);
    const out: string[] = [];
    let indent = 0;
    const pad = () => "  ".repeat(indent);
    let i = 0;
    while (i < toks.length) {
      const tok = toks[i];
      if (tok.k === "comment" || tok.k === "decl" || tok.k === "self") {
        out.push(pad() + tok.v);
        i++;
        continue;
      }
      if (tok.k === "close") {
        indent = Math.max(0, indent - 1);
        out.push(pad() + tok.v);
        i++;
        continue;
      }
      if (tok.k === "cdata") {
        out.push(pad() + tok.v);
        i++;
        continue;
      }
      if (tok.k === "text") {
        // Skip pure whitespace between tags (newlines / indentation) to avoid extra blank lines
        const t = tok.v.trim();
        if (t) out.push(pad() + t);
        i++;
        continue;
      }
      // Open tag: look ahead to see whether it is "only text / CDATA plus the matching close tag" → single-line output
      const name = xmlTagName(tok.v);
      let j = i + 1;
      let inline = "";
      let closed = false;
      while (j < toks.length) {
        const t = toks[j];
        if (t.k === "text") {
          inline += t.v.trim();
          j++;
          continue;
        }
        if (t.k === "cdata") {
          inline += t.v;
          j++;
          continue;
        }
        // On a child element / comment / nested structure → do not inline, take the normal multi-line branch
        if (t.k === "close" && xmlTagName(t.v) === name) {
          closed = true;
          j++;
        }
        break;
      }
      if (closed) {
        out.push(pad() + tok.v + inline + toks[j - 1].v);
        i = j;
        continue;
      }
      out.push(pad() + tok.v);
      indent++;
      i++;
    }
    return out.join("\n");
  } catch {
    return null;
  }
}

/**
 * Lightweight XML minifier: removes excess whitespace between tags (text nodes are kept and trimmed), collapsing to a compact single line.
 * Comments / CDATA / processing instructions / doctype are kept as-is (they may contain {{var}}; inner whitespace is untouched).
 */
export function minifyXml(input: string): string | null {
  const xml = input.trim();
  if (!xml) return "";
  const re =
    /<!--[\s\S]*?-->|<!\[CDATA\[[\s\S]*?\]\]>|<\/?[^\s>]+(?:\s[^>]*?)?\/?>|[^<]+/g;
  const parts: string[] = [];
  let m: RegExpExecArray | null;
  try {
    while ((m = re.exec(xml)) !== null) {
      const tok = m[0];
      // Comments / CDATA / declarations / doctype / tags: kept as-is
      if (tok.startsWith("<")) {
        parts.push(tok);
        continue;
      }
      // Text nodes: trim both ends; pure whitespace (newlines/indent between tags) is skipped
      const t = tok.trim();
      if (t) parts.push(t);
    }
  } catch {
    return null;
  }
  return parts.join("");
}

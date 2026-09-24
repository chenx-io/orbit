// Request body syntax validation: JSON (JSON.parse plus lezer syntax-tree location) / XML (stack-based structure validation).
// Pure functions with no browser API dependency, easy to unit-test and reuse. raw mode is not validated.
// JSON supports comments (// and /* */): comments are stripped before validation (testers may annotate in the editor without errors).

import { parser as jsonParser } from "@lezer/json";
import { t, tFormat } from "@/lib/localeDict";
import { stripJsonComments } from "@/lib/requestBody";

export interface BodyValidateResult {
  message: string;
  /** 1-based line number (0 = unknown) */
  line: number;
  /** 1-based column number (0 = unknown) */
  column: number;
}

export type BodyValidateMode = "json" | "xml" | "raw";

/** Returning null means the syntax is fine (or the content is empty / does not need validation). */
export function validateBody(
  mode: BodyValidateMode,
  text: string,
): BodyValidateResult | null {
  if (!text.trim()) return null;
  if (mode === "json") return validateJson(text);
  if (mode === "xml") return validateXml(text);
  return null; // raw: no validation
}

function positionOf(
  text: string,
  offset: number,
): { line: number; column: number } {
  const clamped = Math.max(0, Math.min(offset, text.length));
  const before = text.slice(0, clamped);
  const line = before.split("\n").length;
  const lastNl = before.lastIndexOf("\n");
  const column = clamped - lastNl;
  return { line, column };
}

function validateJson(text: string): BodyValidateResult | null {
  // Strip comments before validating; error positions are mapped back to the original text through `map` (line/column include comments)
  const { text: clean, map } = stripJsonComments(text);
  if (!clean.trim()) return null; // all comments → treated as valid
  try {
    JSON.parse(clean);
    return null;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    // Locate the first error node with the lezer JSON syntax tree (newer V8 error messages no longer carry a position)
    let errPos = -1;
    try {
      const tree = jsonParser.parse(clean);
      tree.iterate({
        enter: (node) => {
          if (node.type.isError) {
            errPos = node.from;
            return false;
          }
          return undefined;
        },
      });
    } catch {
      // Fall back to position 0 when the parser itself throws
    }
    const cleanPos = errPos >= 0 ? errPos : 0;
    const origPos = map[cleanPos] ?? cleanPos;
    const { line, column } = positionOf(text, origPos);
    return { message: msg, line, column };
  }
}

/**
 * Simple XML structure validation: tag-stack matching + attribute quote checking + unclosed detection.
 * Skips comments / CDATA / processing instructions / DOCTYPE; a `<` in text (not starting a tag) is skipped as plain text.
 * Covers the most common structural errors; not a strict XML conformance check.
 */
export function validateXml(text: string): BodyValidateResult | null {
  const stack: { name: string; offset: number }[] = [];
  let i = 0;
  const n = text.length;

  const err = (offset: number, message: string): BodyValidateResult => {
    const { line, column } = positionOf(text, offset);
    return { message, line, column };
  };

  const skipUntil = (start: number, end: string): number => {
    const idx = text.indexOf(end, start);
    return idx < 0 ? n : idx + end.length;
  };

  while (i < n) {
    const lt = text.indexOf("<", i);
    if (lt < 0) break;

    // Comments / CDATA / processing instructions / DOCTYPE: skipped entirely
    if (text.startsWith("<!--", lt)) {
      i = skipUntil(lt, "-->");
      continue;
    }
    if (text.startsWith("<![CDATA[", lt)) {
      i = skipUntil(lt, "]]>");
      continue;
    }
    if (text.startsWith("<?", lt)) {
      i = skipUntil(lt, "?>");
      continue;
    }
    if (/^<!DOCTYPE/i.test(text.slice(lt, lt + 9))) {
      i = skipUntil(lt, ">");
      continue;
    }

    // The char after `<` is not a possible tag start (letter/_/!/?// etc.) → skip as plain text (e.g. `a < b`)
    if (!/^[A-Za-z_!?/]/.test(text.slice(lt + 1, lt + 2))) {
      i = lt + 1;
      continue;
    }

    const gt = text.indexOf(">", lt);
    if (gt < 0) return err(lt, t("body.error.unclosedTag"));

    const raw = text.slice(lt + 1, gt);

    // Closing tag </name>
    if (raw.startsWith("/")) {
      const name = raw.slice(1).trim().split(/\s+/)[0] || "";
      const top = stack.pop();
      if (!top) return err(lt, tFormat("body.error.unexpectedClose", name));
      if (top.name !== name) {
        return err(lt, tFormat("body.error.mismatchedClose", name, top.name));
      }
      i = gt + 1;
      continue;
    }

    // Self-closing <a/> or <a />
    const isSelfClosing = /\/\s*$/.test(raw);
    const nameMatch = raw.match(/^[A-Za-z_][\w:.-]*/);
    if (!nameMatch) return err(lt, tFormat("body.error.invalidTag", raw));
    const name = nameMatch[0];

    // Rough attribute-quote check: an = is present but the value is not quoted
    const attrPart = isSelfClosing
      ? raw.slice(name.length).replace(/\/\s*$/, "")
      : raw.slice(name.length);
    const eqIdx = attrPart.indexOf("=");
    if (eqIdx >= 0) {
      const afterEq = attrPart.slice(eqIdx + 1).trim();
      if (!/^["']/.test(afterEq)) {
        return err(
          lt + 1 + name.length + eqIdx,
          tFormat(
            "body.error.unquotedAttr",
            attrPart.slice(0, eqIdx + 2).trim(),
          ),
        );
      }
    }

    if (!isSelfClosing) stack.push({ name, offset: lt });
    i = gt + 1;
  }

  if (stack.length) {
    const top = stack[stack.length - 1];
    return err(top.offset, tFormat("body.error.unclosedOpenTag", top.name));
  }
  return null;
}

// Shared constants for the protocol editors (kept in a separate file to avoid Fast Refresh warnings on component files).
import type { GrpcStreamMode, PayloadType, TcpFramingMode } from "@/data/types";

export const PAYLOAD_TYPES: PayloadType[] = ["text", "base64", "hex"];
export const FRAMING_MODES: TcpFramingMode[] = [
  "read_until_close",
  "delimiter",
  "fixed",
  "length_prefix",
];
export const STREAM_MODES: GrpcStreamMode[] = [
  "server_streaming",
  "client_streaming",
  "bidirectional",
];

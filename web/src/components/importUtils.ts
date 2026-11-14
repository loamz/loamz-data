import { validName } from "@/kinds";
import type { ImportFileRequest } from "@/api";

export type ImportSourceType = "volume" | "url";
export type ImportMode = "create" | "append" | "replace";

export function inferFormat(path: string): "csv" | "parquet" | null {
  const lower = path.toLowerCase().trim();
  if (lower.endsWith(".csv")) return "csv";
  if (lower.endsWith(".parquet")) return "parquet";
  return null;
}

export function inferFormatFromUrl(url: string): "csv" | "parquet" | null {
  if (!url) return null;
  try {
    const u = new URL(url);
    return inferFormat(u.pathname);
  } catch {
    const pathOnly = url.split("?")[0]?.split("#")[0] ?? "";
    return inferFormat(pathOnly);
  }
}

export function validateUrl(url: string): { valid: boolean; error?: string } {
  const trimmed = url.trim();
  if (!trimmed) {
    return { valid: false, error: "Enter a web address." };
  }
  if (!trimmed.toLowerCase().startsWith("https://")) {
    return { valid: false, error: "Address must start with https://" };
  }
  const afterScheme = trimmed.slice(8);
  if (!afterScheme || afterScheme.startsWith("/") || afterScheme.startsWith("?") || afterScheme.startsWith(":")) {
    return { valid: false, error: "Address has no host." };
  }
  if (trimmed.length > 4096) {
    return { valid: false, error: "Address is too long (maximum 4096 characters)." };
  }
  let u: URL;
  try {
    u = new URL(trimmed);
  } catch {
    return { valid: false, error: "Address is not a valid URL." };
  }
  if (u.protocol !== "https:") {
    return { valid: false, error: "Address must start with https://" };
  }
  if (u.username || u.password) {
    return { valid: false, error: "Address must not contain a user name or password." };
  }
  if (u.port && u.port !== "443") {
    return { valid: false, error: "Address must use the standard HTTPS port (443)." };
  }
  if (!u.hostname) {
    return { valid: false, error: "Address has no host." };
  }
  return { valid: true };
}

export function deriveDefaultTableName(filePathOrUrl: string): string {
  let target = filePathOrUrl;
  if (target.startsWith("http://") || target.startsWith("https://")) {
    try {
      target = new URL(target).pathname;
    } catch {
      target = target.split("?")[0]?.split("#")[0] ?? "";
    }
  }
  const filename = target.split("/").pop() ?? "";
  const base = filename.replace(/\.(csv|parquet)$/i, "").toLowerCase();
  const sanitized = base.replace(/[^a-z0-9_]/g, "_").replace(/^[^a-z]+/, "");
  if (sanitized && validName(sanitized)) {
    return sanitized;
  }
  return "";
}

export interface ValidateImportParams {
  schema: string;
  tableName: string;
  sourceType: ImportSourceType;
  volumeId?: string;
  filePath?: string;
  url?: string;
  mode: ImportMode;
  explicitFormat?: "csv" | "parquet" | "";
}

export function validateImport({
  schema,
  tableName,
  sourceType,
  volumeId,
  filePath,
  url,
  mode,
  explicitFormat,
}: ValidateImportParams): {
  valid: boolean;
  error?: string;
} {
  if (!schema.trim()) {
    return { valid: false, error: "Choose a target schema." };
  }
  if (!tableName.trim()) {
    return { valid: false, error: "Enter a table name." };
  }
  if (!validName(tableName.trim())) {
    return {
      valid: false,
      error: "Table name must start with a letter and contain only lower-case letters, digits, and underscores (max 63 characters).",
    };
  }
  if (!["create", "append", "replace"].includes(mode)) {
    return { valid: false, error: "Mode must be create, append, or replace." };
  }

  if (sourceType === "url") {
    const urlCheck = validateUrl(url ?? "");
    if (!urlCheck.valid) {
      return urlCheck;
    }
    const detectedFormat = inferFormatFromUrl(url ?? "");
    const format = explicitFormat || detectedFormat;
    if (!format) {
      return {
        valid: false,
        error: "Choose a format (CSV or Parquet) when the address has no file extension.",
      };
    }
    return { valid: true };
  }

  // Volume source
  if (!volumeId?.trim()) {
    return { valid: false, error: "Choose a source volume." };
  }
  if (!filePath?.trim()) {
    return { valid: false, error: "Choose a source file." };
  }
  const detectedFormat = inferFormat(filePath);
  const format = explicitFormat || detectedFormat;
  if (!format) {
    return { valid: false, error: "Only CSV and Parquet files can be imported." };
  }
  return { valid: true };
}

export interface BuildImportParams {
  schema: string;
  tableName: string;
  sourceType: ImportSourceType;
  volumeId?: string;
  filePath?: string;
  url?: string;
  mode: ImportMode;
  format?: "csv" | "parquet" | "";
}

export function buildImportPayload({
  schema,
  tableName,
  sourceType,
  volumeId,
  filePath,
  url,
  mode,
  format,
}: BuildImportParams): ImportFileRequest {
  const target = `${schema.trim()}.${tableName.trim()}`;
  if (sourceType === "url") {
    const cleanUrl = (url ?? "").trim();
    const detected = inferFormatFromUrl(cleanUrl);
    const chosenFormat = (format ? format : detected) ?? undefined;
    return {
      target,
      url: cleanUrl,
      mode,
      ...(chosenFormat ? { format: chosenFormat } : {}),
    };
  }

  const cleanPath = (filePath ?? "").trim().replace(/^\/+/, "");
  const detected = inferFormat(cleanPath);
  const chosenFormat = (format ? format : detected) ?? undefined;
  return {
    target,
    volume: (volumeId ?? "").trim(),
    path: cleanPath,
    mode,
    ...(chosenFormat ? { format: chosenFormat } : {}),
  };
}

export function parseImportOutput(stdout?: string): { rows: number | null; table: string | null } {
  if (!stdout) return { rows: null, table: null };
  const lines = stdout.split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i].trim();
    if (line.startsWith("{") && line.endsWith("}")) {
      try {
        const parsed = JSON.parse(line) as { rows?: unknown; table?: unknown };
        const rows = typeof parsed.rows === "number" ? parsed.rows : null;
        const table = typeof parsed.table === "string" ? parsed.table : null;
        if (rows !== null || table !== null) {
          return { rows, table };
        }
      } catch {
        // ignore non-json line
      }
    }
  }
  return { rows: null, table: null };
}

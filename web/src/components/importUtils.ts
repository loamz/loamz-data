import { validName } from "@/kinds";
import type { ImportFileRequest } from "@/api";

export function inferFormat(path: string): "csv" | "parquet" | null {
  const lower = path.toLowerCase().trim();
  if (lower.endsWith(".csv")) return "csv";
  if (lower.endsWith(".parquet")) return "parquet";
  return null;
}

export function deriveDefaultTableName(filePath: string): string {
  const filename = filePath.split("/").pop() ?? "";
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
  volumeId: string;
  filePath: string;
}

export function validateImport({ schema, tableName, volumeId, filePath }: ValidateImportParams): {
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
  if (!volumeId.trim()) {
    return { valid: false, error: "Choose a source volume." };
  }
  if (!filePath.trim()) {
    return { valid: false, error: "Choose a source file." };
  }
  const format = inferFormat(filePath);
  if (!format) {
    return { valid: false, error: "Only CSV and Parquet files can be imported." };
  }
  return { valid: true };
}

export interface BuildImportParams {
  schema: string;
  tableName: string;
  volumeId: string;
  filePath: string;
  mode: "create" | "append";
  format?: "csv" | "parquet";
}

export function buildImportPayload({
  schema,
  tableName,
  volumeId,
  filePath,
  mode,
  format,
}: BuildImportParams): ImportFileRequest {
  const cleanPath = filePath.trim().replace(/^\/+/, "");
  const detectedFormat = format ?? inferFormat(cleanPath) ?? undefined;
  return {
    target: `${schema.trim()}.${tableName.trim()}`,
    volume: volumeId.trim(),
    path: cleanPath,
    mode,
    ...(detectedFormat ? { format: detectedFormat } : {}),
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

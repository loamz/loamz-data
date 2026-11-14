import { describe, expect, it } from "vitest";
import {
  buildImportPayload,
  deriveDefaultTableName,
  inferFormat,
  inferFormatFromUrl,
  parseImportOutput,
  validateImport,
  validateUrl,
  type ValidateImportParams,
} from "./components/importUtils";

describe("importUtils", () => {
  describe("inferFormat", () => {
    it("detects csv from file extension", () => {
      expect(inferFormat("data.csv")).toBe("csv");
      expect(inferFormat("nested/path/to/file.CSV")).toBe("csv");
    });

    it("detects parquet from file extension", () => {
      expect(inferFormat("data.parquet")).toBe("parquet");
      expect(inferFormat("nested/path/to/file.PARQUET")).toBe("parquet");
    });

    it("returns null for unsupported extensions", () => {
      expect(inferFormat("data.txt")).toBeNull();
      expect(inferFormat("data.json")).toBeNull();
      expect(inferFormat("data")).toBeNull();
    });
  });

  describe("inferFormatFromUrl", () => {
    it("detects format from pathname ignoring query parameters and hash", () => {
      expect(inferFormatFromUrl("https://example.com/data.csv")).toBe("csv");
      expect(inferFormatFromUrl("https://s3.amazonaws.com/bucket/orders.parquet?X-Amz-Signature=123")).toBe("parquet");
      expect(inferFormatFromUrl("https://cdn.example.org/files/export.CSV#heading")).toBe("csv");
    });

    it("returns null when url pathname has no recognized extension", () => {
      expect(inferFormatFromUrl("https://example.com/api/data?format=csv")).toBeNull();
      expect(inferFormatFromUrl("https://storage.googleapis.com/b/o/raw_data")).toBeNull();
      expect(inferFormatFromUrl("")).toBeNull();
    });
  });

  describe("validateUrl", () => {
    it("accepts valid https URLs", () => {
      expect(validateUrl("https://example.com/data.parquet")).toEqual({ valid: true });
      expect(validateUrl("https://bucket.s3.amazonaws.com/path/to/file.csv?sig=abc")).toEqual({ valid: true });
      expect(validateUrl("https://example.com:443/data.csv")).toEqual({ valid: true });
    });

    it("rejects empty or non-https URLs", () => {
      expect(validateUrl("").valid).toBe(false);
      expect(validateUrl("http://example.com/data.csv").valid).toBe(false);
      expect(validateUrl("ftp://example.com/data.csv").valid).toBe(false);
    });

    it("rejects addresses containing username or password", () => {
      const res = validateUrl("https://user:pass@example.com/data.csv");
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/user name or password/i);
    });

    it("rejects addresses with non-443 ports", () => {
      const res8080 = validateUrl("https://example.com:8080/data.csv");
      expect(res8080.valid).toBe(false);
      expect(res8080.error).toMatch(/443/);

      const res8443 = validateUrl("https://example.com:8443/data.csv");
      expect(res8443.valid).toBe(false);
      expect(res8443.error).toMatch(/443/);
    });

    it("rejects addresses without host", () => {
      expect(validateUrl("https:///path/to/file").valid).toBe(false);
    });
  });

  describe("deriveDefaultTableName", () => {
    it("derives clean table name from filename or URL pathname", () => {
      expect(deriveDefaultTableName("trips.csv")).toBe("trips");
      expect(deriveDefaultTableName("orders_2026.parquet")).toBe("orders_2026");
      expect(deriveDefaultTableName("folder/subfolder/customer_data.csv")).toBe("customer_data");
      expect(deriveDefaultTableName("https://example.com/exports/sales_data.parquet?token=secret")).toBe("sales_data");
    });

    it("sanitizes dashes and special characters into underscores", () => {
      expect(deriveDefaultTableName("user-events-v1.csv")).toBe("user_events_v1");
      expect(deriveDefaultTableName("https://example.com/raw-taxi-rides.csv")).toBe("raw_taxi_rides");
    });

    it("strips leading digits/symbols to ensure name starts with letter", () => {
      expect(deriveDefaultTableName("2026_sales.csv")).toBe("sales");
      expect(deriveDefaultTableName("_temp_data.csv")).toBe("temp_data");
      expect(deriveDefaultTableName("https://example.com/001_initial.parquet")).toBe("initial");
    });

    it("returns empty string if name cannot be made valid", () => {
      expect(deriveDefaultTableName("1234.csv")).toBe("");
      expect(deriveDefaultTableName("---.csv")).toBe("");
      expect(deriveDefaultTableName("https://example.com/?query=only")).toBe("");
    });
  });

  describe("validateImport", () => {
    const validVolumeParams: ValidateImportParams = {
      schema: "nyc.main",
      tableName: "trips_new",
      sourceType: "volume",
      volumeId: "vol-1234-uuid",
      filePath: "incoming/trips.csv",
      mode: "create",
    };

    const validUrlParams: ValidateImportParams = {
      schema: "nyc.main",
      tableName: "trips_new",
      sourceType: "url",
      url: "https://example.com/data/trips.parquet?sig=abc",
      mode: "create",
    };

    it("accepts valid volume input for create, append, and replace modes", () => {
      expect(validateImport(validVolumeParams)).toEqual({ valid: true });
      expect(validateImport({ ...validVolumeParams, mode: "append" })).toEqual({ valid: true });
      expect(validateImport({ ...validVolumeParams, mode: "replace" })).toEqual({ valid: true });
    });

    it("accepts valid web address input for create, append, and replace modes", () => {
      expect(validateImport(validUrlParams)).toEqual({ valid: true });
      expect(validateImport({ ...validUrlParams, mode: "append" })).toEqual({ valid: true });
      expect(validateImport({ ...validUrlParams, mode: "replace" })).toEqual({ valid: true });
    });

    it("requires explicit format when address has no recognized extension", () => {
      const urlWithoutExt = "https://example.com/stream/export?token=xyz";
      const noFormat = validateImport({
        ...validUrlParams,
        url: urlWithoutExt,
      });
      expect(noFormat.valid).toBe(false);
      expect(noFormat.error).toMatch(/format/i);

      const withCsv = validateImport({
        ...validUrlParams,
        url: urlWithoutExt,
        explicitFormat: "csv",
      });
      expect(withCsv.valid).toBe(true);

      const withParquet = validateImport({
        ...validUrlParams,
        url: urlWithoutExt,
        explicitFormat: "parquet",
      });
      expect(withParquet.valid).toBe(true);
    });

    it("rejects invalid URLs in address source", () => {
      const badHttp = validateImport({
        ...validUrlParams,
        url: "http://example.com/file.csv",
      });
      expect(badHttp.valid).toBe(false);
      expect(badHttp.error).toMatch(/https/i);

      const badPort = validateImport({
        ...validUrlParams,
        url: "https://example.com:8443/file.csv",
      });
      expect(badPort.valid).toBe(false);
      expect(badPort.error).toMatch(/443/);
    });

    it("rejects empty schema", () => {
      const res = validateImport({ ...validVolumeParams, schema: "" });
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/schema/i);
    });

    it("rejects invalid table names", () => {
      for (const bad of ["", "Trips", "trips-new", "123trips", "trips.name", "trips with spaces"]) {
        const res = validateImport({ ...validVolumeParams, tableName: bad });
        expect(res.valid, `Expected '${bad}' to be rejected`).toBe(false);
      }
    });

    it("rejects empty volumeId for volume source", () => {
      const res = validateImport({ ...validVolumeParams, volumeId: "" });
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/volume/i);
    });

    it("rejects empty filePath for volume source", () => {
      const res = validateImport({ ...validVolumeParams, filePath: "" });
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/file/i);
    });

    it("rejects unsupported file formats for volume source", () => {
      const res = validateImport({ ...validVolumeParams, filePath: "incoming/data.json" });
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/csv and parquet/i);
    });
  });

  describe("buildImportPayload", () => {
    describe("volume source combinations", () => {
      it("constructs payload for create mode", () => {
        const req = buildImportPayload({
          schema: "sales.raw",
          tableName: "orders",
          sourceType: "volume",
          volumeId: "vol-uuid-1",
          filePath: "/uploads/orders.csv",
          mode: "create",
        });
        expect(req).toEqual({
          target: "sales.raw.orders",
          volume: "vol-uuid-1",
          path: "uploads/orders.csv",
          mode: "create",
          format: "csv",
        });
      });

      it("constructs payload for append mode with parquet", () => {
        const req = buildImportPayload({
          schema: "analytics.marts",
          tableName: "monthly_revenue",
          sourceType: "volume",
          volumeId: "vol-uuid-2",
          filePath: "exports/rev.parquet",
          mode: "append",
        });
        expect(req).toEqual({
          target: "analytics.marts.monthly_revenue",
          volume: "vol-uuid-2",
          path: "exports/rev.parquet",
          mode: "append",
          format: "parquet",
        });
      });

      it("constructs payload for replace mode", () => {
        const req = buildImportPayload({
          schema: "nyc.main",
          tableName: "trips",
          sourceType: "volume",
          volumeId: "vol-uuid-3",
          filePath: "batches/replacement.csv",
          mode: "replace",
        });
        expect(req).toEqual({
          target: "nyc.main.trips",
          volume: "vol-uuid-3",
          path: "batches/replacement.csv",
          mode: "replace",
          format: "csv",
        });
      });
    });

    describe("web address source combinations", () => {
      it("constructs payload for create mode with detected parquet format without volume or path", () => {
        const req = buildImportPayload({
          schema: "nyc.main",
          tableName: "weather_history",
          sourceType: "url",
          url: "https://example.com/data/weather.parquet?sig=secret123",
          mode: "create",
        });
        expect(req).toEqual({
          target: "nyc.main.weather_history",
          url: "https://example.com/data/weather.parquet?sig=secret123",
          mode: "create",
          format: "parquet",
        });
        expect(req.volume).toBeUndefined();
        expect(req.path).toBeUndefined();
      });

      it("constructs payload for append mode with detected csv format", () => {
        const req = buildImportPayload({
          schema: "nyc.main",
          tableName: "trips",
          sourceType: "url",
          url: "https://example.com/daily.csv",
          mode: "append",
        });
        expect(req).toEqual({
          target: "nyc.main.trips",
          url: "https://example.com/daily.csv",
          mode: "append",
          format: "csv",
        });
      });

      it("constructs payload for replace mode with address lacking extension and explicit format", () => {
        const req = buildImportPayload({
          schema: "nyc.main",
          tableName: "trips",
          sourceType: "url",
          url: "https://example.com/export/all_data?token=foo",
          mode: "replace",
          format: "parquet",
        });
        expect(req).toEqual({
          target: "nyc.main.trips",
          url: "https://example.com/export/all_data?token=foo",
          mode: "replace",
          format: "parquet",
        });
        expect(req.volume).toBeUndefined();
        expect(req.path).toBeUndefined();
      });
    });
  });

  describe("parseImportOutput", () => {
    it("extracts rows and table from json in stdout", () => {
      const stdout = 'Loading extensions...\nAttached catalog\n{"rows": 3450, "table": "nyc.main.trips"}\nDone\n';
      expect(parseImportOutput(stdout)).toEqual({
        rows: 3450,
        table: "nyc.main.trips",
      });
    });

    it("handles zero rows", () => {
      const stdout = '{"rows": 0, "table": "nyc.main.empty"}';
      expect(parseImportOutput(stdout)).toEqual({
        rows: 0,
        table: "nyc.main.empty",
      });
    });

    it("returns nulls if stdout has no valid json line", () => {
      expect(parseImportOutput("")).toEqual({ rows: null, table: null });
      expect(parseImportOutput("Some error occurred")).toEqual({ rows: null, table: null });
    });
  });
});

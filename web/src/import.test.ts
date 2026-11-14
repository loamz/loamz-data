import { describe, expect, it } from "vitest";
import {
  buildImportPayload,
  deriveDefaultTableName,
  inferFormat,
  parseImportOutput,
  validateImport,
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

  describe("deriveDefaultTableName", () => {
    it("derives clean table name from filename", () => {
      expect(deriveDefaultTableName("trips.csv")).toBe("trips");
      expect(deriveDefaultTableName("orders_2026.parquet")).toBe("orders_2026");
      expect(deriveDefaultTableName("folder/subfolder/customer_data.csv")).toBe("customer_data");
    });

    it("sanitizes dashes and special characters into underscores", () => {
      expect(deriveDefaultTableName("user-events-v1.csv")).toBe("user_events_v1");
    });

    it("strips leading digits/symbols to ensure name starts with letter", () => {
      expect(deriveDefaultTableName("2026_sales.csv")).toBe("sales");
      expect(deriveDefaultTableName("_temp_data.csv")).toBe("temp_data");
    });

    it("returns empty string if name cannot be made valid", () => {
      expect(deriveDefaultTableName("1234.csv")).toBe("");
      expect(deriveDefaultTableName("---.csv")).toBe("");
    });
  });

  describe("validateImport", () => {
    const validParams = {
      schema: "nyc.main",
      tableName: "trips_new",
      volumeId: "vol-1234-uuid",
      filePath: "incoming/trips.csv",
    };

    it("accepts valid input", () => {
      expect(validateImport(validParams)).toEqual({ valid: true });
    });

    it("rejects empty schema", () => {
      const res = validateImport({ ...validParams, schema: "" });
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/schema/i);
    });

    it("rejects invalid table names", () => {
      for (const bad of ["", "Trips", "trips-new", "123trips", "trips.name", "trips with spaces"]) {
        const res = validateImport({ ...validParams, tableName: bad });
        expect(res.valid, `Expected '${bad}' to be rejected`).toBe(false);
      }
    });

    it("rejects empty volumeId", () => {
      const res = validateImport({ ...validParams, volumeId: "" });
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/volume/i);
    });

    it("rejects empty filePath", () => {
      const res = validateImport({ ...validParams, filePath: "" });
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/file/i);
    });

    it("rejects unsupported file formats", () => {
      const res = validateImport({ ...validParams, filePath: "incoming/data.json" });
      expect(res.valid).toBe(false);
      expect(res.error).toMatch(/csv and parquet/i);
    });
  });

  describe("buildImportPayload", () => {
    it("constructs payload for create mode", () => {
      const req = buildImportPayload({
        schema: "sales.raw",
        tableName: "orders",
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

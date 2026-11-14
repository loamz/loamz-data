import { AlertTriangle, CheckCircle2, ChevronRight, FileUp, Loader2, Table2, Upload, XCircle } from "lucide-react";
import { useEffect, useState } from "react";
import * as api from "@/api";
import { StateBadge } from "@/components/RunBits";
import { Button, Dialog, DialogContent, DialogFooter, FilePicker, Input, Segmented, Select, toast } from "@/design";
import { go } from "@/route";
import {
  buildImportPayload,
  deriveDefaultTableName,
  inferFormat,
  inferFormatFromUrl,
  parseImportOutput,
  validateImport,
  type ImportMode,
  type ImportSourceType,
} from "./importUtils";

export interface ImportDialogProps {
  ws: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  volumeId?: string;
  filePath?: string;
  initialTarget?: string;
}

interface VolumeOption {
  id: string;
  name: string;
  fullName: string;
}

export function ImportDialog({
  ws,
  open,
  onOpenChange,
  volumeId: propVolumeId,
  filePath: propFilePath,
  initialTarget,
}: ImportDialogProps) {
  const [schemas, setSchemas] = useState<{ value: string; label: string }[]>([]);
  const [selectedSchema, setSelectedSchema] = useState("");
  const [tableName, setTableName] = useState("");
  const [mode, setMode] = useState<ImportMode>("create");

  // Source selection: volume vs web address
  const [sourceType, setSourceType] = useState<ImportSourceType>("volume");
  const [webUrl, setWebUrl] = useState("");
  const [urlFormat, setUrlFormat] = useState<"csv" | "parquet" | "">("");

  // Volume & file selection (when not pre-selected)
  const [volumes, setVolumes] = useState<VolumeOption[]>([]);
  const [chosenVolumeId, setChosenVolumeId] = useState(propVolumeId ?? "");
  const [volumeFiles, setVolumeFiles] = useState<api.FileEntry[]>([]);
  const [chosenPath, setChosenPath] = useState(propFilePath ?? "");
  const [uploading, setUploading] = useState(false);

  // Run progress state
  const [submitting, setSubmitting] = useState(false);
  const [run, setRun] = useState<api.Run | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rowCount, setRowCount] = useState<number | null>(null);
  const [targetTableId, setTargetTableId] = useState<string | null>(null);

  // Initialize dialog state when opened
  useEffect(() => {
    if (!open) {
      setRun(null);
      setError(null);
      setRowCount(null);
      setTargetTableId(null);
      setSubmitting(false);
      return;
    }

    let live = true;
    const effectiveVolumeId = propVolumeId ?? "";
    const effectivePath = propFilePath ?? "";
    setSourceType(propFilePath ? "volume" : "volume");
    setWebUrl("");
    setUrlFormat("");
    setChosenVolumeId(effectiveVolumeId);
    setChosenPath(effectivePath);
    setRun(null);
    setError(null);
    setRowCount(null);
    setTargetTableId(null);

    // Initial target parsing
    if (initialTarget) {
      const parts = initialTarget.split(".");
      if (parts.length === 3) {
        setSelectedSchema(`${parts[0]}.${parts[1]}`);
        setTableName(parts[2]);
        setMode("append");
      }
    } else {
      setMode("create");
      if (effectivePath) {
        setTableName(deriveDefaultTableName(effectivePath));
      } else {
        setTableName("");
      }
    }

    // Load available catalogs & schemas
    void (async () => {
      try {
        const cats = await api.list(ws, "");
        if (!live) return;
        const schemaOpts: { value: string; label: string }[] = [];
        const volOpts: VolumeOption[] = [];

        for (const cat of cats) {
          const scs = await api.list(ws, cat.id);
          if (!live) return;
          for (const sc of scs) {
            const scFullName = `${cat.name}.${sc.name}`;
            schemaOpts.push({ value: scFullName, label: scFullName });

            // If we need volumes, list schema children
            if (!propVolumeId) {
              const children = await api.list(ws, sc.id);
              if (!live) return;
              for (const child of children) {
                if (child.kind === "volume") {
                  volOpts.push({
                    id: child.id,
                    name: child.name,
                    fullName: `${scFullName}.${child.name}`,
                  });
                }
              }
            }
          }
        }

        if (!live) return;
        setSchemas(schemaOpts);
        if (!initialTarget && schemaOpts.length > 0 && !selectedSchema) {
          setSelectedSchema(schemaOpts[0].value);
        }

        if (!propVolumeId) {
          setVolumes(volOpts);
          if (volOpts.length > 0 && !effectiveVolumeId) {
            setChosenVolumeId(volOpts[0].id);
          }
        }
      } catch (e) {
        if (live) setError((e as Error).message);
      }
    })();

    return () => {
      live = false;
    };
  }, [open, ws, propVolumeId, propFilePath, initialTarget]);

  // Load files when volume changes
  useEffect(() => {
    if (!open || !chosenVolumeId) return;
    let live = true;
    api.files(ws, chosenVolumeId).then(
      (items) => {
        if (!live) return;
        const importable = items.filter((f) => inferFormat(f.path) !== null);
        setVolumeFiles(importable);
        if (!propFilePath) {
          if (importable.length > 0 && (!chosenPath || !importable.some((f) => f.path === chosenPath))) {
            setChosenPath(importable[0].path);
            if (!initialTarget) {
              setTableName(deriveDefaultTableName(importable[0].path));
            }
          }
        }
      },
      () => {
        if (live) setVolumeFiles([]);
      },
    );
    return () => {
      live = false;
    };
  }, [open, ws, chosenVolumeId, propFilePath]);

  const activeVolumeId = propVolumeId || chosenVolumeId;
  const activePath = propFilePath || chosenPath;
  const targetFullName = selectedSchema && tableName ? `${selectedSchema}.${tableName}` : "";
  const detectedUrlFormat = inferFormatFromUrl(webUrl);
  const effectiveUrlFormat = urlFormat || detectedUrlFormat || "";

  // Upload handler for adding a file to the chosen volume
  async function handleUpload(fileList: File[]) {
    const volId = activeVolumeId || chosenVolumeId || volumes[0]?.id;
    if (!volId || fileList.length === 0) return;
    const file = fileList[0];
    setUploading(true);
    try {
      await api.upload(ws, volId, file.name, file);
      toast.success(`Uploaded ${file.name}`);
      const updated = await api.files(ws, volId);
      const importable = updated.filter((f) => inferFormat(f.path) !== null);
      setVolumeFiles(importable);
      setChosenPath(file.name);
      if (!chosenVolumeId) setChosenVolumeId(volId);
      if (!initialTarget) {
        setTableName(deriveDefaultTableName(file.name));
      }
    } catch (err) {
      toast.error(`Could not upload ${file.name}`, (err as Error).message);
    } finally {
      setUploading(false);
    }
  }

  // Submit and poll import
  async function handleStartImport() {
    const val = validateImport({
      schema: selectedSchema,
      tableName,
      sourceType,
      volumeId: activeVolumeId,
      filePath: activePath,
      url: webUrl,
      mode,
      explicitFormat: urlFormat,
    });
    if (!val.valid) {
      setError(val.error ?? "Invalid input");
      return;
    }

    setSubmitting(true);
    setError(null);
    setRun(null);
    setRowCount(null);
    setTargetTableId(null);

    const payload = buildImportPayload({
      schema: selectedSchema,
      tableName,
      sourceType,
      volumeId: activeVolumeId,
      filePath: activePath,
      url: webUrl,
      mode,
      format: urlFormat,
    });

    try {
      let r = await api.importFile(ws, payload);
      setRun(r);
      setSubmitting(false);

      // Poll until finished
      while (api.isRunning(r)) {
        await new Promise((resolve) => setTimeout(resolve, 400));
        r = await api.run(ws, r.id);
        setRun(r);
      }

      if (r.state === "succeeded") {
        const out = parseImportOutput(r.stdout);
        setRowCount(out.rows);

        // Resolve target table securable ID for linking
        try {
          const [catName, scName] = selectedSchema.split(".");
          const cats = await api.list(ws, "");
          const cat = cats.find((c) => c.name === catName);
          if (cat) {
            const scs = await api.list(ws, cat.id);
            const sc = scs.find((s) => s.name === scName);
            if (sc) {
              const items = await api.list(ws, sc.id);
              const tbl = items.find((t) => t.name === tableName && t.kind === "table");
              if (tbl) setTargetTableId(tbl.id);
            }
          }
        } catch {
          // non-fatal if securable resolution fails
        }
      } else {
        setError(r.error || r.stderr || `Import ${r.state.replace("_", " ")}.`);
      }
    } catch (err) {
      setSubmitting(false);
      setError((err as Error).message);
    }
  }

  const isExecuting = submitting || (run !== null && api.isRunning(run));
  const hasFinished = run !== null && !api.isRunning(run);
  const isSuccess = hasFinished && run.state === "succeeded";

  const canSubmit =
    !isExecuting &&
    Boolean(selectedSchema) &&
    Boolean(tableName) &&
    (sourceType === "volume"
      ? Boolean(activeVolumeId) && Boolean(activePath)
      : Boolean(webUrl.trim()) && Boolean(effectiveUrlFormat));

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        title={initialTarget ? `Import into ${initialTarget}` : "Import as table"}
        description={
          hasFinished
            ? isSuccess
              ? "The import job completed successfully."
              : "The import job failed."
            : isExecuting
              ? "Running import job on workspace compute…"
              : "Import a CSV or Parquet file from a volume or web address into an Iceberg table."
        }
      >
        <div className="mt-4 space-y-4">
          {/* Ongoing execution view (NEVER echo web address here) */}
          {isExecuting && (
            <div className="space-y-4 rounded-xl border border-border bg-background/50 p-4">
              <div className="flex items-center gap-3">
                <Loader2 className="size-5 animate-spin text-accent" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-foreground">
                      {sourceType === "url" ? "Importing from web address" : `Importing ${activePath}`}
                    </span>
                    {run && <StateBadge state={run.state} />}
                  </div>
                  <p className="truncate text-xs text-muted">
                    Target: {targetFullName} · Mode: {mode === "create" ? "new table" : mode === "append" ? "append" : "replace rows"}
                  </p>
                </div>
              </div>
            </div>
          )}

          {/* Finished view: Success (NEVER echo web address here) */}
          {isSuccess && (
            <div className="space-y-4 rounded-xl border border-border bg-background/50 p-4">
              <div className="flex items-start gap-3">
                <CheckCircle2 className="mt-0.5 size-5 shrink-0 text-success" />
                <div className="space-y-1">
                  <h4 className="text-sm font-medium text-foreground">Import complete</h4>
                  <p className="text-xs text-muted">
                    {rowCount !== null
                      ? `Imported ${rowCount.toLocaleString()} ${rowCount === 1 ? "row" : "rows"} into ${targetFullName}.`
                      : `Data imported successfully into ${targetFullName}.`}
                  </p>
                </div>
              </div>

              {targetTableId && (
                <div className="pt-2">
                  <Button
                    variant="primary"
                    onClick={() => {
                      onOpenChange(false);
                      go({ screen: "catalog", ws, id: targetTableId });
                    }}
                  >
                    <Table2 /> Open table <ChevronRight />
                  </Button>
                </div>
              )}
            </div>
          )}

          {/* Error view */}
          {error && (
            <div className="flex items-start gap-3 rounded-xl border border-danger/30 bg-danger/10 p-4 text-xs text-danger">
              <XCircle className="mt-0.5 size-4 shrink-0 text-danger" />
              <div className="flex-1 space-y-1">
                <span className="font-medium">Import error:</span>
                <p className="break-words font-mono text-2xs leading-relaxed">{error}</p>
              </div>
            </div>
          )}

          {/* Configuration form (visible before submission or if trying again) */}
          {!isExecuting && !isSuccess && (
            <div className="space-y-4">
              {/* Source Choice if not fixed to a specific volume file */}
              {!propFilePath && (
                <div className="space-y-1.5">
                  <span className="block text-xs font-medium text-foreground">Source</span>
                  <Segmented
                    label="Source choice"
                    value={sourceType}
                    onChange={(v) => {
                      setSourceType(v as ImportSourceType);
                      setError(null);
                    }}
                    options={[
                      { value: "volume", label: "From a volume" },
                      { value: "url", label: "From a web address" },
                    ]}
                  />
                </div>
              )}

              {/* Volume source controls */}
              {sourceType === "volume" && (
                <>
                  {!propVolumeId && (
                    <label className="block space-y-1.5">
                      <span className="text-xs font-medium text-foreground">Source Volume</span>
                      <Select
                        aria-label="Source Volume"
                        value={chosenVolumeId}
                        onValueChange={setChosenVolumeId}
                        options={volumes.map((v) => ({ value: v.id, label: v.fullName }))}
                        placeholder="Select volume…"
                      />
                    </label>
                  )}

                  {!propFilePath && (
                    <div className="space-y-1.5">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium text-foreground">File</span>
                        <FilePicker
                          variant="ghost"
                          size="sm"
                          onFiles={(files) => void handleUpload(files)}
                          disabled={uploading || (!activeVolumeId && volumes.length === 0)}
                        >
                          <Upload className="size-3.5" /> {uploading ? "Uploading…" : "Upload file"}
                        </FilePicker>
                      </div>
                      {volumeFiles.length === 0 ? (
                        <p className="rounded-md border border-border/60 bg-muted/20 px-3 py-2 text-xs text-muted">
                          No CSV or Parquet files in this volume. Upload one to continue.
                        </p>
                      ) : (
                        <Select
                          aria-label="Source File"
                          value={chosenPath}
                          onValueChange={(val) => {
                            setChosenPath(val);
                            if (!initialTarget) {
                              setTableName(deriveDefaultTableName(val));
                            }
                          }}
                          options={volumeFiles.map((f) => ({ value: f.path, label: f.path }))}
                        />
                      )}
                    </div>
                  )}

                  {/* Readonly info if source file was already selected */}
                  {propFilePath && (
                    <div className="rounded-lg border border-border/60 bg-muted/20 p-2.5 text-xs">
                      <span className="text-muted">Source file: </span>
                      <span className="font-mono text-foreground">{propFilePath}</span>
                      <span className="ml-2 text-2xs text-muted">({inferFormat(propFilePath)?.toUpperCase()})</span>
                    </div>
                  )}
                </>
              )}

              {/* Web address source controls */}
              {sourceType === "url" && (
                <div className="space-y-3">
                  <label className="block space-y-1.5">
                    <span className="text-xs font-medium text-foreground">Web address</span>
                    <Input
                      aria-label="Web address"
                      value={webUrl}
                      onChange={(e) => {
                        const v = e.target.value;
                        setWebUrl(v);
                        if (!initialTarget && v) {
                          const derived = deriveDefaultTableName(v);
                          if (derived) setTableName(derived);
                        }
                      }}
                      placeholder="https://example.com/data.parquet"
                      spellCheck={false}
                    />
                    <span className="block text-2xs text-muted">
                      An https:// address (port 443, no user name or password). A signed address works; it is never shown back.
                    </span>
                  </label>

                  {/* Format picker for address */}
                  <label className="block space-y-1.5">
                    <span className="text-xs font-medium text-foreground">
                      File format {detectedUrlFormat ? `(detected: ${detectedUrlFormat.toUpperCase()})` : "(required)"}
                    </span>
                    <Select
                      aria-label="File format"
                      value={effectiveUrlFormat}
                      onValueChange={(val) => setUrlFormat(val as "csv" | "parquet")}
                      options={[
                        { value: "csv", label: "CSV" },
                        { value: "parquet", label: "Parquet" },
                      ]}
                      placeholder="Select format…"
                    />
                    {!detectedUrlFormat && (
                      <span className="block text-2xs text-muted">
                        This address does not end in .csv or .parquet. Choose the format of the file.
                      </span>
                    )}
                  </label>
                </div>
              )}

              {/* Target Schema & Table Name */}
              <div className="grid grid-cols-2 gap-3">
                <label className="block space-y-1.5">
                  <span className="text-xs font-medium text-foreground">Schema</span>
                  <Select
                    aria-label="Target Schema"
                    value={selectedSchema}
                    onValueChange={setSelectedSchema}
                    options={schemas}
                    placeholder="Select schema…"
                  />
                </label>

                <label className="block space-y-1.5">
                  <span className="text-xs font-medium text-foreground">Table Name</span>
                  <Input
                    aria-label="Table Name"
                    value={tableName}
                    onChange={(e) => setTableName(e.target.value)}
                    placeholder="table_name"
                    spellCheck={false}
                  />
                </label>
              </div>

              {/* Mode Selection */}
              <div className="space-y-1.5">
                <span className="block text-xs font-medium text-foreground">Mode</span>
                <Segmented
                  label="Import mode"
                  value={mode}
                  onChange={(m) => setMode(m as ImportMode)}
                  options={[
                    { value: "create", label: "New table" },
                    { value: "append", label: "Add rows" },
                    { value: "replace", label: "Replace rows" },
                  ]}
                />
                <span className="block text-2xs text-muted">
                  {mode === "create"
                    ? "Creates a new Iceberg table matching the file schema."
                    : mode === "append"
                      ? "Appends rows to an existing Iceberg table."
                      : "Replaces all existing rows in the table with the file's rows."}
                </span>

                {mode === "replace" && (
                  <div className="mt-2 flex items-start gap-2 rounded-lg border border-warning/40 bg-warning/10 p-2.5 text-xs text-warning">
                    <AlertTriangle className="mt-0.5 size-4 shrink-0 text-warning" />
                    <span>
                      <strong>Warning:</strong> The table&apos;s current rows will be completely removed and replaced with the imported rows. Existing table columns are kept and must match the file schema.
                    </span>
                  </div>
                )}
              </div>
            </div>
          )}

          <DialogFooter>
            {hasFinished ? (
              <Button variant="secondary" onClick={() => onOpenChange(false)}>
                Done
              </Button>
            ) : (
              <>
                <Button variant="secondary" onClick={() => onOpenChange(false)} disabled={isExecuting}>
                  Cancel
                </Button>
                <Button
                  variant="primary"
                  onClick={() => void handleStartImport()}
                  disabled={!canSubmit}
                >
                  <FileUp /> {isExecuting ? "Importing…" : "Start import"}
                </Button>
              </>
            )}
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
}

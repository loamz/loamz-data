import { Download, File, Trash2, Upload } from "lucide-react";
import { useState } from "react";
import { Button, FilePicker, PageEmpty, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { bytes } from "@/format";
import { useLoad } from "@/hooks";

function save(blob: Blob, name: string) {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name.split("/").pop() ?? "file";
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

export function FilesTab({ volume }: { volume: string }) {
  const list = useLoad(() => api.files(volume), volume);
  const [uploading, setUploading] = useState(0);
  const [over, setOver] = useState(false);

  async function send(chosen: File[]) {
    setUploading((n) => n + chosen.length);
    for (const f of chosen) {
      try {
        await api.upload(volume, f.name, f);
        toast.success(`Uploaded ${f.name}`);
      } catch (err) {
        toast.error(`Could not upload ${f.name}`, (err as Error).message);
      } finally {
        setUploading((n) => n - 1);
      }
    }
    list.reload();
  }

  const items = list.data ?? [];
  return (
    <div
      className="space-y-4"
      onDragOver={(e) => {
        e.preventDefault();
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        e.preventDefault();
        setOver(false);
        void send(Array.from(e.dataTransfer.files));
      }}
    >
      <div className="flex items-center gap-2">
        <p className="flex-1 text-xs text-muted">{list.loading && !list.data ? "Loading…" : `${items.length} ${items.length === 1 ? "file" : "files"}${uploading ? ` · uploading ${uploading}` : ""}`}</p>
        <FilePicker variant="primary" onFiles={(f) => void send(f)} disabled={uploading > 0}>
          <Upload /> Upload
        </FilePicker>
      </div>

      {list.error && !list.data ? (
        <PageEmpty icon={File} title="You can't open the files here">
          {list.error.message}
        </PageEmpty>
      ) : items.length === 0 && !list.loading ? (
        <PageEmpty icon={Upload} title="No files yet">
          Drop files here, or use Upload.
        </PageEmpty>
      ) : (
        <div className={`divide-y divide-border rounded-xl border bg-background/40 transition-colors ${over ? "border-accent" : "border-border"}`}>
          {items.map((f) => (
            <div key={f.path} className="flex items-center gap-3 px-4 py-2">
              <File className="shrink-0 text-muted" aria-hidden />
              <span className="min-w-0 flex-1 truncate text-sm text-foreground" title={f.path}>
                {f.path}
              </span>
              <span className="shrink-0 text-xs tabular-nums text-muted">{bytes(f.size)}</span>
              <Button
                variant="ghost"
                size="icon-sm"
                aria-label={`Download ${f.path}`}
                onClick={async () => {
                  try {
                    save(await api.download(volume, f.path), f.path);
                  } catch (err) {
                    toast.error("Could not download", (err as Error).message);
                  }
                }}
              >
                <Download />
              </Button>
              <Button
                variant="danger"
                size="icon-sm"
                aria-label={`Delete ${f.path}`}
                onClick={async () => {
                  if (!(await confirmAction({ title: `Delete ${f.path}?`, description: "This file is removed for good.", confirmLabel: "Delete", danger: true }))) return;
                  try {
                    await api.removeFile(volume, f.path);
                    list.reload();
                  } catch (err) {
                    toast.error("Could not delete", (err as Error).message);
                  }
                }}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

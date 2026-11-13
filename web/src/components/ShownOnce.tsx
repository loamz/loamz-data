import { Copy } from "lucide-react";
import { Button, Dialog, DialogContent, DialogFooter, toast } from "@/design";

/** Something that is shown once and never again (a token, a password): the text to keep, with a button to copy it. */
export function ShownOnce({ title, description, text, onClose }: { title: string; description: string; text: string | null; onClose: () => void }) {
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text ?? "");
      toast.success("Copied");
    } catch {
      toast.error("Could not copy", "Select the text and copy it.");
    }
  };
  return (
    <Dialog open={text !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent title={title} description={description} className="max-w-lg">
        <pre className="scroll-area mt-4 whitespace-pre-wrap break-all rounded-md border border-border bg-background/50 p-3 font-mono text-xs text-foreground">{text}</pre>
        <DialogFooter>
          <Button variant="secondary" onClick={() => void copy()}>
            <Copy /> Copy
          </Button>
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

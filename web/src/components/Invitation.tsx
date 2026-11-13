import { Copy } from "lucide-react";
import { Button, Dialog, DialogContent, DialogFooter, toast } from "@/design";
import * as api from "@/api";

/** What to tell someone just added: their email and, if they were made now, the temporary password (shown once, changed at first sign-in). */
export function Invitation({ invited, onClose }: { invited: api.Invited | null; onClose: () => void }) {
  const text = invited?.temporary_password ? `Email: ${invited.email}\nTemporary password: ${invited.temporary_password}` : "";
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success("Copied");
    } catch {
      toast.error("Could not copy", "Select the text and copy it.");
    }
  };
  return (
    <Dialog open={invited !== null} onOpenChange={(o) => !o && onClose()}>
      <DialogContent
        title={invited?.temporary_password ? "Give them this password" : "Added"}
        description={
          invited?.temporary_password
            ? "It is shown once. They must change it the first time they sign in."
            : "They already had an account, so they sign in as before."
        }
        className="max-w-lg"
      >
        {invited?.temporary_password && (
          <pre className="scroll-area mt-4 whitespace-pre-wrap break-all rounded-md border border-border bg-background/50 p-3 font-mono text-xs text-foreground">{text}</pre>
        )}
        <DialogFooter>
          {invited?.temporary_password && (
            <Button variant="secondary" onClick={() => void copy()}>
              <Copy /> Copy
            </Button>
          )}
          <Button variant="primary" onClick={onClose}>
            Done
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

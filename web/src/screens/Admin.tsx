import { UserX } from "lucide-react";
import { useEffect, useState } from "react";
import { Badge, Button, Input, Page, SettingGroup, SettingRow, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { AccessList } from "@/components/AccessList";
import { shortId } from "@/kinds";

export function Admin({ me }: { me: api.Me | undefined }) {
  return (
    <Page title="Admin" subtitle="Who can reach the whole space, and removing a person's data." layout="columns">
      <SettingGroup title="You">
        <SettingRow label="Signed in as" description={me?.agent ? `An agent (${shortId(me.agent)}) acting for this person.` : undefined}>
          <span className="font-mono text-xs" title={me?.subject}>{me ? shortId(me.subject) : "…"}</span>
        </SettingRow>
        <SettingRow label="Space" description="Everything here belongs to this space.">
          <span className="text-xs">{me ? (me.tenant.startsWith("u-") ? "Personal" : me.tenant.slice(2)) : "…"}</span>
          {me?.tenant_owner && <Badge tone="accent">owner</Badge>}
        </SettingRow>
      </SettingGroup>

      <Erase />

      <section className="space-y-2" data-span="all">
        <div className="px-1">
          <h2 className="text-sm font-semibold text-foreground">Access to the whole space</h2>
          <p className="mt-0.5 text-xs leading-relaxed text-muted">Owners can create catalogs and manage everything. Readers and contributors here see and write across every catalog.</p>
        </div>
        <AccessList id="root" />
      </section>
    </Page>
  );
}

function Erase() {
  const [subject, setSubject] = useState("");
  const [taskId, setTaskId] = useState("");
  const [task, setTask] = useState<api.Task | null>(null);

  // Follow the task until it settles: an erase is a job that runs in the background and checks itself.
  useEffect(() => {
    if (!taskId) return;
    let stop = false;
    const tick = async () => {
      try {
        const t = await api.task(taskId);
        if (stop) return;
        setTask(t);
        if (t.state === "pending") setTimeout(tick, 1500);
      } catch {
        /* the next click will show the problem */
      }
    };
    void tick();
    return () => {
      stop = true;
    };
  }, [taskId]);

  return (
    <SettingGroup title="Erase a person" description="Removes everything they own here, the files inside it, and every access they hold. The record of what happened is kept.">
      <SettingRow label="Person id" description="Their sign-in id, as shown in Activity and Access.">
        <Input className="w-64" aria-label="Person id" value={subject} onChange={(e) => setSubject(e.target.value)} placeholder="id to erase" spellCheck={false} />
        <Button
          variant="danger"
          disabled={!subject.trim()}
          onClick={async () => {
            const s = subject.trim();
            if (!(await confirmAction({ title: "Erase this person's data?", description: "Their catalogs, volumes, files and access are removed for good. This can't be undone.", confirmLabel: "Erase", danger: true }))) return;
            try {
              const r = await api.erase(s);
              setTask(null);
              setTaskId(r.task_id);
              setSubject("");
              toast.info("Erase started");
            } catch (e) {
              toast.error("Could not start the erase", (e as Error).message);
            }
          }}
        >
          <UserX /> Erase
        </Button>
      </SettingRow>
      {taskId && (
        <SettingRow label="Progress" description={task?.state === "failed" ? task.last_error || "It gave up after several tries." : task?.state === "done" ? "Everything was removed and checked." : "Working. It keeps going even if the server restarts."}>
          <Badge tone={task?.state === "done" ? "success" : task?.state === "failed" ? "danger" : "neutral"}>{task?.state ?? "starting"}</Badge>
        </SettingRow>
      )}
    </SettingGroup>
  );
}

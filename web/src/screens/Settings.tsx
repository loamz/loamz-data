import { Trash2, UserMinus } from "lucide-react";
import { useState } from "react";
import { Badge, Button, Input, Page, PageEmpty, Section, Select, SettingGroup, SettingRow, confirmAction, toast } from "@/design";
import * as api from "@/api";
import { Principal } from "@/components/Principal";
import { PrincipalField } from "@/components/PrincipalField";
import { WorkspaceSwitcher } from "@/components/WorkspaceSwitcher";
import { useLoad } from "@/hooks";
import { go } from "@/route";

/** A workspace's members, what they are entitled to, and which catalogs it can see. For the workspace's admins. */
export function Settings({ ws, me }: { ws: string; me: api.Me | undefined }) {
  const w = useLoad(() => api.workspace(ws), ws);
  const here = w.data;
  if (w.error) {
    return (
      <Page title="Workspace">
        <PageEmpty icon={Trash2} title="This workspace isn't here, or you are not part of it">{w.error.message}</PageEmpty>
      </Page>
    );
  }
  if (!here) return <Page title="Workspace settings">{null}</Page>;
  return (
    <Page
      title={here.name}
      subtitle={here.admin ? "Who works here, what they may do, and which catalogs they can see." : "You are a user of this workspace. Admins manage who works here."}
      actions={me && <WorkspaceSwitcher current={ws} all={me.workspaces} where="settings" />}
    >
      {here.admin ? (
        <>
          <Members ws={ws} />
          <Entitlements ws={ws} />
          <Bindings ws={ws} accountAdmin={here.account_admin} />
          {here.account_admin && <Danger ws={ws} workspace={here} />}
        </>
      ) : (
        <PageEmpty icon={UserMinus} title="Only admins can change this">
          Ask an admin of {here.name}.
        </PageEmpty>
      )}
    </Page>
  );
}

function Members({ ws }: { ws: string }) {
  const list = useLoad(() => api.members(ws), ws);
  const [principal, setPrincipal] = useState("");
  const [role, setRole] = useState("user");
  return (
    <Section title="Members" description="People, groups and service principals assigned to this workspace. An admin manages the workspace; a user works in it.">
      <form
        className="mb-3 flex flex-wrap items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!principal) return;
          try {
            await api.assignMember(ws, principal, role as "admin" | "user");
            toast.success("Assigned");
            list.reload();
          } catch (err) {
            toast.error("Could not assign", (err as Error).message);
          }
        }}
      >
        <PrincipalField onChange={setPrincipal} className="min-w-72" />
        <Select className="w-32" aria-label="Role" value={role} onValueChange={setRole} options={[{ value: "user", label: "User" }, { value: "admin", label: "Admin" }]} />
        <Button variant="primary" type="submit" disabled={!principal}>
          Assign
        </Button>
      </form>
      <div className="divide-y divide-border rounded-lg border border-border">
        {(list.data ?? []).map((m) => (
          <div key={m.principal} className="flex items-center gap-3 px-4 py-2.5">
            <span className="min-w-0 flex-1">
              <Principal id={m.principal} />
            </span>
            <Select
              className="w-28"
              size="sm"
              aria-label={`Role of ${m.principal}`}
              value={m.role}
              onValueChange={async (r) => {
                try {
                  await api.assignMember(ws, m.principal, r as "admin" | "user");
                  list.reload();
                } catch (err) {
                  toast.error("Could not change the role", (err as Error).message);
                }
              }}
              options={[{ value: "user", label: "User" }, { value: "admin", label: "Admin" }]}
            />
            <Button
              variant="danger"
              size="icon-sm"
              aria-label={`Remove ${m.principal}`}
              onClick={async () => {
                if (!(await confirmAction({ title: "Remove from the workspace?", description: "They can no longer open it. Their access to data is unchanged.", confirmLabel: "Remove", danger: true }))) return;
                try {
                  await api.removeMember(ws, m.principal);
                  list.reload();
                } catch (err) {
                  toast.error("Could not remove", (err as Error).message);
                }
              }}
            >
              <UserMinus />
            </Button>
          </div>
        ))}
      </div>
    </Section>
  );
}

function Entitlements({ ws }: { ws: string }) {
  const members = useLoad(() => api.members(ws), ws);
  const ent = useLoad(() => api.entitlements(ws), ws);
  const [who, setWho] = useState("");
  const [what, setWhat] = useState("");
  const available = ent.data?.available ?? [];
  return (
    <Section title="Entitlements" description="What a member may do here beyond opening it. Workspace admins have them all.">
      <form
        className="mb-3 flex flex-wrap items-center gap-2"
        onSubmit={async (e) => {
          e.preventDefault();
          try {
            await api.entitle(ws, who, what);
            ent.reload();
          } catch (err) {
            toast.error("Could not give it", (err as Error).message);
          }
        }}
      >
        <Select className="min-w-64 flex-1" aria-label="Member" placeholder="Member" value={who} onValueChange={setWho} options={(members.data ?? []).map((m) => ({ value: m.principal, label: m.principal }))} />
        <Select className="w-48" aria-label="Entitlement" placeholder="Entitlement" value={what} onValueChange={setWhat} options={available.map((a) => ({ value: a, label: a }))} />
        <Button variant="primary" type="submit" disabled={!who || !what}>
          Give
        </Button>
      </form>
      <div className="divide-y divide-border rounded-lg border border-border">
        {(ent.data?.items ?? []).length === 0 && <p className="px-4 py-5 text-center text-xs text-muted">No entitlements given.</p>}
        {(ent.data?.items ?? []).map((e) => (
          <div key={`${e.principal}/${e.entitlement}`} className="flex items-center gap-3 px-4 py-2.5">
            <span className="min-w-0 flex-1">
              <Principal id={e.principal} />
            </span>
            <Badge>{e.entitlement}</Badge>
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={`Take ${e.entitlement} from ${e.principal}`}
              onClick={async () => {
                await api.unentitle(ws, e.principal, e.entitlement).catch((err: Error) => toast.error("Could not take it", err.message));
                ent.reload();
              }}
            >
              <UserMinus />
            </Button>
          </div>
        ))}
      </div>
    </Section>
  );
}

function Bindings({ ws, accountAdmin }: { ws: string; accountAdmin: boolean }) {
  const bound = useLoad(() => api.boundCatalogs(ws), ws);
  const catalogs = useLoad(() => api.list(ws, ""), ws);
  const [pick, setPick] = useState("");
  const names = Object.fromEntries((catalogs.data ?? []).map((c) => [c.id, c.name]));
  return (
    <Section title="Catalogs" description="A catalog bound to workspaces shows only in those. A catalog bound to none shows in every workspace of the organization.">
      {accountAdmin && (
        <div className="mb-3 flex items-center gap-2">
          <Select className="min-w-64 flex-1" aria-label="Catalog" placeholder="Bind a catalog here" value={pick} onValueChange={setPick} options={(catalogs.data ?? []).filter((c) => !(bound.data ?? []).includes(c.id)).map((c) => ({ value: c.id, label: c.name }))} />
          <Button
            variant="primary"
            disabled={!pick}
            onClick={async () => {
              try {
                await api.bindCatalog(ws, pick);
                setPick("");
                bound.reload();
              } catch (e) {
                toast.error("Could not bind it", (e as Error).message);
              }
            }}
          >
            Bind
          </Button>
        </div>
      )}
      <div className="divide-y divide-border rounded-lg border border-border">
        {(bound.data ?? []).length === 0 && <p className="px-4 py-5 text-center text-xs text-muted">No catalog is bound here, so all of the organization's catalogs show.</p>}
        {(bound.data ?? []).map((id) => (
          <div key={id} className="flex items-center gap-3 px-4 py-2.5">
            <span className="min-w-0 flex-1 truncate text-sm text-foreground">{names[id] ?? id}</span>
            {accountAdmin && (
              <Button variant="ghost" size="sm" onClick={async () => { await api.unbindCatalog(ws, id).catch((e: Error) => toast.error("Could not unbind", e.message)); bound.reload(); }}>
                Unbind
              </Button>
            )}
          </div>
        ))}
      </div>
    </Section>
  );
}

function Danger({ ws, workspace }: { ws: string; workspace: api.Workspace }) {
  const [name, setName] = useState(workspace.name);
  return (
    <SettingGroup title="Workspace" description="Only account admins can rename or delete a workspace. Deleting it does not touch the organization's catalogs.">
      <SettingRow label="Name">
        <Input className="w-56" value={name} onChange={(e) => setName(e.target.value)} aria-label="Workspace name" />
        <Button
          disabled={!name.trim() || name === workspace.name}
          onClick={async () => {
            try {
              await api.renameWorkspace(ws, name);
              toast.success("Renamed");
              location.reload();
            } catch (e) {
              toast.error("Could not rename", (e as Error).message);
            }
          }}
        >
          Rename
        </Button>
      </SettingRow>
      <SettingRow label="Delete this workspace" description="Everyone is unassigned and its settings go. Data stays in the catalogs.">
        <Button
          variant="danger"
          onClick={async () => {
            if (!(await confirmAction({ title: `Delete ${workspace.name}?`, description: "People lose access to this workspace. The catalogs and files are not touched.", confirmLabel: "Delete", danger: true }))) return;
            try {
              await api.deleteWorkspace(ws);
              go("/");
              location.reload();
            } catch (e) {
              toast.error("Could not delete it", (e as Error).message);
            }
          }}
        >
          <Trash2 /> Delete
        </Button>
      </SettingRow>
    </SettingGroup>
  );
}

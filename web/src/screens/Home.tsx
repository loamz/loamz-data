import { Building2, ChevronRight, Layers } from "lucide-react";
import { Badge, Button, Page, PageEmpty, Section, toast } from "@/design";
import * as api from "@/api";
import { openWorkspace } from "@/site";
import { go } from "@/route";

/** Where sign-in lands: who you are, the workspaces you can open, the organizations you belong to. */
export function Home({ me, reload }: { me: api.Me | undefined; reload: () => void }) {
  if (!me) return <Page title="Home">{null}</Page>;
  const first = (me.name || me.email || me.subject).split(/[ @]/)[0];
  return (
    <Page title={`Welcome, ${first}`} subtitle={me.email || `Signed in as ${me.subject}`}>
      <Section title="Your workspaces" description="Where you work with data. Open one to browse catalogs, files and who can reach them.">
        {me.workspaces.length === 0 ? (
          <PageEmpty icon={Layers} title="You are not in a workspace yet">
            Ask an account admin of your organization to add you to one.
          </PageEmpty>
        ) : (
          <div className="-m-1 divide-y divide-border">
            {me.workspaces.map((w) => (
              <button key={w.id} type="button" onClick={() => openWorkspace(w.id)} className="flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-3 text-left transition-colors hover:bg-card-hover">
                <Layers className="shrink-0 text-muted" aria-hidden />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium text-foreground">{w.name}</span>
                  <span className="block truncate text-xs text-muted">{w.org_alias === "Personal" ? "Your own space" : w.org_alias}</span>
                </span>
                <Badge tone={w.admin ? "accent" : "neutral"}>{w.admin ? "Admin" : "User"}</Badge>
                <ChevronRight className="shrink-0 text-muted" aria-hidden />
              </button>
            ))}
          </div>
        )}
      </Section>

      <Section title="Organizations" description="Your own space, and the organizations you belong to. An organization needs a Data account before it has workspaces.">
        <div className="-m-1 divide-y divide-border">
          {me.orgs.map((o) => (
            <div key={o.id} className="flex items-center gap-3 px-3 py-3">
              <Building2 className="shrink-0 text-muted" aria-hidden />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium text-foreground">{o.alias}</span>
                <span className="block text-xs text-muted">{o.kind === "personal" ? "Your own space" : o.has_account ? "Organization" : "No Data account yet"}</span>
              </span>
              {o.can_create_account && (
                <Button
                  variant="primary"
                  onClick={async () => {
                    try {
                      await api.createAccount(o.id);
                      toast.success(`Created the account for ${o.alias}`);
                      reload();
                      go({ screen: "account", org: o.id, tab: "workspaces" });
                    } catch (e) {
                      toast.error("Could not create the account", (e as Error).message);
                    }
                  }}
                >
                  Create account
                </Button>
              )}
              {o.account_admin && (
                <Button variant="secondary" onClick={() => go({ screen: "account", org: o.id, tab: "workspaces" })}>
                  Manage
                </Button>
              )}
            </div>
          ))}
        </div>
      </Section>
    </Page>
  );
}

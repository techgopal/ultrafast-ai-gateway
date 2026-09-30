import { Badge } from "@/components/ui/badge";

/** What the console calls the roles of the gateway. */
export const ROLE_NAMES: Record<string, string> = { admin: "Admin", member: "Member" };
const TEAM_ROLE_NAMES: Record<string, string> = { lead: "Lead", member: "Member" };

/** A role in a neutral badge. One the console does not know is shown as it is. */
export function RoleBadge({ role }: { role: string }) {
  return <Badge variant="outline">{ROLE_NAMES[role] ?? role}</Badge>;
}

/** A role in a team, in a neutral badge. One the console does not know is shown as it is. */
export function TeamRoleBadge({ role }: { role: string }) {
  return <Badge variant="outline">{TEAM_ROLE_NAMES[role] ?? role}</Badge>;
}

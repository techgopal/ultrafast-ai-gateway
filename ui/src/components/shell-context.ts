/** What the shell shows of the signed-in user. The session code fills it. */
export interface ShellUser {
  name: string;
  role: "admin" | "member";
  teams: readonly { name: string }[];
}

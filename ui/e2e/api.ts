// The admin API, from the test: accounts made for one test, and a signed-in
// session of the API that makes what a flow needs.
import { expect, type APIRequestContext } from "@playwright/test";
import { randomBytes } from "node:crypto";
import type { Account } from "./gateway";

/** A new account: an email and a password of 24 characters, both random. */
export function newAccount(kind: string): Account {
  return {
    email: `${kind}-${randomBytes(6).toString("hex")}@example.test`,
    password: randomBytes(18).toString("base64url"),
  };
}


/** A signed-in session of the admin API, for making what a flow needs. */
export class GatewayApi {
  private constructor(
    private readonly request: APIRequestContext,
    private readonly csrf: string,
  ) {}

  static async signIn(request: APIRequestContext, account: Account): Promise<GatewayApi> {
    const answer = await request.post("/api/auth/login", { data: account });
    expect(answer.status(), "the test signs in through the API").toBe(200);
    const body = (await answer.json()) as { csrf_token: string };
    return new GatewayApi(request, body.csrf_token);
  }

  async send(method: "POST" | "PUT" | "PATCH" | "DELETE", path: string, data?: unknown) {
    const answer = await this.request.fetch(path, {
      method,
      headers: { "x-csrf-token": this.csrf },
      ...(data === undefined ? {} : { data }),
    });
    expect(answer.status(), `${method} ${path}`).toBeLessThan(300);
    return answer.status() === 204 ? null : ((await answer.json()) as unknown);
  }

  async get(path: string): Promise<unknown> {
    const answer = await this.request.get(path);
    expect(answer.status(), `GET ${path}`).toBe(200);
    return (await answer.json()) as unknown;
  }

  /** Invites a user and returns their id and the invite link (a path of the console). */
  async invite(name: string, email: string, role: "admin" | "member") {
    const made = (await this.send("POST", "/api/users", { name, email, role })) as {
      user: { id: number };
      invite_link: string;
    };
    return { id: made.user.id, link: made.invite_link };
  }

  /** A user who accepted their invite: they can sign in. */
  async activeUser(name: string, role: "admin" | "member" = "member") {
    const account = newAccount(name.toLowerCase().replace(/[^a-z0-9]+/g, "-"));
    const { id, link } = await this.invite(name, account.email, role);
    const token = new URL(link, "http://console").searchParams.get("token");
    const answer = await this.request.post("/api/auth/accept-invite", {
      data: { token, password: account.password },
    });
    expect(answer.status(), "the invite is accepted").toBe(204);
    return { id, name, ...account };
  }

  async createTeam(name: string): Promise<number> {
    const made = (await this.send("POST", "/api/teams", { name })) as { id: number };
    return made.id;
  }

  async putMember(teamId: number, userId: number, role: "lead" | "member") {
    await this.send("PUT", `/api/teams/${String(teamId)}/members/${String(userId)}`, { role });
  }
}

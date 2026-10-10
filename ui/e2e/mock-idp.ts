// A small OpenID Connect provider for the browser tests: discovery, an
// authorize endpoint that signs the user in at once, a token endpoint that
// returns an RS256 ID token, and the key set. It checks what a real provider
// checks (the client, the redirect address, PKCE) so that a gateway that sends
// the wrong thing fails here.
//
// It listens on 127.0.0.1 and is reached by the name `localhost`: a browser on
// the gateway's own address (127.0.0.1) treats it as another site, so the
// redirect back to the gateway is a cross-site navigation, as with a real
// provider. The issuer is plain http on a loopback host, which the gateway
// allows for exactly that.
import {
  createHash,
  createSign,
  generateKeyPairSync,
  randomBytes,
  type JsonWebKey,
} from "node:crypto";
import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";

export interface IdpUser {
  sub: string;
  email: string;
  email_verified?: boolean;
  name?: string;
  groups?: string[];
}

export interface MockIdp {
  /** For example `http://localhost:41234`. */
  issuer: string;
  /** The origin of the provider, same as the issuer. */
  origin: string;
  clientId: string;
  clientSecret: string;
  /** Who the provider signs in. Change it between sign-ins. */
  user: IdpUser;
  /** The error the next authorize answers with instead of a code; `null`: none. */
  failNextAuthorizeWith: string | null;
  /** The paths asked, in order. */
  requests: string[];
  stop: () => Promise<void>;
}

interface Pending {
  nonce: string;
  challenge: string;
  redirectUri: string;
  clientId: string;
}

const b64url = (data: Buffer | string) => Buffer.from(data).toString("base64url");

function body(request: IncomingMessage): Promise<string> {
  return new Promise((done, fail) => {
    const chunks: Buffer[] = [];
    request.on("data", (chunk: Buffer) => chunks.push(chunk));
    request.on("end", () => {
      done(Buffer.concat(chunks).toString("utf8"));
    });
    request.on("error", fail);
  });
}

function json(response: ServerResponse, status: number, value: unknown): void {
  response.writeHead(status, { "content-type": "application/json", "cache-control": "no-store" });
  response.end(JSON.stringify(value));
}

/** Starts the provider on a port the system gives out. */
export async function startMockIdp(
  user: IdpUser,
  options: { clientId?: string; clientSecret?: string } = {},
): Promise<MockIdp> {
  const { publicKey, privateKey } = generateKeyPairSync("rsa", { modulusLength: 2048 });
  const kid = "mock-idp-key-1";
  const jwk: JsonWebKey = {
    ...publicKey.export({ format: "jwk" }),
    kid,
    use: "sig",
    alg: "RS256",
  };
  const pending = new Map<string, Pending>();
  const idp: MockIdp = {
    issuer: "",
    origin: "",
    clientId: options.clientId ?? "gateway-e2e",
    clientSecret: options.clientSecret ?? randomBytes(18).toString("base64url"),
    user,
    failNextAuthorizeWith: null,
    requests: [],
    stop: () => Promise.resolve(),
  };

  function idToken(entry: Pending): string {
    const now = Math.floor(Date.now() / 1000);
    const claims: Record<string, unknown> = {
      iss: idp.issuer,
      sub: idp.user.sub,
      aud: idp.clientId,
      iat: now,
      exp: now + 300,
      nonce: entry.nonce,
      email: idp.user.email,
      email_verified: idp.user.email_verified ?? true,
      name: idp.user.name ?? idp.user.email,
    };
    if (idp.user.groups !== undefined) claims.groups = idp.user.groups;
    const head = b64url(JSON.stringify({ alg: "RS256", typ: "JWT", kid }));
    const payload = b64url(JSON.stringify(claims));
    const signature = createSign("RSA-SHA256").update(`${head}.${payload}`).sign(privateKey);
    return `${head}.${payload}.${b64url(signature)}`;
  }

  function authorize(url: URL, response: ServerResponse): void {
    const query = url.searchParams;
    const redirectUri = query.get("redirect_uri") ?? "";
    const state = query.get("state") ?? "";
    const challenge = query.get("code_challenge") ?? "";
    const valid =
      query.get("client_id") === idp.clientId &&
      query.get("response_type") === "code" &&
      query.get("code_challenge_method") === "S256" &&
      challenge !== "" &&
      (query.get("scope") ?? "").split(" ").includes("openid") &&
      (query.get("nonce") ?? "") !== "" &&
      state !== "" &&
      redirectUri !== "";
    if (!valid) {
      response.writeHead(400, { "content-type": "text/plain" });
      response.end("invalid authorization request");
      return;
    }
    const back = new URL(redirectUri);
    back.searchParams.set("state", state);
    if (idp.failNextAuthorizeWith !== null) {
      back.searchParams.set("error", idp.failNextAuthorizeWith);
      idp.failNextAuthorizeWith = null;
    } else {
      const code = randomBytes(16).toString("hex");
      pending.set(code, {
        nonce: query.get("nonce") ?? "",
        challenge,
        redirectUri,
        clientId: idp.clientId,
      });
      back.searchParams.set("code", code);
    }
    response.writeHead(302, { location: back.href, "cache-control": "no-store" });
    response.end();
  }

  async function token(request: IncomingMessage, response: ServerResponse): Promise<void> {
    const form = new URLSearchParams(await body(request));
    let id = form.get("client_id");
    let secret = form.get("client_secret");
    const basic = /^Basic (.+)$/.exec(request.headers.authorization ?? "")?.[1];
    if (basic !== undefined) {
      const [name = "", given = ""] = Buffer.from(basic, "base64").toString("utf8").split(":");
      id = decodeURIComponent(name);
      secret = decodeURIComponent(given);
    }
    if (id !== idp.clientId || secret !== idp.clientSecret) {
      json(response, 401, { error: "invalid_client" });
      return;
    }
    const entry = pending.get(form.get("code") ?? "");
    pending.delete(form.get("code") ?? "");
    const verifier = form.get("code_verifier") ?? "";
    const challenge = createHash("sha256").update(verifier).digest("base64url");
    if (
      form.get("grant_type") !== "authorization_code" ||
      entry === undefined ||
      entry.redirectUri !== form.get("redirect_uri") ||
      entry.challenge !== challenge
    ) {
      json(response, 400, { error: "invalid_grant" });
      return;
    }
    json(response, 200, {
      access_token: randomBytes(16).toString("hex"),
      token_type: "Bearer",
      expires_in: 300,
      id_token: idToken(entry),
    });
  }

  const server: Server = createServer((request, response) => {
    const url = new URL(request.url ?? "/", idp.origin);
    idp.requests.push(`${request.method ?? "GET"} ${url.pathname}`);
    if (url.pathname === "/.well-known/openid-configuration") {
      json(response, 200, {
        issuer: idp.issuer,
        authorization_endpoint: `${idp.origin}/authorize`,
        token_endpoint: `${idp.origin}/token`,
        jwks_uri: `${idp.origin}/jwks`,
        response_types_supported: ["code"],
        subject_types_supported: ["public"],
        id_token_signing_alg_values_supported: ["RS256"],
        code_challenge_methods_supported: ["S256"],
        token_endpoint_auth_methods_supported: ["client_secret_basic", "client_secret_post"],
      });
    } else if (url.pathname === "/jwks") {
      json(response, 200, { keys: [jwk] });
    } else if (url.pathname === "/authorize") {
      authorize(url, response);
    } else if (url.pathname === "/token" && request.method === "POST") {
      void token(request, response);
    } else if (url.pathname === "/elsewhere") {
      // A page of another site, from which the browser can be sent to the gateway.
      response.writeHead(200, { "content-type": "text/html; charset=utf-8" });
      response.end("<!doctype html><title>Elsewhere</title><p>Another site.</p>");
    } else {
      response.writeHead(404);
      response.end();
    }
  });
  await new Promise<void>((done, fail) => {
    server.once("error", fail);
    server.listen(0, "127.0.0.1", done);
  });
  const address = server.address();
  const port = typeof address === "object" && address !== null ? address.port : 0;
  idp.origin = `http://localhost:${String(port)}`;
  idp.issuer = idp.origin;
  idp.stop = () =>
    new Promise<void>((done) => {
      server.closeAllConnections();
      server.close(() => {
        done();
      });
    });
  return idp;
}

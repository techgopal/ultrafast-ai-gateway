import { control } from "@/components/classes";

const PYTHON = `import hmac, hashlib
t, v1 = [part.split("=", 1)[1] for part in header.split(",")]
signed = t.encode() + b"." + raw_body
expected = hmac.new(secret.encode(), signed, hashlib.sha256).hexdigest()
if not hmac.compare_digest(expected, v1): reject()
if abs(time.time() - int(t)) > 300: reject()`;

const NODE = `const crypto = require("node:crypto");
const [t, v1] = header.split(",").map((part) => part.split("=")[1]);
const hmac = crypto.createHmac("sha256", secret).update(\`\${t}.\${rawBody}\`);
const expected = hmac.digest("hex");
if (expected.length !== v1.length || !crypto.timingSafeEqual(Buffer.from(expected), Buffer.from(v1))) reject();
if (Math.abs(Date.now() / 1000 - Number(t)) > 300) reject();`;

const block =
  "rounded-md bg-muted p-3 font-mono text-xs wrap-anywhere whitespace-pre-wrap text-foreground";

/** How a receiver checks that a delivery came from the gateway. */
export function VerifySignatures() {
  return (
    <details className="rounded-lg border bg-card px-4 text-card-foreground">
      <summary className={`${control} flex cursor-pointer items-center font-medium`}>
        How to verify signatures
      </summary>
      <div className="flex flex-col gap-3 pb-4 text-sm">
        <p>
          Every delivery carries the header{" "}
          <code className="font-mono wrap-anywhere">x-uf-signature: t=&lt;unix seconds&gt;,v1=&lt;hex&gt;</code>.
          The value after <code className="font-mono">v1=</code> is the HMAC-SHA256, in hex, of{" "}
          <code className="font-mono">&lt;t&gt;.&lt;raw body&gt;</code> with the signing secret of
          the channel as the key. Check it against the raw body, before it is parsed, and refuse a
          time that is too old.
        </p>
        <p className="font-medium">Python</p>
        <pre className={block}>
          <code>{PYTHON}</code>
        </pre>
        <p className="font-medium">Node.js</p>
        <pre className={block}>
          <code>{NODE}</code>
        </pre>
      </div>
    </details>
  );
}

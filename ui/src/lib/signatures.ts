// The examples a receiver can copy to check the signature of a delivery
// (`x-uf-signature: t=<unix seconds>,v1=<hex>`). They are run against the
// gateway's known answer in `signatures.test.ts`.

/** Expects `header`, `raw_body` (bytes), `secret` and `reject()`. */
export const PYTHON_EXAMPLE = `import hmac, hashlib, time
t, v1 = [part.split("=", 1)[1] for part in header.split(",")]
signed = t.encode() + b"." + raw_body
expected = hmac.new(secret.encode(), signed, hashlib.sha256).hexdigest()
if not hmac.compare_digest(expected, v1): reject()
if abs(time.time() - int(t)) > 300: reject()`;

/** Expects `header`, `rawBody` (text), `secret` and `reject()`. */
export const NODE_EXAMPLE = `const crypto = require("node:crypto");
const [t, v1] = header.split(",").map((part) => part.split("=")[1]);
const hmac = crypto.createHmac("sha256", secret).update(\`\${t}.\${rawBody}\`);
const expected = hmac.digest("hex");
if (expected.length !== v1.length || !crypto.timingSafeEqual(Buffer.from(expected), Buffer.from(v1))) reject();
if (Math.abs(Date.now() / 1000 - Number(t)) > 300) reject();`;

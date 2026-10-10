import { control } from "@/components/classes";
import { NODE_EXAMPLE, PYTHON_EXAMPLE } from "@/lib/signatures";

const block =
  "rounded-md bg-muted p-3 font-mono text-xs wrap-anywhere whitespace-pre-wrap text-foreground";

interface VerifyProps {
  /** Whose secret signs: "channel" for alerts. */
  subject?: string;
  /** What is signed and sent: "delivery" for alerts. */
  unit?: string;
}

/** How a receiver checks that a delivery came from the gateway. */
export function VerifySignatures({ subject = "channel", unit = "delivery" }: VerifyProps) {
  return (
    <details className="rounded-lg border bg-card px-4 text-card-foreground">
      <summary className={`${control} flex cursor-pointer items-center font-medium`}>
        How to verify signatures
      </summary>
      <div className="flex flex-col gap-3 pb-4 text-sm">
        <p>
          Every {unit} carries the header{" "}
          <code className="font-mono wrap-anywhere">x-uf-signature: t=&lt;unix seconds&gt;,v1=&lt;hex&gt;</code>.
          The value after <code className="font-mono">v1=</code> is the HMAC-SHA256, in hex, of{" "}
          <code className="font-mono">&lt;t&gt;.&lt;raw body&gt;</code> with the signing secret of
          the {subject} as the key. Check it against the raw body, before it is parsed, and refuse a
          time that is too old.
        </p>
        <p className="font-medium">Python</p>
        <pre className={block}>
          <code>{PYTHON_EXAMPLE}</code>
        </pre>
        <p className="font-medium">Node.js</p>
        <pre className={block}>
          <code>{NODE_EXAMPLE}</code>
        </pre>
      </div>
    </details>
  );
}

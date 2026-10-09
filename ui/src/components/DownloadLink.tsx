import { useRef, useState, type ComponentProps, type ReactNode } from "react";
import { api } from "@/api/client";
import { messageOfError } from "@/api/errors";
import { control } from "@/components/classes";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";

/**
 * A link to a file the gateway sends. The browser makes the download, so a
 * large file is streamed and never held in memory; but a browser shows
 * nothing useful when the session has ended or the gateway fails, and may
 * save the error text in place of the file. So the session is checked first,
 * through the client (an ended session is handled as everywhere else), and a
 * refusal is shown here; the download starts only after the check.
 *
 * A small file whose making can be refused for a reason the user must read
 * (the configuration export) is fetched through the client instead (`fetchFile`):
 * a refusal is shown in place and a file is saved only when there is one.
 */
export function DownloadLink({
  href,
  variant,
  fetchFile,
  filename,
  children,
}: {
  href: string;
  /** Gets the file through the client; its JSON is saved under the name `filename` gives. */
  fetchFile?: () => Promise<unknown>;
  filename?: () => string;
  variant?: ComponentProps<typeof Button>["variant"];
  children: ReactNode;
}) {
  const [problem, setProblem] = useState<string | null>(null);
  const checking = useRef(false);

  async function start(event: { preventDefault: () => void }) {
    event.preventDefault();
    if (checking.current) return;
    checking.current = true;
    setProblem(null);
    let content: Blob | null = null;
    try {
      if (fetchFile === undefined) {
        await api.get("/api/settings");
      } else {
        const file = await fetchFile();
        content = new Blob([JSON.stringify(file, null, 2)], { type: "application/json" });
      }
    } catch (error) {
      setProblem(messageOfError(error));
      return;
    } finally {
      checking.current = false;
    }
    const link = document.createElement("a");
    if (content === null) {
      link.href = href;
      link.download = "";
      link.click();
      return;
    }
    const url = URL.createObjectURL(content);
    link.href = url;
    link.download = filename?.() ?? "";
    link.click();
    URL.revokeObjectURL(url);
  }

  return (
    <div className="flex flex-col gap-3">
      <div>
        <Button asChild {...(variant === undefined ? {} : { variant })} className={control}>
          <a href={href} download onClick={(event) => void start(event)}>
            {children}
          </a>
        </Button>
      </div>
      {problem === null ? null : (
        <Alert variant="destructive">
          <AlertDescription>
            <p>{problem}</p>
          </AlertDescription>
        </Alert>
      )}
    </div>
  );
}

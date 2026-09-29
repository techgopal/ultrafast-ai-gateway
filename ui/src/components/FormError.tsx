import type { ReactNode, Ref } from "react";
import { Alert, AlertDescription } from "@/components/ui/alert";

interface FormErrorProps {
  ref?: Ref<HTMLDivElement>;
  /** What `useFormFailure` gives: one line per message. */
  messages?: readonly string[];
  children?: ReactNode;
}

/**
 * Why the request failed, at the top of its form. It is announced when it
 * appears (`role="alert"`), and it can take the focus. Without a message it
 * renders nothing.
 */
export function FormError({ ref, messages = [], children }: FormErrorProps) {
  const nothing = children === undefined || children === null || children === false;
  if (messages.length === 0 && nothing) return null;
  return (
    <Alert
      ref={ref}
      tabIndex={-1}
      variant="destructive"
      className="outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <AlertDescription>
        {messages.map((message) => (
          <p key={message}>{message}</p>
        ))}
        {children}
      </AlertDescription>
    </Alert>
  );
}

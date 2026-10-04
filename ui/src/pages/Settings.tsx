import { Link, useRouterState } from "@tanstack/react-router";
import { useForm } from "@tanstack/react-form";
import { useId, useRef } from "react";
import { useSettings, useUpdateSettings } from "@/api/queries";
import { ConsoleRefusal } from "@/api/errors";
import { can } from "@/auth/guards";
import { useSession } from "@/auth/session";
import { control } from "@/components/classes";
import { ErrorState } from "@/components/ErrorState";
import { Field } from "@/components/Field";
import { applyApiError, useFormFailure, useSubmit } from "@/components/form";
import { FormError } from "@/components/FormError";
import { NotAvailableContent } from "@/components/NotAvailableContent";
import { PageHeader } from "@/components/PageHeader";
import { useToast } from "@/components/toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { BackupSection } from "@/pages/SettingsBackup";
import { ConfigSection } from "@/pages/SettingsConfig";
import { AuditSection } from "@/pages/SettingsAudit";
import { SignInSection } from "@/pages/SettingsSignIn";

export const DONE = "Settings saved.";
export const RETENTION_RULE = "Enter a whole number from 1 to 3650.";
export const RETENTION_HINT = "From 1 to 3650 days. Older request logs are deleted.";

function RetentionForm({ days }: { days: number }) {
  const update = useUpdateSettings();
  const toast = useToast();
  const { mutateAsync, reset } = update;
  const form = useForm({
    defaultValues: { log_retention_days: String(days) },
    onSubmit: async ({ value }) => {
      try {
        const typed = value.log_retention_days.trim();
        const count = /^\d+$/.test(typed) ? Number(typed) : 0;
        if (count < 1 || count > 3650) {
          throw new ConsoleRefusal(RETENTION_RULE, "log_retention_days");
        }
        await mutateAsync({ log_retention_days: count });
        reset();
        toast(DONE);
      } catch (error) {
        reset();
        applyApiError(form, error);
      }
    },
  });
  const formRef = useRef<HTMLFormElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  const failure = useFormFailure(form, formRef, errorRef);
  const onSubmit = useSubmit(form);

  return (
    <form
      ref={formRef}
      aria-label="Retention"
      noValidate
      className="flex max-w-md flex-col gap-4"
      onSubmit={onSubmit}
    >
      <FormError ref={errorRef} messages={failure.messages} />
      <form.Field name="log_retention_days">
        {(field) => (
          <Field
            label="Keep request logs for (days)"
            name={field.name}
            hint={RETENTION_HINT}
            error={failure.fieldError(field.name)}
          >
            {({ id, name, ...described }) => (
              <Input
                {...described}
                id={id}
                name={name}
                inputMode="numeric"
                autoComplete="off"
                className={control}
                value={field.state.value}
                onBlur={field.handleBlur}
                onChange={(event) => {
                  field.handleChange(event.target.value);
                }}
              />
            )}
          </Field>
        )}
      </form.Field>
      <div>
        <Button type="submit" className={control} disabled={update.isPending}>
          {update.isPending ? "Saving" : "Save retention"}
        </Button>
      </div>
    </form>
  );
}

function RetentionSection({ days }: { days: number }) {
  const headingId = useId();
  return (
    <section aria-labelledby={headingId} className="flex flex-col gap-4">
      <h2 id={headingId} className="text-lg font-medium">
        Retention
      </h2>
      <RetentionForm days={days} />
    </section>
  );
}

/** The two views of the page: the settings, and the audit log. */
function SectionNav({ view }: { view: "general" | "audit" }) {
  const link = "inline-flex items-center rounded-md px-3 text-sm font-medium aria-[current=page]:bg-muted aria-[current=page]:text-foreground text-muted-foreground hover:text-foreground " + control;
  return (
    <nav aria-label="Settings sections" className="flex flex-wrap gap-1">
      <Link
        to="/settings"
        className={link}
        {...(view === "general" ? { "aria-current": "page" as const } : {})}
      >
        General
      </Link>
      <Link
        to="/settings"
        hash="audit"
        className={link}
        {...(view === "audit" ? { "aria-current": "page" as const } : {})}
      >
        Audit log
      </Link>
    </nav>
  );
}

function General() {
  const settings = useSettings();
  return (
    <div className="flex flex-col gap-8">
      {settings.data !== undefined ? (
        <>
          <RetentionSection days={settings.data.log_retention_days} />
          <SignInSection settings={settings.data} />
        </>
      ) : settings.error !== null ? (
        <ErrorState
          error={settings.error}
          onRetry={() => {
            void settings.refetch();
          }}
        />
      ) : (
        <div
          role="status"
          aria-busy="true"
          aria-label="Loading the settings"
          className="flex max-w-md flex-col gap-4"
        >
          <Skeleton className="h-4 w-48" />
          <Skeleton className="h-8 w-full" />
        </div>
      )}
      <BackupSection />
      <ConfigSection />
    </div>
  );
}

function SettingsOf() {
  const hash = useRouterState({ select: (state) => state.location.hash });
  const view = hash === "audit" ? "audit" : "general";
  return (
    <>
      <PageHeader title="Settings" />
      <SectionNav view={view} />
      {view === "audit" ? <AuditSection /> : <General />}
    </>
  );
}

/** The settings of the gateway, and its audit log: only an admin reads and changes them. */
export function Settings() {
  const session = useSession();
  if (session.status !== "signedIn") return null;
  if (!can(session.me, { type: "manageSettings" })) return <NotAvailableContent />;
  return <SettingsOf />;
}

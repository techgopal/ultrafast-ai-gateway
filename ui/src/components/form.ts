// What the forms of the console share: where the errors of the API go, and
// where the focus goes after a submit that failed.
import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type RefObject,
} from "react";
import { ApiError, ConsoleRefusal } from "@/api/errors";
import { messageOfError } from "@/components/ErrorState";

/** What this file needs of a form of TanStack Form. Every `useForm` gives it. */
export interface FormLike {
  readonly state: FormSnapshot;
  readonly store: {
    subscribe(listener: () => void): { unsubscribe: () => void };
  };
}

interface FormSnapshot {
  /** The values, by the name of the field. */
  readonly values: unknown;
  readonly submissionAttempts: number;
}

/** What an attempt to submit failed with. */
interface Failure {
  /** Counts the failures of the form, so that each one moves the focus. */
  readonly count: number;
  /** The attempt that failed. The next attempt starts without its errors. */
  readonly attempt: number;
  /**
   * The message of each field, with the value the field had. A field that
   * was changed since is not in it any more.
   */
  readonly fields: ReadonlyMap<string, { message: string; value: unknown }>;
  /** What no field stands for. */
  readonly messages: readonly string[];
}

const NONE: Failure = { count: 0, attempt: -1, fields: new Map(), messages: [] };
const NO_MESSAGES: readonly string[] = [];

class FailureStore {
  private failure = NONE;
  private readonly listeners = new Set<() => void>();
  /** Ends the watch of the form that the failure before started. */
  private unwatch: () => void = () => undefined;

  get = (): Failure => this.failure;

  /**
   * A new failure. `form` is watched from now on: the error of a field is
   * removed for good when the field gets another value.
   */
  set(failure: Omit<Failure, "count">, form: FormLike): void {
    this.unwatch();
    this.unwatch = () => undefined;
    this.failure = { ...failure, count: this.failure.count + 1 };
    if (failure.fields.size > 0) {
      const subscription = form.store.subscribe(() => {
        this.forgetChanged(form);
      });
      this.unwatch = () => {
        subscription.unsubscribe();
      };
    }
    this.tell();
  }

  private forgetChanged(form: FormLike): void {
    const now = valuesOf(form);
    const kept = new Map(
      [...this.failure.fields].filter(([name, field]) => Object.is(now.get(name), field.value)),
    );
    if (kept.size === this.failure.fields.size) return;
    // The same count: a field that lost its error moves no focus.
    this.failure = { ...this.failure, fields: kept };
    if (kept.size === 0) {
      this.unwatch();
      this.unwatch = () => undefined;
    }
    this.tell();
  }

  private tell(): void {
    for (const listener of [...this.listeners]) listener();
  }

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
}

// The failure of a form is kept beside the form, not in it: an error of the
// API is not a rule of the form, and must not keep it from being sent again.
const stores = new WeakMap<FormLike, FailureStore>();

function storeOf(form: FormLike): FailureStore {
  let store = stores.get(form);
  if (store === undefined) {
    store = new FailureStore();
    stores.set(form, store);
  }
  return store;
}

function valuesOf(form: FormLike): ReadonlyMap<string, unknown> {
  const { values } = form.state;
  return new Map(typeof values === "object" && values !== null ? Object.entries(values) : []);
}

/** The message of a field the form does not have says which field it is about. */
function named(field: string, message: string): string {
  return message.startsWith(field) ? message : `${field}: ${message}`;
}

/** What the error says about single fields: the name of each, and the text. */
function aboutFields(error: unknown): [name: string, text: string][] {
  if (error instanceof ApiError) return Object.entries(error.fields);
  if (error instanceof ConsoleRefusal && error.field !== undefined) {
    return [[error.field, error.message]];
  }
  return [];
}

/**
 * Puts what a submit failed with onto the form: each key of `error.fields`
 * that is a field of the form becomes the error of that field; the other
 * keys, and the message of the error when no field has one, are shown by
 * `FormError`. Then the focus goes to the first field that got an error, or
 * to the error of the form. The values of the form are not touched.
 *
 * The form shows this through `useFormFailure`, and through nothing else:
 * the errors of the API are kept beside the form, not in it, so a page must
 * never read them from `field.state.meta.errors`, where they are not. (There
 * they would keep the form from being sent again.)
 *
 * Only top-level field names are matched: a key of `error.fields` is a field
 * of the form when it is a key of `form.state.values`. A key such as
 * `members.0.email` matches no field and is shown by `FormError`.
 *
 * The error of a field goes for good when the field is changed, also when the
 * old value is typed again; all errors go when the next submit starts.
 *
 * What the console refuses itself (`ConsoleRefusal`) goes the same way: with
 * a `field` it is the error of that field, without one the error of the form.
 *
 * An answer of a session that is over (`SessionOverError`) puts nothing on
 * the form and moves no focus: it says nothing to who is signed in now.
 */
export function applyApiError(form: FormLike, error: unknown): void {
  const message = messageOfError(error);
  if (message === null) return;
  const values = valuesOf(form);
  const fields = new Map<string, { message: string; value: unknown }>();
  const messages: string[] = [];
  for (const [name, text] of aboutFields(error)) {
    if (values.has(name)) fields.set(name, { message: text, value: values.get(name) });
    else messages.push(named(name, text));
  }
  if (fields.size === 0 && messages.length === 0) messages.push(message);
  storeOf(form).set({ attempt: form.state.submissionAttempts, fields, messages }, form);
}

/**
 * The error, said by a field of the form: a refusal such as `user_exists` is
 * about what one field holds, and the gateway sends it without `fields`. An
 * error of another code is returned as it is.
 *
 * `text` is what the field says, where the message of the gateway does not
 * say what the form knows: "Not found." is, in a form that asks for the id
 * of a user, "No user with that ID.". Without it the field says the message
 * of the gateway. Either way the error stays what the gateway answered, with
 * its status, its code and its message; only a field is named.
 */
export function onField(error: unknown, code: string, field: string, text?: string): unknown {
  if (!(error instanceof ApiError) || error.code !== code) return error;
  return new ApiError(error.status, error.code, error.message, {
    [field]: text ?? error.message,
  });
}

function focusOnFailure(form: HTMLFormElement | null, message: HTMLElement | null): void {
  const field = form?.querySelector<HTMLElement>('[aria-invalid="true"]');
  (field ?? message)?.focus();
}

/**
 * After a submit that failed, the focus goes to the first field with an error,
 * or to the message of the form when no field has one. Returns what the form
 * calls when a submit failed, after it set its errors.
 *
 * A form of TanStack Form uses `useFormFailure`, which does this itself.
 */
export function useFocusOnFailure(
  form: RefObject<HTMLFormElement | null>,
  message: RefObject<HTMLElement | null>,
): () => void {
  const [failures, setFailures] = useState(0);
  useEffect(() => {
    if (failures === 0) return;
    focusOnFailure(form.current, message.current);
  }, [failures, form, message]);
  return useCallback(() => {
    setFailures((count) => count + 1);
  }, []);
}

export interface FormFailure {
  /** For the `messages` of the `FormError`. */
  messages: readonly string[];
  /** For the `error` of the `Field` of this name. */
  fieldError: (name: string) => string | undefined;
}

/**
 * What `applyApiError` put onto the form, for the form to show. `formRef` is
 * the ref of the `<form>`, `errorRef` the one of the `FormError` at its top:
 * they are where the focus goes after a failure.
 *
 * It is the only way to the errors of the API: a page gives `messages` to
 * its `FormError` and `fieldError(name)` to each `Field`, and never reads
 * `field.state.meta.errors` for them, which does not hold them. `name` is a
 * top-level field name, a key of the values of the form; names of nested
 * fields are not matched.
 */
export function useFormFailure(
  form: FormLike,
  formRef: RefObject<HTMLFormElement | null>,
  errorRef: RefObject<HTMLElement | null>,
): FormFailure {
  const store = storeOf(form);
  const failure = useSyncExternalStore(store.subscribe, store.get);
  const subscribe = useCallback(
    (listener: () => void) => {
      const subscription = form.store.subscribe(listener);
      return () => {
        subscription.unsubscribe();
      };
    },
    [form],
  );
  const state = useSyncExternalStore(subscribe, () => form.state);

  useEffect(() => {
    if (failure.count === 0) return;
    focusOnFailure(formRef.current, errorRef.current);
  }, [failure.count, formRef, errorRef]);

  return useMemo(() => {
    const current = state.submissionAttempts === failure.attempt;
    return {
      messages: current ? failure.messages : NO_MESSAGES,
      // A field that was changed since the failure is not among them any more.
      fieldError: (name) => (current ? failure.fields.get(name)?.message : undefined),
    };
  }, [failure, state]);
}

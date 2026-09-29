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
import { ApiError } from "@/api/errors";
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
  /** The message of each field, with the value the field had. */
  readonly fields: ReadonlyMap<string, { message: string; value: unknown }>;
  /** What no field stands for. */
  readonly messages: readonly string[];
}

const NONE: Failure = { count: 0, attempt: -1, fields: new Map(), messages: [] };
const NO_MESSAGES: readonly string[] = [];

class FailureStore {
  private failure = NONE;
  private readonly listeners = new Set<() => void>();

  get = (): Failure => this.failure;

  set(failure: Omit<Failure, "count">): void {
    this.failure = { ...failure, count: this.failure.count + 1 };
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

/**
 * Puts what a submit failed with onto the form: each key of `error.fields`
 * that is a field of the form becomes the error of that field; the other
 * keys, and the message of the error when no field has one, are shown by
 * `FormError`. Then the focus goes to the first field that got an error, or
 * to the error of the form. The values of the form are not touched.
 *
 * The form shows this through `useFormFailure(form)`. The error of a field
 * goes when the field is changed, and all of them go when the next submit
 * starts.
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
  if (error instanceof ApiError) {
    for (const [name, text] of Object.entries(error.fields)) {
      if (values.has(name)) fields.set(name, { message: text, value: values.get(name) });
      else messages.push(named(name, text));
    }
  }
  if (fields.size === 0 && messages.length === 0) messages.push(message);
  storeOf(form).set({ attempt: form.state.submissionAttempts, fields, messages });
}

/**
 * The error, said by a field of the form: a refusal such as `user_exists` is
 * about what one field holds, and the gateway sends it without `fields`. An
 * error of another code is returned as it is.
 */
export function onField(error: unknown, code: string, field: string): unknown {
  if (!(error instanceof ApiError) || error.code !== code) return error;
  return new ApiError(error.status, error.code, error.message, { [field]: error.message });
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
    const { values } = state;
    const now = new Map(typeof values === "object" && values !== null ? Object.entries(values) : []);
    return {
      messages: current ? failure.messages : NO_MESSAGES,
      fieldError: (name) => {
        const field = failure.fields.get(name);
        if (!current || field === undefined) return undefined;
        // The message was about the value the field had then.
        return Object.is(now.get(name), field.value) ? field.message : undefined;
      },
    };
  }, [failure, state]);
}

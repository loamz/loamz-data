import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError } from "./api";

export interface Loaded<T> {
  data?: T;
  error?: ApiError;
  loading: boolean;
  reload: () => void;
}

/** Runs `fn` when `key` changes and keeps the latest answer; an answer for an older key is dropped. */
export function useLoad<T>(fn: () => Promise<T>, key: string): Loaded<T> {
  const [state, setState] = useState<{ data?: T; error?: ApiError; loading: boolean }>({ loading: true });
  const [tick, setTick] = useState(0);
  const latest = useRef(0);
  const run = useRef(fn);
  run.current = fn;
  useEffect(() => {
    const mine = ++latest.current;
    setState((s) => ({ data: s.data, loading: true }));
    run.current().then(
      (data) => mine === latest.current && setState({ data, loading: false }),
      (e: unknown) => mine === latest.current && setState({ error: e instanceof ApiError ? e : new ApiError(0, "Could not reach the server."), loading: false }),
    );
  }, [key, tick]);
  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { ...state, reload };
}

export function useHash(): string {
  const [hash, setHash] = useState(location.hash);
  useEffect(() => {
    const on = () => setHash(location.hash);
    addEventListener("hashchange", on);
    return () => removeEventListener("hashchange", on);
  }, []);
  return hash;
}

/** Whether the screen is at least `px` wide (the list and the detail sit side by side from 768). */
export function useWide(px = 768): boolean {
  const q = `(min-width: ${px}px)`;
  const [wide, setWide] = useState(() => matchMedia(q).matches);
  useEffect(() => {
    const m = matchMedia(q);
    const on = () => setWide(m.matches);
    m.addEventListener("change", on);
    return () => m.removeEventListener("change", on);
  }, [q]);
  return wide;
}

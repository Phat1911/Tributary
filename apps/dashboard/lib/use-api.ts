// Tiny data-fetching hook (no external state lib — workflow step 10
// discipline). `path === null` pauses fetching; callers drive refetches by
// bumping `reload()`. State updates happen only from async callbacks, and
// `loading` is DERIVED from whether the cached state matches the current
// request version — no setState-in-effect cascades.
"use client";

import { useCallback, useEffect, useState } from "react";
import { ApiError, api } from "@/lib/api";

export interface ApiState<T> {
  data: T | null;
  error: ApiError | null;
  loading: boolean;
  reload: () => void;
}

/** One settled fetch result, tagged with the request it answers. */
interface FetchState<T> {
  version: string;
  data: T | null;
  error: ApiError | null;
}

export function useApi<T>(path: string | null): ApiState<T> {
  const [tick, setTick] = useState(0);
  const [state, setState] = useState<FetchState<T> | null>(null);

  const version = `${path ?? "-"}#${tick}`;
  const reload = useCallback(() => setTick((t) => t + 1), []);

  useEffect(() => {
    if (path === null) return;
    let cancelled = false;
    const current = version;
    api
      .get<T>(path)
      .then((data) => {
        if (!cancelled) setState({ version: current, data, error: null });
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setState({
            version: current,
            data: null,
            error:
              err instanceof ApiError
                ? err
                : new ApiError(0, "network", String(err ?? "network error")),
          });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [path, version]);

  const settled = state?.version === version;
  return {
    data: settled ? state.data : null,
    error: settled ? state.error : null,
    // In flight iff there is a path and no settled state for this version.
    loading: path !== null && !settled,
    reload,
  };
}

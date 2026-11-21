import { useEffect, useRef } from "react";

/** Run `fn` every `ms` while `on`; the latest `fn` is always used and nothing overlaps. */
export function usePoll(fn: () => Promise<void>, ms: number, on: boolean) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    if (!on) return;
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    const tick = async () => { await ref.current().catch(() => {}); if (live) timer = setTimeout(tick, ms); };
    tick();
    return () => { live = false; clearTimeout(timer); };
  }, [ms, on]);
}

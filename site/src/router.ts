// Two routes need no router library: the History API and one event.
import { useEffect, useState } from "react";
import { flushSync } from "react-dom";

const EVENT = "kea:navigate";

export function navigate(to: string) {
  if (to === location.pathname) return;
  const swap = () => {
    history.pushState(null, "", to);
    // The new page renders inside the callback, so the transition captures it.
    flushSync(() => window.dispatchEvent(new Event(EVENT)));
    window.scrollTo({ top: 0 });
  };
  if (!document.startViewTransition || window.matchMedia("(prefers-reduced-motion: reduce)").matches) swap();
  else document.startViewTransition(swap);
}

export function usePath(): string {
  const [path, setPath] = useState(location.pathname);
  useEffect(() => {
    const update = () => setPath(location.pathname);
    window.addEventListener("popstate", update);
    window.addEventListener(EVENT, update);
    return () => {
      window.removeEventListener("popstate", update);
      window.removeEventListener(EVENT, update);
    };
  }, []);
  return path;
}

/** A same-site link that navigates without a reload; modified clicks keep the browser's behaviour. */
export function follow(event: React.MouseEvent<HTMLAnchorElement>) {
  if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
    return;
  }
  event.preventDefault();
  navigate(event.currentTarget.getAttribute("href") ?? "/");
}

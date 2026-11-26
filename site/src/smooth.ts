// Smooth, inertial scrolling for the launch pages. Native scrolling stays for touch and for anyone
// who asks the system for less motion.
import Lenis from "lenis";
import "lenis/dist/lenis.css";
import { useEffect } from "react";

let lenis: Lenis | null = null;

export function useSmoothScroll() {
  useEffect(() => {
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    lenis = new Lenis({ lerp: 0.09, wheelMultiplier: 0.95, smoothWheel: true });
    let frame = 0;
    const loop = (time: number) => {
      lenis?.raf(time);
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(frame);
      lenis?.destroy();
      lenis = null;
    };
  }, []);
}

/** Scrolls to a section, gliding when smooth scrolling is on. */
export function scrollToId(id: string) {
  const element = document.getElementById(id);
  if (!element) return;
  if (!lenis) {
    element.scrollIntoView(); // no smooth scroller means the reader asked for less motion
    return;
  }
  // The scroller keeps its own idea of where the page is. A native scroll it has not caught yet (focus pulling a
  // link into view, say) would make it start and aim from there, so it catches up first and aims from the page.
  lenis.scrollTo(window.scrollY, { immediate: true, force: true });
  lenis.scrollTo(element.getBoundingClientRect().top + window.scrollY - 12, { duration: 1.4 });
}

/** Marks `[data-reveal]` elements as they enter the window, once, so CSS can bring them in. */
export function useReveal(deps: unknown[] = []) {
  useEffect(() => {
    const elements = document.querySelectorAll("[data-reveal]:not(.is-in)");
    const observer = new IntersectionObserver(
      (entries) =>
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-in");
            observer.unobserve(entry.target);
          }
        }),
      { rootMargin: "0px 0px -4% 0px", threshold: 0 },
    );
    elements.forEach((element) => observer.observe(element));
    return () => observer.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}

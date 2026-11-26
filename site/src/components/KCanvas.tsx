import { useEffect, useRef, useState } from "react";
import type { KScene, KTarget } from "../three/KScene";

/** A fixed, transparent canvas behind the page that draws the porcelain mark where the page asks.
 * Three.js loads on its own, after the words: the page reads before the mark arrives. */
export function KCanvas({ target, onFail }: { target: () => KTarget; onFail?: () => void }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [failed, setFailed] = useState(false);
  const want = useRef(target);
  want.current = target;
  useEffect(() => {
    let scene: KScene | undefined;
    let cancelled = false;
    import("../three/KScene")
      .then(({ KScene }) => {
        if (cancelled || !canvas.current) return;
        scene = new KScene(canvas.current, {
          reducedMotion: window.matchMedia("(prefers-reduced-motion: reduce)").matches,
          target: () => want.current(),
        });
      })
      .catch(() => {
        setFailed(true);
        onFail?.();
      });
    return () => {
      cancelled = true;
      scene?.dispose();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return failed ? null : <canvas ref={canvas} className="kx-canvas" aria-hidden="true" />;
}

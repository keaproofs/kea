import { useEffect, useState } from "react";
import { scrollToId } from "../smooth";

/** The right-edge index of the page: one dot per section, the current one lit. */
export function SectionDots({ sections }: { sections: { id: string; label: string }[] }) {
  const [current, setCurrent] = useState(sections[0]?.id);
  useEffect(() => {
    const update = () => {
      const middle = window.innerHeight * 0.45;
      let active = sections[0]?.id;
      for (const { id } of sections) {
        const element = document.getElementById(id);
        if (element && element.getBoundingClientRect().top <= middle) active = id;
      }
      setCurrent(active);
    };
    update();
    window.addEventListener("scroll", update, { passive: true });
    return () => window.removeEventListener("scroll", update);
  }, [sections]);
  return (
    <nav className="kx-dots" aria-label="Page sections">
      {sections.map(({ id, label }) => (
        <button
          key={id}
          type="button"
          className="kx-dots-item"
          aria-label={label}
          aria-current={current === id ? "true" : undefined}
          onClick={() => scrollToId(id)}
        >
          <span />
        </button>
      ))}
    </nav>
  );
}

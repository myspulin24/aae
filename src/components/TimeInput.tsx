import { useState } from "react";

import { hhmm, parseHhmm } from "../format";

/**
 * Čas dne ve 24hodinovém tvaru HH:MM. Vestavěné pole `type="time"` se ve
 * WebView2 řídí jazykem systému a umí ukázat 12hodinový čas.
 * Šipky nahoru a dolů posouvají o 5 minut.
 */
export function TimeInput({
  value,
  onChange,
  label,
}: {
  value: number;
  onChange: (minute: number) => void;
  label?: string;
}) {
  const [text, setText] = useState<string | null>(null);

  function commit() {
    if (text === null) return;
    const m = parseHhmm(text);
    setText(null);
    if (m !== null && m !== value) onChange(m);
  }

  return (
    <input
      className={`time ${text !== null && parseHhmm(text) === null ? "invalid" : ""}`}
      aria-label={label}
      inputMode="numeric"
      maxLength={5}
      value={text ?? hhmm(value)}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") commit();
        if (e.key === "Escape") setText(null);
        if (e.key === "ArrowUp" || e.key === "ArrowDown") {
          e.preventDefault();
          setText(null);
          onChange((value + (e.key === "ArrowUp" ? 5 : -5) + 1440) % 1440);
        }
      }}
    />
  );
}

// Plynulé herní hodiny: mezi stavy od hostitele (10× za s) dopočítá čas
// podle rychlosti, aby se letadla na mapě hýbala plynule.
import { useEffect, useState } from "react";

import { getState } from "../store";

/** Kroků (herních minut) za sekundu při rychlosti 1×. */
export const STEPS_PER_SECOND = 16;

export function gameMinuteNow(): number {
  const { game, gameAt } = getState();
  if (!game) return 0;
  const c = game.clock;
  if (c.paused || c.finished) return c.now;
  // Nanejvýš 2 s dopředu: když stav nechodí, hodiny se zastaví.
  const elapsed = Math.min(2, Math.max(0, (performance.now() - gameAt) / 1000));
  return c.now + elapsed * STEPS_PER_SECOND * c.speed;
}

/** Herní minuta obnovovaná zhruba 30× za sekundu. */
export function useGameMinute(fps = 30): number {
  const [minute, setMinute] = useState(gameMinuteNow);
  useEffect(() => {
    let raf = 0;
    let last = 0;
    const frame = (t: number) => {
      if (t - last >= 1000 / fps) {
        last = t;
        setMinute(gameMinuteNow());
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [fps]);
  return minute;
}

import { useEffect, useState, type ReactNode } from "react";

/** Tlačítko na dvě kliknutí: první se zeptá, druhé provede akci. */
export function ConfirmButton({
  children,
  confirm,
  onConfirm,
  className = "",
  disabled = false,
  title,
}: {
  children: ReactNode;
  confirm: string;
  onConfirm: () => void;
  className?: string;
  disabled?: boolean;
  title?: string;
}) {
  const [armed, setArmed] = useState(false);
  useEffect(() => {
    if (!armed) return;
    const t = window.setTimeout(() => setArmed(false), 3000);
    return () => window.clearTimeout(t);
  }, [armed]);
  return (
    <button
      className={`${className} ${armed ? "danger" : ""}`}
      disabled={disabled}
      title={title}
      onClick={() => {
        if (armed) {
          setArmed(false);
          onConfirm();
        } else {
          setArmed(true);
        }
      }}
    >
      {armed ? confirm : children}
    </button>
  );
}

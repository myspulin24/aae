import { api } from "./api/client";
import { Game } from "./game/Game";
import { Lobby } from "./screens/Lobby";
import { Menu } from "./screens/Menu";
import { dismiss, run, useApp } from "./store";

export function App() {
  const phase = useApp((s) => s.status.phase);
  const hasGame = useApp((s) => s.game !== null);
  const info = useApp((s) => s.info);

  let screen;
  if (!info) screen = <Splash text="Načítám…" />;
  else if (phase === "menu") screen = <Menu />;
  else if (phase === "connecting") screen = <Connecting />;
  else if (phase === "lobby") screen = <Lobby />;
  else if (phase === "waiting") screen = <WaitingRoom />;
  else if (phase === "closed") screen = <Closed />;
  else if (hasGame) screen = <Game />;
  else screen = <Connecting />;

  return (
    <>
      {screen}
      <Toasts />
    </>
  );
}

function Splash({ text }: { text: string }) {
  return (
    <div className="screen center">
      <div className="logo">AAE</div>
      <p className="muted">{text}</p>
    </div>
  );
}

function Connecting() {
  const host = useApp((s) => s.status.host);
  const phase = useApp((s) => s.status.phase);
  const message = useApp((s) => s.status.message);
  return (
    <div className="screen center">
      <div className="logo">AAE</div>
      <div className="spinner" />
      <p>
        {phase === "reconnecting"
          ? message
          : host
            ? "Otevírám hru na internetu… (až 10 s)"
            : "Připojuji se k hostiteli…"}
      </p>
      <button onClick={() => void run(api.leave())}>Zrušit</button>
    </div>
  );
}

function WaitingRoom() {
  return (
    <div className="screen center">
      <div className="logo">AAE</div>
      <div className="spinner" />
      <h2>Čekáte na hostitele</h2>
      <p className="muted narrow">
        Kampaň už běží a hostitel vás ještě nemá v seznamu hráčů. Až vás přiřadí k aerolince, hra se
        sama otevře.
      </p>
      <button onClick={() => void run(api.leave())}>Zpět do menu</button>
    </div>
  );
}

function Closed() {
  const message = useApp((s) => s.status.message);
  return (
    <div className="screen center">
      <div className="logo">AAE</div>
      <h2>Spojení skončilo</h2>
      <p className="narrow">{message}</p>
      <button className="primary" onClick={() => void run(api.leave())}>
        Zpět do menu
      </button>
    </div>
  );
}

function Toasts() {
  const toasts = useApp((s) => s.toasts);
  return (
    <div className="toasts" role="status">
      {toasts.map((t) => (
        <button key={t.id} className={`toast ${t.tone}`} onClick={() => dismiss(t.id)}>
          {t.text}
        </button>
      ))}
    </div>
  );
}

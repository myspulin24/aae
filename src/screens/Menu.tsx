import { useEffect, useState } from "react";

import { api, errorText } from "../api/client";
import type { CampaignInfoDto } from "../api/bindings/CampaignInfoDto";
import type { UpdateDto } from "../api/bindings/UpdateDto";
import { ago } from "../format";
import { run, toast, useApp } from "../store";

export function Menu() {
  const info = useApp((s) => s.info);
  const [name, setName] = useState(info?.name ?? "");
  const [savedName, setSavedName] = useState(info?.name ?? "");
  const [invitation, setInvitation] = useState("");
  const [campaigns, setCampaigns] = useState<CampaignInfoDto[]>([]);
  const [update, setUpdate] = useState<UpdateDto | null>(null);
  const [updating, setUpdating] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void api.campaigns().then(setCampaigns, () => setCampaigns([]));
    // Aktualizace se hledají jen tady v menu, nikdy během hry.
    void api.updateCheck().then(setUpdate, () => setUpdate(null));
  }, []);

  const nameReady = savedName.trim().length > 0;

  async function saveName() {
    const n = name.trim();
    if (!n || n === savedName) return;
    if ((await run(api.setName(n))) !== undefined) setSavedName(n);
  }

  // Jméno se musí uložit dřív, než jádro začne relaci.
  async function go(action: () => Promise<void>) {
    await saveName();
    setBusy(true);
    try {
      await action();
    } catch (e) {
      toast(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  }

  async function install() {
    setUpdating(true);
    try {
      await api.updateInstall();
    } catch (e) {
      toast(errorText(e), "error");
      setUpdating(false);
    }
  }

  return (
    <div className="screen menu">
      <header className="menu-head">
        <div className="logo big">AAE</div>
        <div>
          <h1>American Airlines Express</h1>
          <p className="muted">Letecká manažerská hra pro kamarády · verze {info?.version}</p>
        </div>
      </header>

      {update && (
        <div className="banner info">
          <span>
            Je k dispozici nová verze <strong>{update.version}</strong>.
            {update.notes ? ` ${update.notes}` : ""}
          </span>
          <button className="primary" disabled={updating} onClick={() => void install()}>
            {updating ? "Stahuji…" : "Aktualizovat a restartovat"}
          </button>
        </div>
      )}

      <section className="card">
        <h2>Vaše jméno</h2>
        <div className="row">
          <input
            value={name}
            maxLength={24}
            placeholder="Jak vám mají kamarádi říkat"
            onChange={(e) => setName(e.target.value)}
            onBlur={() => void saveName()}
            onKeyDown={(e) => e.key === "Enter" && void saveName()}
            autoFocus={!nameReady}
          />
          {name.trim() !== savedName && (
            <button onClick={() => void saveName()} disabled={!name.trim()}>
              Uložit
            </button>
          )}
        </div>
        {!nameReady && <p className="muted">Bez jména nejde hru založit ani se připojit.</p>}
      </section>

      <div className="menu-grid">
        <section className="card">
          <h2>Nová kampaň</h2>
          <p className="muted">
            Založíte hru jako hostitel a pozvánku pošlete kamarádům. Soupeřit můžete i jen s
            počítačem.
          </p>
          <button
            className="primary"
            disabled={busy || !name.trim()}
            onClick={() => void go(() => api.hostStart(null))}
          >
            Založit hru
          </button>
        </section>

        <section className="card">
          <h2>Připojit se ke hře</h2>
          <textarea
            rows={3}
            value={invitation}
            placeholder="Vložte pozvánku od hostitele (dlouhý kód začínající „endpoint…“)"
            onChange={(e) => setInvitation(e.target.value)}
          />
          <button
            className="primary"
            disabled={busy || !name.trim() || !invitation.trim()}
            onClick={() => void go(() => api.join(invitation))}
          >
            Připojit
          </button>
        </section>
      </div>

      <section className="card">
        <div className="row spread">
          <h2>Uložené kampaně</h2>
          <button className="link" onClick={() => void run(api.openCampaignsDir())}>
            Otevřít složku
          </button>
        </div>
        {campaigns.length === 0 ? (
          <p className="muted">Zatím žádné. Kampaň se ukládá sama každý herní den.</p>
        ) : (
          <table className="list">
            <thead>
              <tr>
                <th>Kampaň</th>
                <th>Hráči</th>
                <th>Herní datum</th>
                <th>Uloženo</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {campaigns.map((c) => (
                <tr key={c.path}>
                  <td>{c.name}</td>
                  <td>{c.players.join(", ")}</td>
                  <td>
                    {c.date} <span className="muted">(den {c.day + 1} z {c.durationDays})</span>
                  </td>
                  <td className="muted">{ago(c.modified)}</td>
                  <td className="right">
                    <button
                      disabled={busy || !name.trim()}
                      onClick={() => void go(() => api.hostStart(c.path))}
                    >
                      Pokračovat
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        <p className="muted small">Složka: {info?.campaignsDir}</p>
      </section>
    </div>
  );
}

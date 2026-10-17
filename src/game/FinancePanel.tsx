// Finance: hotovost, úvěry, výsledek dne a úroveň služeb.
import { useState } from "react";

import type { GameDto } from "../api/bindings/GameDto";
import { kc, num, percent } from "../format";
import { act } from "../store";

const SERVICE = [
  { level: 1, name: "Základní", hint: "60 Kč na cestujícího, menší přitažlivost" },
  { level: 2, name: "Standard", hint: "120 Kč na cestujícího" },
  { level: 3, name: "Prémiové", hint: "220 Kč na cestujícího, větší přitažlivost" },
];

export function FinancePanel({ game }: { game: GameDto }) {
  const my = game.my;
  const s = my.stats;
  const [amount, setAmount] = useState("5000000");
  const value = Number(amount) || 0;
  const free = Math.max(0, my.loanLimit - my.loans);

  return (
    <div className="panel">
      {my.negativeDays > 0 && (
        <div className="banner bad">
          Záporná hotovost už {my.negativeDays}. den. Po {my.bankruptcyDays} dnech přijde bankrot – vezměte úvěr,
          zdražte nebo omezte flotilu.
        </div>
      )}
      <dl className="facts">
        <dt>Hotovost</dt>
        <dd className={my.cash < 0 ? "bad" : ""}>{kc(my.cash)}</dd>
        <dt>Úvěry</dt>
        <dd>
          {kc(my.loans)} <span className="muted small">z rámce {kc(my.loanLimit)}, úrok 8 % ročně</span>
        </dd>
        <dt>Hodnota firmy</dt>
        <dd>
          <strong>{kc(my.firmValue)}</strong> <span className="muted small">(podle ní je pořadí)</span>
        </dd>
      </dl>

      <h3>Výsledek</h3>
      <table className="list">
        <thead>
          <tr>
            <th />
            <th>Včera</th>
            <th>Dnes zatím</th>
          </tr>
        </thead>
        <tbody>
          <tr>
            <td>Tržby</td>
            <td>{kc(s.revenueYesterday)}</td>
            <td>{kc(s.revenueToday)}</td>
          </tr>
          <tr>
            <td>Náklady</td>
            <td>{kc(s.costYesterday)}</td>
            <td>{kc(s.costToday)}</td>
          </tr>
          <tr>
            <td>
              <strong>Výsledek</strong>
            </td>
            <td className={s.revenueYesterday - s.costYesterday < 0 ? "bad" : "good"}>
              <strong>{kc(s.revenueYesterday - s.costYesterday)}</strong>
            </td>
            <td className={s.revenueToday - s.costToday < 0 ? "bad" : "good"}>
              {kc(s.revenueToday - s.costToday)}
            </td>
          </tr>
        </tbody>
      </table>
      <p className="muted small">
        Celkem {num(s.flights)} letů ({num(s.cancelledFlights)} zrušených), {num(s.totalPax)} cestujících, průměrná
        obsazenost {percent(s.totalPax, s.totalSeats)}. Chartery: {s.chartersFlown}, vyhrané zakázky:{" "}
        {s.contractsWon}, sloty: {s.slotsWon}.
      </p>

      <h3>Úvěr</h3>
      <div className="row wrap">
        <input
          inputMode="numeric"
          value={amount}
          onChange={(e) => setAmount(e.target.value.replace(/[^\d]/g, ""))}
        />
        <button
          disabled={value <= 0 || value > free}
          onClick={() => void act({ type: "takeLoan", amount: value }, `Úvěr ${kc(value)} přijat.`)}
        >
          Půjčit
        </button>
        <button
          disabled={value <= 0 || value > my.loans || value > my.cash}
          onClick={() => void act({ type: "repayLoan", amount: value }, `Splaceno ${kc(value)}.`)}
        >
          Splatit
        </button>
        <span className="muted small">volný rámec {kc(free)}</span>
      </div>

      <h3>Služby na palubě</h3>
      <div className="service">
        {SERVICE.map((x) => (
          <button
            key={x.level}
            className={my.service === x.level ? "active" : ""}
            onClick={() =>
              my.service !== x.level &&
              void act({ type: "setService", level: x.level }, `Služby: ${x.name.toLowerCase()}.`)
            }
          >
            <strong>{x.name}</strong>
            <span className="small muted">{x.hint}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

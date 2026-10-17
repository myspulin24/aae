// Kouřový test celé hry ve Windows: dvě instance AAE (hostitel a host) přes
// tauri-driver a msedgedriver, skutečná síť iroh, skutečné ukládání.
//
//   node scripts/smoke.mjs            sestavit (debug) a otestovat
//   node scripts/smoke.mjs --no-build použít target/debug/aae.exe
//
// Cesta: menu → hostitel založí hru → host se připojí pozvánkou → AI soupeř →
// zahájení → host naplánuje kyvadlo → hlasování o 4× → uplyne herní den
// (automatické uložení) → hostitel odejde a host uvidí konec hry.
// Snímky obrazovky ukládá do target/smoke.
import { execFileSync, execSync, spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const shots = join(root, "target", "smoke");
mkdirSync(shots, { recursive: true });

if (!process.argv.includes("--no-build")) {
  execSync("npx tauri build --debug --no-bundle", { cwd: root, stdio: "inherit" });
}
const app = join(root, "target", "debug", "aae.exe");
if (!existsSync(app)) throw new Error(`Chybí ${app}`);

// ---------------------------------------------------------------- ovladače

function webview2Version() {
  const keys = [
    "HKLM\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
    "HKCU\\Software\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
  ];
  for (const key of keys) {
    try {
      const out = execFileSync("reg", ["query", key, "/v", "pv"], { encoding: "utf8" });
      const m = /pv\s+REG_SZ\s+([\d.]+)/.exec(out);
      if (m) return m[1];
    } catch {
      // další klíč
    }
  }
  throw new Error("WebView2 Runtime nebyl nalezen.");
}

async function edgeDriver() {
  const version = webview2Version();
  const dir = join(root, "target", "e2e", `edgedriver-${version}`);
  const exe = join(dir, "msedgedriver.exe");
  if (existsSync(exe)) return exe;
  mkdirSync(dir, { recursive: true });
  const res = await fetch(`https://msedgedriver.microsoft.com/${version}/edgedriver_win64.zip`);
  if (!res.ok) throw new Error(`Stažení msedgedriver selhalo: ${res.status}`);
  const zip = join(dir, "edgedriver.zip");
  writeFileSync(zip, Buffer.from(await res.arrayBuffer()));
  const tar = join(process.env.SystemRoot ?? "C:/Windows", "System32", "tar.exe");
  execFileSync(tar, ["-xf", zip, "-C", dir], { stdio: "inherit" });
  return exe;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const ELEMENT = "element-6066-11e4-a52e-4f735466cecf";

/** Minimální klient WebDriveru (W3C) pro jednu instanci aplikace. */
class Driver {
  constructor(label, port) {
    this.label = label;
    this.port = port;
  }

  async req(method, path, body) {
    const res = await fetch(`http://127.0.0.1:${this.port}${path}`, {
      method,
      headers: { "content-type": "application/json" },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const json = await res.json();
    if (!res.ok || json.value?.error) {
      throw new Error(`[${this.label}] ${method} ${path}: ${JSON.stringify(json.value)}`);
    }
    return json.value;
  }

  async start(env) {
    const native = this.port + 1;
    this.proc = spawn("tauri-driver", ["--port", String(this.port), "--native-port", String(native), "--native-driver", this.edge], {
      env: { ...process.env, ...env },
      stdio: ["ignore", "ignore", "inherit"],
    });
    await sleep(1500);
    const s = await this.req("POST", "/session", {
      capabilities: { alwaysMatch: { "tauri:options": { application: app } } },
    });
    this.sid = s.sessionId;
    // Cílové rozlišení hry; na jiných monitorech by snímky nešly porovnat.
    await this.req("POST", this.s("/window/rect"), { x: this.port === 4444 ? 0 : 200, y: 0, width: 1920, height: 1080 }).catch((e) => console.log(`  velikost okna: ${e.message}`));
  }

  s(path) {
    return `/session/${this.sid}${path}`;
  }

  exec(script, ...args) {
    return this.req("POST", this.s("/execute/sync"), { script, args });
  }

  waitFor(what, script, ...args) {
    return this.waitLong(what, 60_000, script, ...args);
  }

  async waitLong(what, ms, script, ...args) {
    const deadline = Date.now() + ms;
    let last;
    while (Date.now() < deadline) {
      try {
        last = await this.exec(script, ...args);
        if (last) return last;
      } catch (e) {
        last = e.message;
      }
      await sleep(250);
    }
    await this.shot(`chyba-${what.replace(/\W+/g, "-")}`);
    throw new Error(`[${this.label}] nečekal jsem se: ${what} (poslední: ${JSON.stringify(last)})`);
  }

  async find(css) {
    const el = await this.req("POST", this.s("/element"), { using: "css selector", value: css });
    return el[ELEMENT];
  }

  async type(css, text) {
    const id = await this.find(css);
    await this.req("POST", this.s(`/element/${id}/clear`), {});
    await this.req("POST", this.s(`/element/${id}/value`), { text });
  }

  /** Klikne na tlačítko podle začátku textu. */
  async click(text, within = "body") {
    await this.waitFor(`tlačítko „${text}“`, CLICK, text, within);
  }

  async select(css, value) {
    await this.waitFor(`výběr ${css}`, SELECT, css, String(value));
  }

  async text() {
    return this.exec("return document.body.innerText");
  }

  async shot(name) {
    const png = await this.req("GET", this.s("/screenshot"));
    const file = join(shots, `${this.label}-${name}.png`);
    writeFileSync(file, Buffer.from(png, "base64"));
    console.log(`  snímek ${file}`);
  }

  async stop() {
    try {
      if (this.sid) await this.req("DELETE", this.s(""));
    } catch {
      // aplikace už mohla skončit
    }
    this.proc?.kill();
  }
}

const CLICK = `
  const [text, within] = arguments;
  const scope = document.querySelector(within) ?? document.body;
  const b = [...scope.querySelectorAll("button")].find(
    (x) => !x.disabled && x.textContent.trim().startsWith(text) && x.offsetParent !== null);
  if (!b) return false;
  b.click();
  return true;`;

const SELECT = `
  const [css, value] = arguments;
  const el = document.querySelector(css);
  if (!el || ![...el.options].some((o) => o.value === value)) return false;
  const setter = Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set;
  setter.call(el, value);
  el.dispatchEvent(new Event("change", { bubbles: true }));
  return true;`;

const HAS_TEXT = `return document.body.innerText.includes(arguments[0]);`;

// ---------------------------------------------------------------- průběh

const edge = await edgeDriver();
const work = mkdtempSync(join(tmpdir(), "aae-smoke-"));
const campaigns = join(work, "Kampaně");
const stamp = Date.now();
const host = new Driver("hostitel", 4444);
const guest = new Driver("host", 4454);
host.edge = guest.edge = edge;

let ok = false;
try {
  console.log("▶ Spouštím hostitele");
  await host.start({ AAE_PROFIL: `smoke-h-${stamp}`, AAE_KAMPANE: campaigns });
  await host.waitFor("menu", HAS_TEXT, "Nová kampaň");
  await host.type(".menu .card input", "Hostitel");
  await host.click("Založit hru");
  await host.waitFor("lobby s pozvánkou", `return document.querySelector(".invitation textarea")?.value || false`);
  const invitation = await host.exec(`return document.querySelector(".invitation textarea").value`);
  console.log(`  pozvánka ${invitation.slice(0, 24)}… (${invitation.length} znaků)`);
  await host.shot("1-lobby");

  console.log("▶ Spouštím hosta a připojuji pozvánkou");
  await guest.start({ AAE_PROFIL: `smoke-g-${stamp}`, AAE_KAMPANE: join(work, "host") });
  await guest.waitFor("menu", HAS_TEXT, "Připojit se ke hře");
  await guest.type(".menu .card input", "Kamarád");
  await guest.type(".menu textarea", invitation);
  await guest.click("Připojit");
  await guest.waitFor("lobby u hosta", HAS_TEXT, "Čeká se, až hostitel zahájí kampaň");
  await host.waitFor("host v lobby", HAS_TEXT, "Kamarád");

  console.log("▶ Hostitel přidá AI a zahájí kampaň");
  await host.click("+ Hub");
  await host.waitFor("AI v seznamu", HAS_TEXT, "Odebrat");
  await host.shot("2-lobby-plna");
  await guest.shot("2-lobby");
  await host.click("Zahájit kampaň");
  await host.waitFor("hra u hostitele", `return !!document.querySelector(".topbar")`);
  await guest.waitFor("hra u hosta", `return !!document.querySelector(".topbar")`);
  await host.shot("3-hra");

  console.log("▶ Host naplánuje kyvadlo prvním letadlem");
  await guest.waitFor("letadla v plánu", `return document.querySelectorAll(".gantt-row").length >= 2`);
  await guest.exec(`document.querySelector(".gantt-row").click(); return true`);
  await guest.waitFor("editor plánu", `return !!document.querySelector(".plan-editor")`);
  // Linka s rozumnou poptávkou (Berlín, Mnichov nebo Varšava podle domova).
  const dest = await guest.exec(
    `const s = document.querySelector(".shuttle select");
     const o = [...s.options].find((x) => /^(BER|MUC|WAW) /.test(x.textContent));
     return o ? o.value : false`,
  );
  if (!dest) throw new Error("V nabídce kyvadla chybí BER/MUC/WAW.");
  await guest.select(".shuttle select", dest);
  await guest.click("Vyplnit", ".plan-editor");
  await guest.waitFor("platný plán", HAS_TEXT, "Plán je v pořádku");
  await guest.shot("4-editor");
  await guest.click("Uložit plán", ".plan-editor");
  await guest.waitFor("potvrzení plánu", HAS_TEXT, "Plán uložen");

  console.log("▶ Hlasování o rychlosti 4×");
  await host.click("4×", ".speed");
  await guest.waitFor("hlasování u hosta", HAS_TEXT, "navrhuje zrychlit");
  await guest.shot("5-hlasovani");
  await guest.click("Ano", ".vote-card");
  await host.waitFor("rychlost 4×", `return document.querySelector(".speed .active")?.textContent === "4×"`);

  console.log("▶ Čekám na další herní den (automatické uložení)");
  await host.waitFor("automatické uložení", HAS_TEXT, "Uloženo:");
  await host.click("Burza", ".tabs");
  await host.shot("6-burza");
  await guest.click("Finance", ".tabs");
  await guest.shot("6-finance");
  console.log("▶ Letadlo hosta letí podle plánu a vydělává");
  await guest.click("Linky", ".tabs");
  await guest.waitFor("linka v přehledu", `return !!document.querySelector(".routes-table")`);
  await guest.waitLong("letadlo ve vzduchu", 120_000, `return document.querySelectorAll(".planes g").length > 0`);
  await guest.shot("8-let");
  await guest.click("Finance", ".tabs");
  await guest.waitLong(
    "tržby za včerejšek",
    120_000,
    `const row = [...document.querySelectorAll(".panel table.list tr")].find((r) => r.cells[0]?.textContent === "Tržby");
     return !!row && !row.cells[1].textContent.startsWith("0");`,
  );
  await guest.click("Linky", ".tabs");
  await guest.shot("9-linky");
  await host.click("Soupeři", ".tabs");
  await host.shot("9-souperi");

  const saved = readdirSync(campaigns);
  console.log(`  uložené kampaně: ${saved.join(", ")}`);
  if (saved.length !== 1) throw new Error("Kampaň se neuložila.");

  console.log("▶ Hostitel odchází, host uvidí konec hry");
  await host.click("Odejít", ".topbar");
  await host.click("Uložit a ukončit?", ".topbar");
  await host.waitFor("menu po odchodu", HAS_TEXT, "Uložené kampaně");
  await guest.waitFor("konec u hosta", HAS_TEXT, "Hostitel ukončil hru");
  await guest.shot("7-konec");
  await host.shot("7-menu");
  ok = true;
  console.log("\n✓ Kouřový test prošel");
} finally {
  await guest.stop();
  await host.stop();
  // Profily z testu do dat aplikace nepatří.
  for (const who of ["h", "g"]) {
    const profile = join(process.env.APPDATA ?? "", "io.github.myspulin24.aae", `profil-smoke-${who}-${stamp}.json`);
    rmSync(profile, { force: true });
  }
  if (ok) rmSync(work, { recursive: true, force: true });
  else console.error(`Data ponechána: ${work}`);
}
process.exit(ok ? 0 : 1);

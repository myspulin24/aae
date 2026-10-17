# AAE – American Airlines Express

Letecká manažerská hra pro 2–4 kamarády: živá online kampaň na víc večerů,
inspirovaná Airline Tycoon Deluxe (ekonomika, plánování letů, burza zakázek),
bez vlastního serveru (hostitel je server, kamarádi se připojí pozvánkou přes iroh).

- Návrh a technická revize: `personal-dev/notes/letecka-hra-navrh.md`
- Plný název je pracovní a slouží jen pro zobrazení; složky, repo a identifikátory používají neutrální `aae`.

## Stav

| Vlna | Obsah | Stav |
|---|---|---|
| 1 | Síťový test iroh ([`spikes/nettest`](spikes/nettest/README.md)) | hotovo; LAN i internet ověřeno, zbývá test se třemi sítěmi |
| 2 | Workspace, CI, jádro simulace [`crates/sim`](crates/sim), laboratoř | hotovo |
| 3 | [`crates/net`](crates/net): protokol, Transport (iroh + síť v paměti), hostitel, klient | hotovo |
| 4 | [`src-tauri`](src-tauri) + UI v Reactu ([`src`](src)), typy z ts-rs, kouřový test dvou oken | hotovo |
| 5 | Ukládání a zálohy kampaní, log záměrů, updater, vydávací workflow | hotovo |

## Jak hrát

1. Nainstalujte `AAE_…_x64-setup.exe` (vydání na GitHubu) a zadejte jméno.
2. Hostitel klikne na **Založit hru**, zkopíruje pozvánku a pošle ji kamarádům.
   Soupeře řízené počítačem přidá v lobby; pak **Zahájit kampaň**.
3. Kamarádi vloží pozvánku do **Připojit se ke hře**.
4. Ve hře: letový plán dole (klik na letadlo → editor, letiště jde přidat i kliknutím
   na mapu, „Kyvadlo“ vyplní celý den), ceny v záložce Linky, letadla ve Flotile,
   zakázky a aukce slotů na Burze. Zrychlení odsouhlasí všichni, zpomalení platí hned.
5. Kampaň ukládá hostitel sám každý herní den do `DokumentyAAEKampaně`
   (posledních 10 dní a začátky 5 relací v podsložce `zalohy`). Další večer hostitel
   zvolí **Pokračovat** a kamarádi se připojí novou pozvánkou.

## Aplikace (`src-tauri`, `src`)

Hostitel spouští hostitelskou smyčku z `aae-net` a připojuje se k ní vlastním klientem
v paměti; host se připojuje přes iroh a po výpadku to zkouší znovu každých 5 s. Okno
volá jen příkazy z `src-tauri/build.rs` a dostává stav jako události `aae://…`
(pohled filtrovaný hostitelem, R4). Typy pro TypeScript generuje ts-rs do
`src/api/bindings`; `npm test` hlídá, že jsou v repozitáři aktuální.

## Jádro simulace (`crates/sim`)

Deterministická simulace: stav mění jen `GameState::apply` (záměr) a `GameState::step`
(1 herní minuta). Bez systémového času, vláken a `HashMap`; vlastní generátor náhody
uložený ve stavu; peníze v haléřích; fronta událostí řazená podle (krok, pořadí).
Obsahuje gravitační model poptávky, dělení cestujících mezi lety (R8), burzu se sběrným
oknem a losem, aukce slotů s ochrannou lhůtou, pauzu jako množinu důvodů, rychlosti
1×/2×/4× s přepočtem lhůt, stávky a počasí, bankrot (R11) a AI soupeře se 4 strategiemi.

Herní data: `crates/sim/data/airports.csv` (75 letišť Evropy; souřadnice z OurAirports,
Unlicense) a `aircraft.csv` (7 typů). Populace, bohatství, ceny a náklady jsou přibližné
herní hodnoty k vyladění.

## Příkazy

```
npm start                        # vývoj: okno s živým rozhraním (tauri dev)
npm run package                  # instalátor NSIS do target/release/bundle/nsis
node scripts/smoke.mjs           # kouřový test: dvě okna, síť, plán, hlasování, uložení
npm test                         # vše, co spouští CI (typy, Rust, ts-rs, vitest)
npm run lab -- --runs 20         # laboratoř: 20 kampaní po 2 letech se 4 AI
npm run lab -- --runs 1 --report 30   # průběh financí po 30 dnech
npm run lab -- --runs 50 --csv lab.csv
cargo test --release -p aae-sim -- --ignored   # 10 let bez přetečení
```

Druhé okno na jednom počítači (zkouška hostitele a hosta): proměnná `AAE_PROFIL=druhy`
zvolí jiný profil, `AAE_KAMPANE=<složka>` přesměruje uložené kampaně.

## Vydání

1. Doplnit sekci do `CHANGELOG.md` a zvednout verzi v `package.json`, `Cargo.toml`
   (workspace) a `src-tauri/tauri.conf.json`.
2. Lokálně `npm test` a `node scripts/smoke.mjs`.
3. Po zeleném CI na `main` tag `vX.Y.Z` → workflow `release` sestaví podepsaný instalátor
   a `latest.json` pro updater (klíč v secrets, lokálně `~/.tauri/aae.key`).

## Laboratoř – první vyladění (2026-10-17)

50 kampaní po 2 letech (seed 100–149), 4 AI, rychlost 1×:

| Strategie | Výher | Průměrná hodnota firmy | Bankrotů |
|---|---:|---:|---:|
| levná | 13 | 103 mil. Kč | 0 |
| prémiová | 7 | 78 mil. Kč | 2 |
| hub | 15 | 93 mil. Kč | 0 |
| charter | 15 | 72 mil. Kč | 13 |

Startovní kapitál 25 mil. Kč. Nejčastější vítěz vyhrál 30 % běhů; nejpomalejší běh 0,86 s
(kritérium < 5 s). Při 4× podobné rozložení (max 35 %). K dalšímu ladění: charter je
riskantní (většina bankrotů), v desetiletých bězích (mimo výchozí kampaň) charter
převažuje a jedna aerolinka narostla nad 0,5 mld. Kč.

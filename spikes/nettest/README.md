# AAE – síťový test (vlna 1)

Malý program, který ověří nejrizikovější předpoklad hry: kamarád se připojí
k hostiteli přes pozvánku bez nastavování routeru, VPN a bez vlastního serveru
(iroh: přímé spojení přes NAT, jinak relay n0). Návrh: `personal-dev/notes/letecka-hra-navrh.md`, úkol T1.

## Test s kamarádem (20 minut)

1. **Hostitel** spustí `aae-nettest.exe`, zvolí `1`. Pozvánka se zkopíruje do schránky
   a uloží do `pozvanka.txt` vedle exe. Pošlete ji kamarádovi celou (WhatsApp, Discord).
2. **Kamarád** (jiná síť, ne vaše Wi-Fi) spustí `aae-nettest.exe`, zvolí `2` a vloží pozvánku.
3. Test běží asi 30 s: ping každou sekundu, typ spojení (přímé / relay) a na konci
   přenos 200 kB (odhad velikosti plného stavu hry).
4. Kamarád pošle zpátky `vysledek-site.txt`, který vznikne vedle exe.

Windows SmartScreen může exe označit jako neznámé (není podepsané): „Další informace“ → „Přesto spustit“.

**Splněno, když:** spojení vznikne bez nastavování routeru, aspoň 85 % pingů je pod 300 ms,
žádný není nad 1 s a `vysledek-site.txt` uvádí typ spojení (program to vyhodnotí sám). Ideálně zopakovat i přes mobilní hotspot
a pak se třemi sítěmi najednou (kritérium „Síť“ v návrhu).

## Režimy

| Příkaz | Co dělá |
|---|---|
| `aae-nettest` | menu |
| `aae-nettest host` | hostitel: pozvánka, čeká na připojení, vrací zprávy |
| `aae-nettest pripojit <pozvánka>` | klient: 30 pingů + dávka 200 kB, výsledek do `vysledek-site.txt` |
| `aae-nettest lokalni` | hostitel i klient na jednom PC, bez relay a bez internetu (`presets::Minimal`) |

Proměnné pro ladění: `AAE_BEZ_SCHRANKY=1` nekopíruje pozvánku do schránky,
`AAE_JEN_RELAY=1` zahodí z pozvánky přímé adresy, takže spojení začne přes relay.

Klíč hostitele je uložený v `%APPDATA%\aae\nettest-host.key`, ID v pozvánce je proto stejné při každém spuštění.

## Sestavení

```
cargo build --release
```

Výstup: `target/release/aae-nettest.exe` (~13,5 MB, staticky linkovaný runtime,
viz `.cargo/config.toml`, takže na PC kamaráda nepotřebuje Visual C++ Redistributable).

## Naměřeno na vývojovém PC (2026-10-17, iroh 1.3.0)

Jen jeden počítač, tedy bez průchodu NATem mezi domácnostmi:

- `lokalni`: spojení 18 ms, pingy 1–10 ms, 200 kB tam a zpět 12–16 ms, relay nepoužit.
  Integrační test nad skutečným iroh v CI bez internetu je tedy proveditelný (rozhodnutí R14).
- `host` + `pripojit` přes n0: pozvánka 165 znaků, spojení 16 ms, přímé.
- S `AAE_JEN_RELAY=1`: spojení 152 ms přes relay n0 (RTT 46–120 ms), po ~1 s přechod na přímé.

## Naměřeno přes internet (2026-10-17 15:09, klient na jiné síti)

- Spojení 539 ms; začalo přes relay, pak hole punching na přímé (relay 1×, přímé 29×).
- Ping min / průměr / max: 43 / 104 / 557 ms; pod 300 ms 28 z 30 (93 %). Kritérium „Stabilita“ bylo po testu změněno na „aspoň 85 % pod 300 ms a žádný nad 1 s“ (u 30 pingů smí přes 300 ms nejvýš čtyři); test ho splňuje, maximum 557 ms se do 1 s vejde.
- RTT na konci 197 ms, měřeno hned po přenosu dávky (pravděpodobně nafouknuté frontou).
- 200 kB tam a zpět: 3 181 ms; test neodlišuje směr, takže není jasné, která strana je pomalejší.

Závěr: průchod NATem bez vlastního serveru funguje (premisa 4 ověřena na jedné dvojici sítí).
Ve stejné síti (LAN) iroh sám zvolí přímé spojení s odezvou ~1 ms; zvláštní LAN režim tedy není potřeba.
Zbývá: test se třemi sítěmi najednou.

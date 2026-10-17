// Vytáhne z CHANGELOG.md sekci k jedné verzi (`node scripts/changelog.mjs 0.1.0`).
// Text se stane popisem vydání na GitHubu i poznámkou v `latest.json`, kterou
// ukáže menu hry u nabídky aktualizace. Chybějící sekce = chyba, nic se nevydá.
import { readFileSync } from "node:fs";

const version = process.argv[2];
if (!version) {
  console.error("Použití: node scripts/changelog.mjs <verze>");
  process.exit(2);
}
const escaped = version.replace(/[.*+?^${}()|[\]\]/g, "\$&");
const heading = new RegExp(`^##\s+v?${escaped}\s*(?:[—–-].*)?$`);
const lines = readFileSync("CHANGELOG.md", "utf8").split(/\r?\n/);
const start = lines.findIndex((l) => heading.test(l));
if (start === -1) {
  console.error(`V CHANGELOG.md chybí sekce ## ${version}`);
  process.exit(1);
}
const rest = lines.slice(start + 1);
const end = rest.findIndex((l) => /^##\s/.test(l));
const text = (end === -1 ? rest : rest.slice(0, end)).join("\n").trim();
if (!text) {
  console.error(`Sekce ## ${version} v CHANGELOG.md je prázdná`);
  process.exit(1);
}
console.log(text);

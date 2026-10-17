// Sada oprávnění `app-commands` ze seznamu COMMANDS v src-tauri/build.rs.
// `node scripts/gen-permissions.mjs` zapíše soubor, `--check` jen ověří shodu (CI).
import { readFileSync, writeFileSync } from "node:fs";

const build = readFileSync("src-tauri/build.rs", "utf8");
const list = build.match(/COMMANDS: &\[&str\] = &\[([\s\S]*?)\];/);
if (!list) throw new Error("V build.rs chybí COMMANDS.");
const commands = [...list[1].matchAll(/"([a-z_]+)"/g)].map((m) => m[1]);

const toml = [
  "# Vygenerováno ze seznamu COMMANDS v build.rs skriptem scripts/gen-permissions.mjs.",
  "",
  "[[set]]",
  'identifier = "app-commands"',
  'description = "Všechny vlastní příkazy AAE. Nic dalšího okno volat nesmí."',
  "permissions = [",
  ...commands.map((c) => `  "allow-${c.replaceAll("_", "-")}",`),
  "]",
  "",
].join("\n");

const path = "src-tauri/permissions/app-commands.toml";
if (process.argv.includes("--check")) {
  const current = readFileSync(path, "utf8").replaceAll("\r\n", "\n");
  if (current !== toml) {
    console.error(`${path} neodpovídá build.rs – spusťte node scripts/gen-permissions.mjs`);
    process.exit(1);
  }
} else {
  writeFileSync(path, toml);
}

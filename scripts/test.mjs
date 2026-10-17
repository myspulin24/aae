// `npm test`: celá sada – typy, oprávnění, Rust (včetně exportu typů ts-rs),
// čerstvost vygenerovaných typů a testy rozhraní.
import { execSync } from "node:child_process";

function step(title, cmd) {
  console.log(`\n▶ ${title}\n  ${cmd}`);
  execSync(cmd, { stdio: "inherit" });
}

step("Kontrola typů TypeScriptu", "npx tsc --noEmit -p tsconfig.json");
step("Oprávnění příkazů odpovídají build.rs", "node scripts/gen-permissions.mjs --check");
step("Testy Rustu (simulace, síť, aplikace)", "cargo test --workspace --locked");

// ts-rs zapisuje typy při testech; v repozitáři musí být aktuální (R13).
const changed = execSync("git status --porcelain -- src/api/bindings", { encoding: "utf8" }).trim();
if (changed) {
  console.error(`\nVygenerované typy v src/api/bindings nejsou v repozitáři aktuální:\n${changed}`);
  console.error("Spusťte `cargo test -p aae-app` a změny commitněte.");
  process.exit(1);
}

step("Testy rozhraní", "npx vitest run");

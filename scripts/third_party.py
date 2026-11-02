#!/usr/bin/env python3
"""Erzeugt THIRD-PARTY.md: Lizenzen aller Rust-Abhängigkeiten der App (aus `cargo metadata`)
und der npm-Pakete, die im Frontend landen, plus wimlib.

    python3 scripts/third_party.py > THIRD-PARTY.md
"""
import json
import pathlib
import subprocess
from collections import defaultdict

root = pathlib.Path(__file__).resolve().parent.parent

meta = json.loads(
    subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked"], cwd=root
    )
)
pkgs = {p["id"]: p for p in meta["packages"]}
nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
own = {p["id"] for p in meta["packages"] if p["source"] is None}


def runtime_deps(start_ids):
    """Nur normale Abhängigkeiten (keine Build-/Test-only), über alle Plattformen."""
    seen, stack = set(), list(start_ids)
    while stack:
        i = stack.pop()
        for d in nodes[i]["deps"]:
            if any(k["kind"] is None for k in d["dep_kinds"]) and d["pkg"] not in seen:
                seen.add(d["pkg"])
                stack.append(d["pkg"])
    return seen - own


by_license = defaultdict(list)
for i in runtime_deps(own):
    p = pkgs[i]
    by_license[p.get("license") or "siehe Paket"].append(f'{p["name"]} {p["version"]}')

npm = json.loads((root / "package-lock.json").read_text())
npm_rt = []
for path, info in npm.get("packages", {}).items():
    name = path.removeprefix("node_modules/")
    # Svelte ist formal eine Entwicklungs-Abhängigkeit, seine Laufzeit wird aber in die
    # Oberfläche hineinkompiliert.
    if path.startswith("node_modules/") and (not info.get("dev") or name in ("svelte", "esm-env", "clsx")):
        npm_rt.append((path.removeprefix("node_modules/"), info.get("version", "?"), info.get("license", "?")))

out = [
    "# Software von Dritten",
    "",
    "SealFlash steht unter der GPL-3.0-or-later. Es enthält bzw. nutzt folgende Software anderer",
    "Projekte. Diese Datei wird mit `python3 scripts/third_party.py > THIRD-PARTY.md` erzeugt.",
    "",
    "## wimlib (nur Windows-Installer)",
    "",
    "`wimlib-imagex.exe` und `libwim-15.dll` aus **wimlib 1.14.5** (<https://wimlib.net>), unverändert.",
    "Lizenz: GPL-3.0-or-later (libwim wahlweise LGPL-2.1-or-later). Die Lizenztexte liegen im",
    "Installationsordner unter `licenses/wimlib/`. Der vollständige Quelltext liegt jedem",
    "SealFlash-Release als `wimlib-1.14.5.tar.gz` bei und ist unter",
    "<https://wimlib.net/downloads/wimlib-1.14.5.tar.gz> erhältlich.",
    "",
    "Unter Linux wird das wimlib der Distribution verwendet (Paket `wimtools` bzw. `wimlib-utils`).",
    "",
    "## Rust-Bibliotheken",
    "",
]
for lic in sorted(by_license):
    out.append(f"**{lic}**: " + ", ".join(sorted(by_license[lic], key=str.lower)))
    out.append("")
out += ["## JavaScript-Bibliotheken (Oberfläche)", ""]
for name, ver, lic in sorted(npm_rt):
    out.append(f"- {name} {ver} – {lic}")
print("\n".join(out))

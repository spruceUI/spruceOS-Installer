#!/usr/bin/env python3
# Writes manifest/manifest.json from the latest release of each TF1 image
# source. CI runs this for every beta and versioned release and for the daily
# refresh of the latest release; the output is never committed.
# Run from the repo root: python3 manifest/generate.py

import json
import re
import subprocess

SOURCES = [
    {
        "repo": "spruceUI/dArkMoss",
        "os": "dArkMoss",
        "match": r"\.img\.7z\.\d{3}$",
        "devices": {
            "_RGB30_": "Powkiddy RGB30",
            "_MINILOONG_": "Miniloong Pocket 1",
            "_V10_": "Powkiddy V10",
            "_A10MINI_": "A10 Mini",
            "_G350_": "G350",
            "_XUMINI_": "MagicX XU Mini M",
            "_RG353M_": "Anbernic RG353M",
        },
        "aliases": {"_RGB30_": {"_RGB20SX_": "Powkiddy RGB20SX"}},
        "note": "Debian base for TF1; spruceOS goes on TF2",
        # One card slot: the installer adds SPRUCEOS behind a rootfs of this
        # many GiB (dArkMoss firstboot's size) and puts spruceOS on it.
        "single_card": {"_V10_": 11},
        "single_card_note": "Debian base; spruceOS shares the same card",
    },
    {
        "repo": "pvaibhav/BaseOS",
        "os": "BaseOS",
        "match": r"\.img\.zip$",
        "devices": {
            "-rg28xx-": "Anbernic RG28XX",
            "-rg34xx-": "Anbernic RG34XX",
            "-rg34xxsp-": "Anbernic RG34XX SP",
            "-rg35xxh-": "Anbernic RG35XX H",
            "-rg35xxplus-": "Anbernic RG35XX Plus",
            "-rg35xxpro-": "Anbernic RG35XX Pro",
            "-rg35xxsp-": "Anbernic RG35XX SP",
            "-rg40xxh-": "Anbernic RG40XX H",
            "-rg40xxv-": "Anbernic RG40XX V",
            "-rgcubexx-": "Anbernic RG CubeXX",
            "-rgsp-": "Anbernic RG SP",
        },
        "note": "third-party base by pvaibhav for TF1; spruceOS goes on TF2",
    },
    {
        "repo": "spruceUI/oakMOSS",
        "os": "oakMOSS",
        "match": r"-sd1\.img\.xz$",
        "devices": {"-zero28-": "MagicX Zero 28", "-zero40-": "MagicX Zero 40", "-xu20-": "MagicX XU20"},
        "note": "SD1 base image; the spruce card goes in SD2",
    },
]


def latest_release(repo):
    out = subprocess.check_output(["gh", "api", f"repos/{repo}/releases/latest"])
    return json.loads(out)


def main():
    assets = []
    for source in SOURCES:
        release = latest_release(source["repo"])
        tag = release["tag_name"]
        for asset in release["assets"]:
            name = asset["name"]
            if not re.search(source["match"], name):
                continue
            device = next((label for key, label in source["devices"].items() if key in name), None)
            if device is None:
                print(f"skipping {name}: no device mapping")
                continue
            entries = [(name, device)]
            for key, aliases in source.get("aliases", {}).items():
                if key in name:
                    entries += [(name.replace(key, alias), label) for alias, label in aliases.items()]
            rootfs_gib = next((gib for key, gib in source.get("single_card", {}).items() if key in name), None)
            note = source["single_card_note"] if rootfs_gib else source["note"]
            for entry_name, label in entries:
                entry = {
                    "name": entry_name,
                    "url": asset["browser_download_url"],
                    "size": asset["size"],
                    "display_name": label,
                    "devices": f"{source['os']} {tag}: {note}",
                }
                if rootfs_gib:
                    entry["single_card_rootfs_gib"] = rootfs_gib
                assets.append(entry)
    assets.sort(key=lambda a: (a["display_name"], a["name"]))
    manifest = {"version": "1.0", "display_name": "TF1 image", "assets": assets}
    with open("manifest/manifest.json", "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"wrote {len(assets)} assets")


if __name__ == "__main__":
    main()

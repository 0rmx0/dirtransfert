import re
import sys

version = sys.argv[1]
changelog = sys.argv[2]
output = sys.argv[3]

text = open(changelog, encoding="utf-8").read()
m = re.search(r"## \[" + re.escape(version) + r"\][^\n]*\n(.*?)(?=\n## \[|\Z)", text, re.S)
section = m.group(0) if m else ""
if not section:
    section = (
        "## "
        + version
        + "\n\n- Voir le CHANGELOG.md du depot pour les changements de cette version."
    )

assets = """

---

## Plateformes

- Linux amd64 / arm64 (tar.gz)
- Windows amd64 (zip)
- macOS amd64 / arm64 (tar.gz)
- Paquet Debian .deb (amd64)

Chaque archive est accompagnee de sa somme SHA-256 (fichier `.sha256`).

Voir le README du depot pour la documentation complete.
"""

open(output, "w", encoding="utf-8").write(section + assets)

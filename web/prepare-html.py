# SPDX-License-Identifier: MIT
"""Replace Trunk's inline bootstrap with the external EarthCraft loader."""

from pathlib import Path
from os import environ

index = Path(environ["TRUNK_STAGING_DIR"], "index.html")
source = index.read_text(encoding="utf-8")
opening = '<script type="module">'
before, found, remainder = source.partition(opening)
generated, ending, after = remainder.partition("</script>")
if not found or not ending or "TrunkApplicationStarted" not in generated:
    raise SystemExit("Trunk did not generate the expected EarthCraft bootstrap.")
if 'src="./startup.mjs"' not in after:
    raise SystemExit("The external EarthCraft loader is missing.")
index.write_text(before + after, encoding="utf-8")

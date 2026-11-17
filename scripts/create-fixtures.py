#!/usr/bin/env python3
"""Create only synthetic QA files in a new directory; never overwrite projects."""
import argparse
import json
import os
from pathlib import Path


def write_fixture(path, contents):
    with path.open("xb") as file:
        os.chmod(path, 0o600)
        file.write(contents.encode("utf-8"))


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("directory", type=Path)
parser.add_argument("--stress", action="store_true",
                    help="add a catalog of 20 projects and a 1,000-variable document")
args = parser.parse_args()
root = args.directory.resolve()
root.mkdir(mode=0o700, parents=True, exist_ok=False)
small = (
    "# Proyecto sintético de QA\n"
    "APP_NAME=Dotvar de prueba\n"
    "EMPTY=\n"
    "  export API_URL = 'https://example.invalid'  # comentario conservado\n"
    "DUPLICATE=primero\nDUPLICATE=segundo\n"
    'MULTILINE="primera línea\nsegunda línea"\n'
    "\n# Fragmento que debe conservarse\n"
    "not a supported assignment\n"
)
for name, contents in {
    ".env": small,
    ".env.local": "# Archivo local sintético\nLOCAL_MARKER=segundo archivo\n",
    ".env.production": "# Producción sintética\nPRODUCTION_MARKER=sin credenciales\n",
    "sample.env": "# Otro nombre de archivo\nUNICODE='Español: áéíóú 日本語'\n",
}.items():
    write_fixture(root / name, contents)
large = root / "benchmark"
large.mkdir(mode=0o700)
sample = "# 200 variables sinteticas\n" + "".join(
    f"QA_{index:04}=synthetic-{index:04}\n" for index in range(200)
)
for name in [".env", ".env.local", ".env.staging", ".env.production"]:
    write_fixture(large / name, sample)

if args.stress:
    projects = []
    for index in range(20):
        project = root / "stress" / f"Proyecto-{index:02}"
        project.mkdir(mode=0o700, parents=True)
        active = project / ".env"
        size = 1000 if index == 0 else 200
        values = [f"QA_{row:04}=synthetic-{row:04}\n" for row in range(size)]
        values[0] = 'OPENAI_API_KEY="synthetic-0000"  # comentario inicial\n'
        values[2] = '# Comentario sintético\nQA_0002="primera línea\nsegunda línea"\n'
        write_fixture(active, "".join(values))
        tabs = [str(active)]
        if index == 0:
            for name in (".env.local", ".env.staging", ".env.production"):
                path = project / name
                write_fixture(path, sample)
                tabs.append(str(path))
        projects.append({"id": index + 1, "root": str(project), "tabs": tabs,
                         "active_tab": str(active)})
    config = root / "stress-config"
    config.mkdir(mode=0o700)
    write_fixture(config / "catalog.json",
                  json.dumps({"version": 1, "projects": projects, "active_project": 1}))
print(root)

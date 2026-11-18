#!/usr/bin/env python3
"""Write and verify the Finder layout of Dotvar's disk image."""

import argparse
from pathlib import Path

import dmgbuild
from ds_store import DSStore


WINDOW = ((200, 200), (720, 500))
ICONS = {"Dotvar.app": (208, 213), "Applications": (512, 213)}
ICON_SIZE = 112
HIDDEN_BARS = {
    "ShowToolbar": "show_toolbar",
    "ShowSidebar": "show_sidebar",
    "ShowStatusBar": "show_status_bar",
    "ShowPathbar": "show_pathbar",
    "ShowTabView": "show_tab_view",
}


def build(app, background, output):
    dmgbuild.build_dmg(str(output.resolve()), "Dotvar", settings={
        "format": "UDZO",
        "filesystem": "HFS+",
        "files": [str(app.resolve())],
        "symlinks": {"Applications": "/Applications"},
        "background": str(background.resolve()),
        "window_rect": WINDOW,
        "default_view": "icon-view",
        **dict.fromkeys(HIDDEN_BARS.values(), False),
        "include_icon_view_settings": True,
        "include_list_view_settings": False,
        "arrange_by": None,
        "icon_size": ICON_SIZE,
        "text_size": 14,
        "label_pos": "bottom",
        "icon_locations": ICONS,
    })


def verify(volume):
    if not (volume / ".background.tiff").is_file():
        raise SystemExit("Missing Finder background in the DMG.")
    with DSStore.open(str(volume / ".DS_Store"), "r") as store:
        window = store["."]["bwsp"]
        (x, y), (width, height) = WINDOW
        bounds = f"{{{{{x}, {y}}}, {{{width}, {height}}}}}"
        if window["WindowBounds"] != bounds or any(window[key] for key in HIDDEN_BARS):
            raise SystemExit("The DMG has unexpected Finder window settings.")
        view = store["."]["icvp"]
        if view["backgroundType"] != 2 or not view["backgroundImageAlias"]:
            raise SystemExit("The DMG does not point to its Finder background.")
        if view["iconSize"] != ICON_SIZE or view["arrangeBy"] != "none":
            raise SystemExit("The DMG has unexpected Finder icon settings.")
        for name, position in ICONS.items():
            if store[name]["Iloc"] != position:
                raise SystemExit(f"Unexpected Finder position for {name}.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build")
    build_parser.add_argument("app", type=Path)
    build_parser.add_argument("background", type=Path)
    build_parser.add_argument("output", type=Path)
    verify_parser = commands.add_parser("verify")
    verify_parser.add_argument("volume", type=Path)
    args = parser.parse_args()
    if args.command == "build":
        build(args.app, args.background, args.output)
    else:
        verify(args.volume)


if __name__ == "__main__":
    main()

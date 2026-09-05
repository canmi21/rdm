# The installer's window, read by dmgbuild, which writes it into the volume's .DS_Store without
# Finder's help: a 16:9 window whose content is background.svg rendered at 2x, the .app on the
# left and an Applications shortcut on the right. The .app and the rendered background are
# named with -D by pkgs/package.sh. The window is 100 taller than the background: Finder keeps
# its bar above and its bar below the content. An icon slot of 149 draws the app's tile 120 wide,
# since the tile fills 824 of its 1024, so the tile's edges sit on the background's grid. See
# spec/packaging.md.
app = defines["app"]
name = app.rsplit("/", 1)[-1]

format = "UDZO"
files = [app]
symlinks = {"Applications": "/Applications"}
background = defines["background"]
window_rect = ((200, 200), (640, 460))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
sidebar_width = 0
icon_size = 149
text_size = 13
icon_locations = {name: (160, 160), "Applications": (480, 160)}

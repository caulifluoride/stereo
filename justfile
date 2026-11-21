prefix := env_var_or_default("PREFIX", env_var("HOME") / ".local")

# build and install the binary, desktop entry and icon
install:
    cargo install --path . --locked --root {{prefix}}
    install -Dm644 stereo.desktop {{prefix}}/share/applications/stereo.desktop
    install -Dm644 assets/stereo.svg {{prefix}}/share/icons/hicolor/scalable/apps/stereo.svg
    -update-desktop-database {{prefix}}/share/applications
    -gtk-update-icon-cache -q {{prefix}}/share/icons/hicolor

uninstall:
    cargo uninstall --root {{prefix}} stereo
    rm -f {{prefix}}/share/applications/stereo.desktop {{prefix}}/share/icons/hicolor/scalable/apps/stereo.svg
    -update-desktop-database {{prefix}}/share/applications

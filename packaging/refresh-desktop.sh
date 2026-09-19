#!/bin/sh
# Cache refreshes are optional and must not prevent installation or removal.
if command -v gtk-update-icon-cache >/dev/null && [ -d /usr/share/icons/hicolor ]; then
    gtk-update-icon-cache -q -f -t /usr/share/icons/hicolor || true
fi
if command -v update-desktop-database >/dev/null && [ -d /usr/share/applications ]; then
    update-desktop-database -q /usr/share/applications || true
fi
if command -v update-mime-database >/dev/null && [ -d /usr/share/mime ]; then
    update-mime-database /usr/share/mime || true
fi
exit 0

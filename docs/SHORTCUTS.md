# Keyboard and pointer controls

| Action | Shortcut |
| --- | --- |
| New / Open / Save | Ctrl+N / Ctrl+O / Ctrl+S |
| Import as layer / Save As | Ctrl+Shift+O / Ctrl+Shift+S |
| Export | Ctrl+Alt+Shift+S |
| Close project | Ctrl+W |
| Undo / Redo | Ctrl+Z / Ctrl+Shift+Z (or Ctrl+Y) |
| Duplicate / Merge / Group | Ctrl+J / Ctrl+E / Ctrl+G |
| Ungroup / Clipping mask | Ctrl+Shift+G / Ctrl+Alt+G |
| New layer | Ctrl+Shift+N |
| Select all / Deselect / Invert selection | Ctrl+A / Ctrl+D / Ctrl+Shift+I |
| Cut / Copy / Paste / Copy Merged | Ctrl+X / Ctrl+C / Ctrl+V / Ctrl+Shift+C |
| Foreground / Background fill | Alt+Backspace / Ctrl+Backspace |
| Content-aware fill | Shift+F5 |
| Levels / Hue-Saturation / Curves | Ctrl+L / Ctrl+U / Ctrl+M |
| Invert pixels or mask | Ctrl+I |
| Fit / Actual pixels | Ctrl+0 / Ctrl+1 |
| Zoom in / out | Ctrl+Plus / Ctrl+Minus, or mouse wheel |
| Show transform / Hide controls | Ctrl+T / Ctrl+H |
| Move / Marquee / Lasso / Wand / Crop | V / M / L / W / C |
| Brush / Eraser / Heal / Clone / Blur | B / E / J / S / R |
| Gradient / Shape / Eyedropper / Hand / Zoom | G / U / I / H / Z |
| Text / Apply text / Cancel text | T / Ctrl+Enter / Escape |
| Brush size / Hardness | [ and ] / Shift+[ and Shift+] |
| Opacity | Number keys 1–9, 0 for 100% |
| Swap / Reset colors | X / D |
| Nudge / Larger nudge | Arrow keys / Shift+arrow keys |
| Pan | Space-drag or middle-button drag |
| Pan horizontally | Horizontal mouse wheel or Shift+wheel over the canvas |
| Adjust slider or number | Wheel up / down over the control (increase / decrease) |
| Apply crop or polygon / Cancel | Enter / Escape |
| Shortcut reference | F1 |

Copy an image in another app, or copy one or more image files in a file manager, then use Ctrl+V (or Edit → Paste) to add them as layers. External images are centered on the canvas; a new document is created if none is open. Local file URLs and absolute file paths can also be pasted. Multiple files are imported together in one undo step, without changing the source files. When a text field has focus, Ctrl+V pastes text into that field.

Use **File → Open Image from Clipboard** to open copied pixels in a new document sized to the image, even when another document is open. Copied image files open in separate tabs. Clipboard images preserve transparency and prompt to save when closed.

For a marquee selection, Ctrl+C copies the active layer's selected pixels. If no layer is active, it copies the visible canvas within the selection. Ctrl+V places those pixels on a new layer at their original position. Ctrl+Shift+C always copies the visible composite; Ctrl+X requires an active layer. Successful copies show the copied dimensions in the status bar.

Shift with a selection adds coverage, Alt subtracts, and Shift+Alt intersects. Drag inside a selection to move its outline; hold Ctrl to move selected pixels, or Ctrl+Alt to duplicate them. The contextual header also offers explicit selection modes.

Move handles scale the selected layers, the circular handle rotates them, and Ctrl-dragging a corner applies perspective distortion. Shift constrains movement or rotation; the Link control toggles the size ratio. Alt-drag duplicates a layer. The mask thumbnail targets the mask for painting and transformations. Its context menu controls linking and visibility.

Alt-click sets a Clone Stamp source. Shift-click continues a straight brush line. Clone alignment and sampling are configured in the contextual header. Use the same header to switch marquee/lasso/shape variants and linear/radial gradients.

With the Text tool, click the canvas to add text or click existing text to edit it. The text dialog provides multiline input, a searchable list of installed font families, pixel size, color, bold, italic, underline, and strikethrough. Enter starts a new line; Ctrl+Enter applies the preview as one undo step; Escape cancels. Double-click a text layer or choose “Edit text…” from its context menu to reopen it. Use Move to position, scale, or rotate text. Painting or applying pixel filters converts a text layer to pixels; undo restores its text settings.

Each font in the selector previews its own typeface. While the selector is open, Up/Down moves through the filtered fonts and updates the canvas preview. Enter or Escape closes the selector while keeping the preview; Escape again cancels the text edit.

With the Move tool, click visible layer content to select it, or click empty canvas or the surrounding workspace to deselect. Shift-click toggles layers in the selection. Auto Select is enabled by default; turn it off to keep the current selection while moving, or hold Ctrl to select from the canvas temporarily.

Drag a layer row or thumbnail to reorder it. Drop on the upper or lower half of a row to place it above or below that layer; drop in the center of a folder row to nest it. The highlighted line or outline marks the destination. Alt-drag duplicates it. Drop onto another project tab to copy the layer and its descendants. “Move Out of Group” is in the row's context menu. Shift/Ctrl-click layer rows toggles multi-selection.

NEF/NRW files enter RAW Develop before becoming layers. In Develop, Ctrl+Z / Ctrl+Shift+Z undo and redo RAW settings; Escape exits the white-balance picker or mask drawing. Drag pans, wheel zooms, and the Fit / 100% buttons set the inspection scale. In Split view, drag near the comparison divider to move it; drag elsewhere or Alt-drag to pan. Space-drag or middle-button drag pans in every view, including with a picker or mask tool active. Side by side zooms both images around their pane centers and pans them together. Double-click a RAW layer (or its image with Move selected) to reopen Develop. Use Develop to commit, or Cancel to retain the previous layer state.

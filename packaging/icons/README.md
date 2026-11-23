# Tanis Terminal icon

`tterm.svg` is the master: the black rounded square with the indigo prompt and the white
underscore of the design's board G. As ted's since 0.7.0, the square fills the whole canvas with
Postino's corner radius and the symbol is scaled to match, so the apps look the same size in a
dock or a taskbar. It is the same file as `crates/tterm-app/assets/tterm.svg`.

The PNG sizes under `png/` and the Windows icon are generated from it and committed:

```
for s in 16 24 32 48 64 128 256 512 1024; do
  rsvg-convert -w $s -h $s tterm.svg -o png/tterm-$s.png
done
magick png/tterm-{16,24,32,48,64,128,256}.png ../../crates/tterm-app/assets/tterm.ico
```

`crates/tterm-app/assets/tterm.rc` embeds the `.ico` in `tterm.exe` (`build.rs`): the window,
the taskbar and Explorer use it. The macOS `.icns` is built from the PNGs by
`packaging/macos/bundle.sh`; the Linux packages install the SVG and the PNGs (`packaging/linux`).

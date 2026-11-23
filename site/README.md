# Tanis Terminal website

The website at https://terminal.tanis.codes, built from ted's (itself built from Postino's): plain
HTML, CSS and JavaScript, no build step except the documentation. The `Website` workflow
(`.github/workflows/website.yml`) publishes it to GitHub Pages on every push to `main` that
touches `site/`, and after every stable release.

- `index.html` and `install.html`: the home and download pages, written by hand.
- `install.sh`: the Linux install script, served at `/install.sh` and shipped in the tarball.
- `docs-src/`: the sources of the user documentation. `python3 site/docs-src/build.py` (or
  `make docs`) wraps every page of `docs-src/pages/` in `template.html` and writes `docs/`, which
  is generated and git ignored, plus `sitemap.xml` and `llms.txt`. It fails on a broken link or
  anchor and on an em dash. Every user-visible change updates these pages in the same commit.
- `img/`: the screenshots, light and dark, taken from the real app at 1800 x 1125 and saved as
  WebP, with made-up usage data and a neutral prompt; `logo.svg`, `icon-512.png` and `og.png`.
- `css/`, `js/` and `fonts/`: ted's.

To preview it, build the documentation and serve the folder:

```
make docs
python3 -m http.server -d site 8000
```

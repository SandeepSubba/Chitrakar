# Benches

Two ways of timing the editor in headless Chromium, which is where the
numbers in `docs/PLAN.md` §0 come from. Both use the Playwright that
`app/` already brings (`npm install` there first) and Chromium wherever
Playwright finds it, or `CHITRAKAR_CHROMIUM=/path/to/chromium`.

## The engine alone: `engine/run.mjs`

A 12-megapixel photograph fitted to a 1170×878 window, then each layer
of `engine/layers.json` — every adjustment and filter — added over it in
turn and the frame timed: the least of eight frames, less the
photograph alone. Through the WASM engine only, with no page and no
React, so what it measures is rendering.

```sh
cd app && npm run build:wasm            # app/src/wasm-pkg
node tools/bench/engine/run.mjs                       # every layer
node tools/bench/engine/run.mjs --only "radial"       # one
node tools/bench/engine/run.mjs --prof --only median  # where the time went
node tools/bench/engine/run.mjs /some/other/wasm-pkg  # an A/B against another build
```

With `--io` it times a photograph on its way in and out instead
(`engine/io.html`): a 12-megapixel PNG placed, the document saved with
its picture encoded and with it kept, the draft's skeleton, and a
24-megapixel raw mosaic (made on the page, packed as the raw reader
packs one) opened, previewed and developed in full — as Bayer, and
developed again as Fuji's X-Trans, which has a demosaic of its own,
and once more with a lens's corrections (straightened, its colour
fringes closed, its corners lifted) as a Fuji's RAF carries them.

```sh
node tools/bench/engine/run.mjs --io
node tools/bench/engine/run.mjs --io /some/other/wasm-pkg
```

The frame is invalidated before it is timed: a view moved by whole
pixels at the same scale is a pan, which the engine draws from the
frame before it, and would be timed as one.

## The editor as it is used: `app/gesture.mjs`

The page, React and the engine together, driven through a drag or a run
of wheel zooms: how long a step takes, and how many steps were drafts.
Over the same photograph, or over a `.chitra` file given on the command
line. Against the dev server for a quick look — but it carries React in
development mode, a large share of a step — or the production build for
the numbers.

```sh
cd app && npm run dev                   # http://localhost:5173/
node tools/bench/app/gesture.mjs http://localhost:5173/ photo
GESTURE=wheel node tools/bench/app/gesture.mjs http://localhost:5173/ some/page.chitra

cd app && npm run build                 # the production build
node tools/bench/app/serve-dist.mjs &   # http://localhost:8800/
THROTTLE=4 node tools/bench/app/gesture.mjs http://localhost:8800/ photo   # at a phone's speed
PROF=1 node tools/bench/app/gesture.mjs http://localhost:8800/ photo       # where the time went
```

A step's time includes two frames of waiting for the browser and the
round trip of the pointer event, some 40–50 ms unthrottled, so read the
numbers against each other rather than as the frame's own cost; `PROF=1`
gives that, by self time.

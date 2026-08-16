/**
 * Renders public/icons/icon.svg into a 512×512 maskable icon: the mark scaled
 * to 60% and centred on the manifest's background colour, so the OS's circular /
 * squircle mask (which may crop up to 20% on each side) never clips it.
 *
 *   node scripts/generate-maskable-icon.mjs
 *
 * Uses the Chromium that @playwright/test already installs, like
 * generate-icons.mjs. The PNG is committed so a plain build needs no browser.
 */
import { chromium } from '@playwright/test'
import { readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const ICON_DIR = join(ROOT, 'public', 'icons')
const SIZE = 512
const INNER = Math.round(SIZE * 0.6)
const PAD = (SIZE - INNER) / 2
const BACKGROUND = '#ffffff' // manifest background_color

const svg = await readFile(join(ICON_DIR, 'icon.svg'), 'utf8')
const browser = await chromium.launch()
try {
  const page = await browser.newPage({ viewport: { width: SIZE, height: SIZE }, deviceScaleFactor: 1 })
  await page.setContent(
    `<!doctype html><style>
       html,body{margin:0;padding:0;width:${SIZE}px;height:${SIZE}px;background:${BACKGROUND}}
       .wrap{position:absolute;left:${PAD}px;top:${PAD}px;width:${INNER}px;height:${INNER}px}
       svg{display:block;width:${INNER}px;height:${INNER}px}
     </style><div class="wrap">${svg}</div>`,
  )
  await writeFile(join(ICON_DIR, 'icon-maskable-512.png'), await page.screenshot())
  console.log('icon-maskable-512.png')
} finally {
  await browser.close()
}

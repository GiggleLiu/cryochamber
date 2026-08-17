/**
 * Renders public/icons/icon.svg to the PNG sizes the manifest and iOS need.
 *
 *   node scripts/generate-icons.mjs
 *
 * Uses the Chromium that @playwright/test already installs, so this adds no
 * dependency. Re-run it whenever icon.svg changes; the PNGs are committed so a
 * plain `npm run build` never needs a browser.
 */
import { chromium } from '@playwright/test'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..')
const ICON_DIR = join(ROOT, 'public', 'icons')
const SIZES = [180, 192, 512]

const svg = await readFile(join(ICON_DIR, 'icon.svg'), 'utf8')
await mkdir(ICON_DIR, { recursive: true })

const browser = await chromium.launch()
try {
  for (const size of SIZES) {
    const page = await browser.newPage({
      viewport: { width: size, height: size },
      deviceScaleFactor: 1,
    })
    await page.setContent(
      `<!doctype html><style>html,body{margin:0;padding:0;width:${size}px;height:${size}px}
       svg{display:block;width:${size}px;height:${size}px}</style>${svg}`,
    )
    await writeFile(join(ICON_DIR, `icon-${size}.png`), await page.screenshot())
    await page.close()
    console.log(`icon-${size}.png`)
  }
} finally {
  await browser.close()
}

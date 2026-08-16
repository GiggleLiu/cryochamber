import { IMAGE_EXT_RE, deferHubImages, inlineImageLinks } from './images'

test('IMAGE_EXT_RE recognises picture extensions, case-insensitively', () => {
  for (const name of ['a.png', 'a.JPG', 'a.jpeg', 'b.gif', 'c.webp', 'd.svg', 'e.avif', 'f.bmp', 'g.ico']) {
    expect(IMAGE_EXT_RE.test(name)).toBe(true)
  }
  for (const name of ['a.pdf', 'a.png.txt', 'notes', 'a.mp4']) {
    expect(IMAGE_EXT_RE.test(name)).toBe(false)
  }
})

test('an attachment link to an image becomes a thumbnail inside the same anchor', () => {
  const html = '<p><a href="/api/chambers/x/files/artwork.png">artwork.png</a></p>'
  const out = inlineImageLinks(html)
  expect(out).toContain('<img')
  expect(out).toContain('src="/api/chambers/x/files/artwork.png"')
  expect(out).toContain('alt="artwork.png"')
  expect(out).toContain('class="msg-thumb"')
  // The anchor survives — the click handler keys off it for the lightbox.
  expect(out).toContain('<a href="/api/chambers/x/files/artwork.png">')
})

test('non-image attachments and non-attachment links are left alone', () => {
  const pdf = '<p><a href="/api/chambers/x/files/report.pdf">report.pdf</a></p>'
  expect(inlineImageLinks(pdf)).toBe(pdf)
  const external = '<p><a href="https://example.com/cat.png">cat.png</a></p>'
  expect(inlineImageLinks(external)).toBe(external)
})

test('an anchor that already wraps an image is untouched', () => {
  const html =
    '<p><a href="/api/chambers/x/files/plot.png"><img src="/api/chambers/x/files/plot.png" alt="plot"></a></p>'
  expect(inlineImageLinks(html)).toBe(html)
})

test('the link text becomes the alt text when it differs from the filename', () => {
  const out = inlineImageLinks('<a href="/api/chambers/x/files/a_b_plot.png">the plot</a>')
  expect(out).toContain('alt="the plot"')
})

test('a percent-encoded filename is decoded for the extension check', () => {
  const out = inlineImageLinks('<a href="/api/chambers/x/files/my%20photo.png">my photo.png</a>')
  expect(out).toContain('<img')
})

describe('deferHubImages', () => {
  test('parks a hub image src in data-upload-src and drops src', () => {
    const out = deferHubImages('<p><img src="/api/chambers/c/files/a.png" alt="a"></p>')
    expect(out).toContain('data-upload-src="/api/chambers/c/files/a.png"')
    expect(out).not.toMatch(/ src=/)
    expect(out).toContain('alt="a"')
  })

  test('leaves non-hub images and image-free html untouched', () => {
    const plain = '<p><img src="/static/logo.png" alt="logo"></p>'
    expect(deferHubImages(plain)).toBe(plain)
    expect(deferHubImages('<p>no pictures</p>')).toBe('<p>no pictures</p>')
  })

  test('composes with inlineImageLinks: the thumbnail is deferred too', () => {
    const out = deferHubImages(
      inlineImageLinks('<a href="/api/chambers/c/files/a.png">a.png</a>'),
    )
    expect(out).toContain('class="msg-thumb"')
    expect(out).toContain('data-upload-src="/api/chambers/c/files/a.png"')
    expect(out).not.toMatch(/<img[^>]* src=/)
  })
})

import {
  initial,
  listTimeLabel,
  previewText,
  relativeTimeLabel,
  separatorLabel,
  tileColor,
} from './format'

// 2026-08-15 14:32 local time.
const NOW = new Date(2026, 7, 15, 14, 32)
const at = (d: Date): number => Math.floor(d.getTime() / 1000)

describe('separatorLabel', () => {
  test('names today when there is no previous message to compare against', () => {
    expect(separatorLabel(at(new Date(2026, 7, 15, 9, 5)), NOW)).toBe('Today 09:05')
  })

  test('drops to a bare time when the previous message is the same day', () => {
    const prev = at(new Date(2026, 7, 15, 8, 0))
    expect(separatorLabel(at(new Date(2026, 7, 15, 9, 5)), NOW, prev)).toBe('09:05')
  })

  test('still names the day when the previous message was the day before', () => {
    const prev = at(new Date(2026, 7, 14, 23, 50))
    expect(separatorLabel(at(new Date(2026, 7, 15, 0, 10)), NOW, prev)).toBe('Today 00:10')
  })

  test('the previous calendar day is named, not dated', () => {
    expect(separatorLabel(at(new Date(2026, 7, 14, 19, 32)), NOW)).toBe('Yesterday 19:32')
  })

  test('inside the past week uses the weekday', () => {
    // 2026-08-11 is a Tuesday.
    expect(separatorLabel(at(new Date(2026, 7, 11, 8, 0)), NOW)).toBe('Tue 08:00')
  })

  test('older in the same year uses day and month', () => {
    expect(separatorLabel(at(new Date(2026, 2, 3, 23, 59)), NOW)).toBe('3 Mar 23:59')
  })

  test('a different year carries the year', () => {
    expect(separatorLabel(at(new Date(2025, 7, 14, 19, 32)), NOW)).toBe('14 Aug 2025 19:32')
  })

  test('crossing midnight counts as a different day even minutes apart', () => {
    expect(separatorLabel(at(new Date(2026, 7, 14, 23, 58)), NOW)).toBe('Yesterday 23:58')
  })
})

describe('listTimeLabel', () => {
  test.each([
    [new Date(2026, 7, 15, 9, 5), '09:05'],
    [new Date(2026, 7, 14, 19, 32), 'Yesterday'],
    [new Date(2026, 7, 11, 8, 0), 'Tue'],
    [new Date(2026, 2, 3, 23, 59), '3 Mar'],
    [new Date(2025, 7, 14, 19, 32), '14/8/25'],
  ])('%s → %s', (when, expected) => {
    expect(listTimeLabel(at(when), NOW)).toBe(expected)
  })
})

describe('previewText', () => {
  test('flattens rendered markdown to a single line', () => {
    expect(
      previewText('<p>Sweep <strong>done</strong>.</p>\n<ul><li>a</li>\n<li>b</li></ul>'),
    ).toBe('Sweep done. a b')
  })

  test('markup and entities never leak into the preview', () => {
    expect(previewText('<p>a &amp; b</p><script>evil()</script>')).toBe('a & b')
  })

  test('empty content yields an empty string', () => {
    expect(previewText('')).toBe('')
  })
})

describe('tileColor', () => {
  test('is deterministic per key and stays inside the palette', () => {
    expect(tileColor('qec-decoders')).toBe(tileColor('qec-decoders'))
    expect(tileColor('qec-decoders')).toMatch(/^#[0-9a-f]{6}$/)
  })

  test('different keys generally land on different colours', () => {
    const colors = new Set(['alpha', 'beta', 'gamma', 'delta'].map(tileColor))
    expect(colors.size).toBeGreaterThan(1)
  })
})

describe('initial', () => {
  test('uppercases the first character of the name', () => {
    expect(initial('ada lovelace')).toBe('A')
  })

  test('falls back to the second argument when the name is empty', () => {
    expect(initial('', 'bot@b.c')).toBe('B')
  })

  test('with nothing to work from it still renders a character', () => {
    expect(initial('')).toBe('?')
  })
})

describe('relativeTimeLabel', () => {
  const now = new Date('2026-08-15T12:00:00Z')

  test('counts minutes, hours and days back from now', () => {
    expect(relativeTimeLabel('2026-08-15T11:59:30Z', now)).toBe('just now')
    expect(relativeTimeLabel('2026-08-15T11:20:00Z', now)).toBe('40m ago')
    expect(relativeTimeLabel('2026-08-15T05:00:00Z', now)).toBe('7h ago')
    expect(relativeTimeLabel('2026-08-12T12:00:00Z', now)).toBe('3d ago')
  })

  test('falls back to a date once it is more than a month old', () => {
    expect(relativeTimeLabel('2026-06-01T12:00:00Z', now)).toBe('1 Jun')
  })

  test('an unparseable timestamp is shown as-is rather than as NaN', () => {
    expect(relativeTimeLabel('not a date', now)).toBe('not a date')
  })
})

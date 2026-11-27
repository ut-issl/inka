import { describe, expect, test } from 'vitest'

import { neutralize } from './neutralize'

describe('neutralize', () => {
  test('drops control characters, so an escape sequence cannot reach the screen', () => {
    expect(neutralize('safe\u001b[31mred\u0007')).toBe('safe[31mred')
  })

  test('drops bidirectional controls, so text reads in the order that was hashed', () => {
    expect(neutralize('abc‮def⁦ghi⁩')).toBe('abcdefghi')
  })

  test('keeps tabs, line breaks and the joiners emoji are built from', () => {
    expect(neutralize('a\tb\nc')).toBe('a\tb\nc')
    expect(neutralize('👩‍🚀')).toBe('👩‍🚀')
  })

  test('drops a carriage return, which the normalized text never carries', () => {
    expect(neutralize('a\r\nb')).toBe('a\nb')
  })
})

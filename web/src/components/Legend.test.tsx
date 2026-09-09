import { render, screen } from '@testing-library/react'
import { describe, expect, test } from 'vitest'

import { DISCHARGE_MODES } from '../model/elements'
import { Legend } from './Legend'

describe('Legend', () => {
  test('names all four discharge modes', () => {
    render(<Legend />)

    for (const mode of DISCHARGE_MODES) {
      expect(screen.getByText(mode)).toBeInTheDocument()
    }
  })

  test('explains the namespace badge without offering an action', () => {
    render(<Legend />)

    expect(screen.getByText(/coloured badge on each node/)).toBeInTheDocument()
    expect(screen.queryAllByRole('button')).toHaveLength(0)
  })
})

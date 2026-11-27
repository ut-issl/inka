import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { MemoryRouter } from 'react-router'
import { describe, expect, test } from 'vitest'

import { node, snapshot } from '../test/factory'
import { NodeReference } from './NodeReference'

const target = snapshot([
  node('sensor', { mode: 'fiat', namespace: 'pointing', description: 'The star tracker meets **its accuracy**.' }),
]).nodes[0]!

function renderReference(defined = true) {
  render(
    <MemoryRouter>
      <NodeReference node={defined ? target : undefined} id="sensor" to="/review/sensor" />
    </MemoryRouter>,
  )
}

describe('NodeReference', () => {
  test('is a link to the node, readable without any hover', () => {
    renderReference()

    expect(screen.getByRole('link', { name: 'sensor' })).toHaveAttribute('href', '/review/sensor')
    expect(screen.queryByRole('note')).toBeNull()
  })

  test('previews the node’s mode, id, namespace, status and description on hover', async () => {
    const user = userEvent.setup()
    renderReference()

    await user.hover(screen.getByRole('link', { name: 'sensor' }))

    const preview = await screen.findByRole('note', { name: 'Preview of sensor' })
    expect(preview).toHaveTextContent('fiat:sensor')
    expect(preview).toHaveTextContent('pointing')
    expect(preview).toHaveTextContent('blamed')
    expect(preview).toHaveTextContent('The star tracker meets its accuracy.')
    expect(preview.querySelector('strong')).toHaveTextContent('its accuracy')
  })

  test('previews on keyboard focus too', async () => {
    const user = userEvent.setup()
    renderReference()

    await user.tab()

    expect(await screen.findByRole('note', { name: 'Preview of sensor' })).toBeInTheDocument()
  })

  test('a preview offers no decision and no control of its own', async () => {
    const user = userEvent.setup()
    renderReference()

    await user.hover(screen.getByRole('link', { name: 'sensor' }))
    const preview = await screen.findByRole('note')

    expect(preview.querySelector('button, input, textarea, form')).toBeNull()
  })

  test('a reference to a node the argument does not define is still a link, without a preview', async () => {
    const user = userEvent.setup()
    renderReference(false)

    await user.hover(screen.getByRole('link', { name: 'sensor' }))

    expect(screen.queryByRole('note')).toBeNull()
  })
})

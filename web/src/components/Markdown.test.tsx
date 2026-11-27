import { render, screen } from '@testing-library/react'
import { describe, expect, test } from 'vitest'

import { Markdown } from './Markdown'

describe('Markdown', () => {
  test('renders CommonMark: emphasis, code and lists', () => {
    render(<Markdown text={'The margin is **at least** `0.05 deg`:\n\n- one\n- two'} />)

    expect(screen.getByText('at least').tagName).toBe('STRONG')
    expect(screen.getByText('0.05 deg').tagName).toBe('CODE')
    expect(screen.getAllByRole('listitem')).toHaveLength(2)
  })

  test('shows raw HTML as the text it is, never interpreting it', () => {
    const { container } = render(<Markdown text={'before <script>alert(1)</script> <b>bold</b> after'} />)

    expect(container.querySelector('script')).toBeNull()
    expect(container.querySelector('b')).toBeNull()
    expect(container).toHaveTextContent('<script>alert(1)</script>')
    expect(container).toHaveTextContent('<b>bold</b>')
  })

  test('shows a block of raw HTML as text too', () => {
    const { container } = render(<Markdown text={'<div onclick="steal()">\nhidden?\n</div>'} />)

    expect(container.querySelector('[onclick]')).toBeNull()
    expect(container).toHaveTextContent('<div onclick="steal()">')
  })

  test('loads no image: shows its alternative text and its destination', () => {
    const { container } = render(<Markdown text={'![the plot](https://example.com/plot.png)'} />)

    expect(container.querySelector('img')).toBeNull()
    expect(screen.getByText('the plot')).toBeInTheDocument()
    expect(screen.getByText('https://example.com/plot.png')).toBeInTheDocument()
  })

  test('shows where a link leads beside its text', () => {
    render(<Markdown text={'see [the standard](https://example.com/std) for why'} />)

    const link = screen.getByRole('link', { name: 'the standard' })
    expect(link).toHaveAttribute('href', 'https://example.com/std')
    expect(link).toHaveAttribute('rel', expect.stringContaining('noopener'))
    expect(screen.getByText('https://example.com/std')).toBeInTheDocument()
  })

  test('never makes a link out of a script destination, and still shows where it pointed', () => {
    const { container } = render(<Markdown text={'[click](javascript:alert(1))'} />)

    expect(container.querySelector('a')).toBeNull()
    expect(screen.getByText('click')).toBeInTheDocument()
    expect(screen.getByText('javascript:alert(1)')).toBeInTheDocument()
  })

  test('shows a destination that is not followed, such as a local file, as text', () => {
    const { container } = render(<Markdown text={'[report](file:///tmp/report.pdf)'} />)

    expect(container.querySelector('a')).toBeNull()
    expect(screen.getByText('file:///tmp/report.pdf')).toBeInTheDocument()
  })

  test('shows the destination of an image that is not followed, too', () => {
    render(<Markdown text={'![scan](file:///tmp/scan.png)'} />)

    expect(screen.getByText('file:///tmp/scan.png')).toBeInTheDocument()
  })

  test('shows the title of a link and of an image as visible text', () => {
    const { container } = render(
      <Markdown
        text={'[report](https://example.com/report "draft, not approved") ![plot](https://example.com/p.png "run 3")'}
      />,
    )

    expect(screen.getByText('“draft, not approved”')).toBeInTheDocument()
    expect(screen.getByText('“run 3”')).toBeInTheDocument()
    expect(container.querySelector('[title]')).toBeNull()
  })

  test('neutralizes controls written as character references, which parsing turns into the characters', () => {
    const { container } = render(<Markdown text={'abc&#x202e;def&#x202c; [x&#x202e;y](https://example.com "t&#x202e;u")'} />)

    expect(container.textContent).not.toMatch(/[\u202A-\u202E\u2066-\u2069]/)
    expect(container.textContent).toContain('abcdef')
    expect(screen.getByRole('link')).toHaveTextContent('xy')
    expect(screen.getByText('“tu”')).toBeInTheDocument()
  })

  test('neutralizes control and bidirectional characters before rendering', () => {
    const { container } = render(<Markdown text={'abc‮def\u001b[31m'} />)

    expect(container.textContent).toBe('abcdef[31m')
  })
})

import { expect, test, type Page } from '@playwright/test'

/**
 * The graph surface against the `aocs` fixture, in a real browser.
 *
 * The cards are DOM elements, so what is asserted is geometry — where the
 * cards actually are — never pixels: no two overlap, and a child is below its
 * parent, whatever the fonts of the machine make of the text.
 */

interface Box {
  readonly id: string
  readonly left: number
  readonly top: number
  readonly right: number
  readonly bottom: number
}

/** Opens the graph and waits until the cards have been laid out. */
async function open(page: Page): Promise<void> {
  await page.goto('/#/graph')
  await settled(page)
}

/** Waits until every card on the canvas has been placed and has stopped moving. */
async function settled(page: Page): Promise<void> {
  await expect(page.locator('.card').first()).toBeVisible()
  await expect
    .poll(async () => page.locator('.react-flow__node').evaluateAll((nodes) => nodes.every((node) => getComputedStyle(node).opacity === '1')))
    .toBe(true)
  await page.waitForTimeout(500)
}

async function boxes(page: Page): Promise<Box[]> {
  return page.locator('.card').evaluateAll((cards) =>
    cards.map((card) => {
      const rect = card.getBoundingClientRect()
      return { id: (card as HTMLElement).dataset.id ?? '', left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom }
    }),
  )
}

function expectNoOverlap(all: readonly Box[]): void {
  for (const [index, a] of all.entries()) {
    for (const b of all.slice(index + 1)) {
      const apart = a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top
      expect(apart, `${a.id} and ${b.id} overlap`).toBe(true)
    }
  }
}

const card = (page: Page, id: string) => page.locator(`.card[data-id="${id}"]`)

test('opens collapsed: the objective and its direct children, as cards that say what they are', async ({ page }) => {
  await open(page)

  await expect(page.locator('.card')).toHaveCount(5)
  await expect(page.getByText('5 of 17 nodes shown')).toBeVisible()
  const root = card(page, 'aocs_ok')
  await expect(root.locator('.card__mode')).toHaveText('objective')
  await expect(root.locator('.card__id')).toHaveText('aocs_ok')
  await expect(root.locator('.pill')).toBeVisible()
  await expect(root.locator('.card__description')).not.toBeEmpty()
})

test('no two cards overlap, and every child is below its parent', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Expand all' }).click()
  await expect(page.locator('.card')).toHaveCount(17)
  await settled(page)

  const all = await boxes(page)
  expectNoOverlap(all)

  const byId = new Map(all.map((box) => [box.id, box]))
  const edges = await page.locator('.react-flow__edge').evaluateAll((found) =>
    found.map((edge) => (edge.getAttribute('data-id') ?? '').split('->')),
  )
  expect(edges.length).toBeGreaterThan(0)
  for (const [parent, child] of edges) {
    expect(byId.get(child as string)!.top, `${child} is below ${parent}`).toBeGreaterThanOrEqual(
      byId.get(parent as string)!.bottom,
    )
  }
})

test('every card has the same width, so its height comes from its text alone', async ({ page }) => {
  await open(page)

  const widths = (await boxes(page)).map((box) => Math.round(box.right - box.left))

  expect(new Set(widths).size).toBe(1)
})

test('a collapsed node says how much lies below it; expanding shows its direct children only', async ({ page }) => {
  await open(page)
  const toggle = card(page, 'safe_mode_ok').locator('.card__toggle')
  await expect(toggle).toHaveText(/^Expand · \d+ nodes? below( · \d+ blamed)?$/)
  await expect(toggle).toHaveAttribute('aria-expanded', 'false')

  await toggle.click()
  await settled(page)

  await expect(toggle).toHaveText('Collapse')
  await expect(toggle).toHaveAttribute('aria-expanded', 'true')
  expect(await page.locator('.card').count()).toBeGreaterThan(5)
  expectNoOverlap(await boxes(page))

  await toggle.click()
  await expect(page.locator('.card')).toHaveCount(5)
})

test('a node with several parents appears once, with an edge from each', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Expand all' }).click()
  await settled(page)

  await expect(card(page, 'actuator_authority_ok')).toHaveCount(1)
  await expect(page.locator('.react-flow__edge[data-id="momentum_ok->actuator_authority_ok"]')).toHaveCount(1)
  await expect(page.locator('.react-flow__edge[data-id="pointing_ok->actuator_authority_ok"]')).toHaveCount(1)
})

test('the whole graph expands and collapses again in one action', async ({ page }) => {
  await open(page)

  await page.getByRole('button', { name: 'Expand all' }).click()
  await expect(page.getByText('17 of 17 nodes shown')).toBeVisible()

  await page.getByRole('button', { name: 'Collapse all' }).click()
  await expect(page.getByText('5 of 17 nodes shown')).toBeVisible()
})

test('the four modes are named on the cards and told apart', async ({ page }) => {
  await open(page)
  await page.getByRole('button', { name: 'Expand all' }).click()
  await settled(page)

  const looks = await page.locator('.card').evaluateAll((cards) =>
    cards.map((element) => {
      const style = getComputedStyle(element)
      return {
        mode: element.querySelector('.card__mode')?.textContent ?? '',
        look: `${style.borderTopColor}|${style.borderLeftStyle}|${style.borderTopLeftRadius}`,
      }
    }),
  )
  const lookOf = new Map(looks.map(({ mode, look }) => [mode, look]))

  expect([...lookOf.keys()].sort()).toEqual(['bind', 'fiat', 'objective', 'structure'])
  expect(new Set(lookOf.values()).size).toBe(4)
})

test('a card records nothing: the graph offers no decision and sends no write', async ({ page }) => {
  const writes: string[] = []
  page.on('request', (request) => {
    if (request.method() !== 'GET') writes.push(request.method())
  })
  await open(page)

  await card(page, 'safe_mode_ok').locator('.card__toggle').click()
  await page.getByRole('button', { name: 'Expand all' }).click()
  await settled(page)

  await expect(page.getByRole('button', { name: /approve|decline/i })).toHaveCount(0)
  await expect(page.locator('input, textarea, form')).toHaveCount(0)
  expect(writes).toEqual([])
})

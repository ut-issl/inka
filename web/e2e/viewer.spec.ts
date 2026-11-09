import { expect, test, type Page } from '@playwright/test'

/**
 * The viewer against the `aocs` fixture, in a real browser.
 *
 * Each test states one Web UI invariant that only a browser can show.
 */

/** Waits until the graph has been laid out and the fit animation has settled. */
async function ready(page: Page): Promise<void> {
  await page.goto('/')
  await expect(page.locator('.node-list__item').first()).toBeVisible()
  await expect(page.locator('.graph__canvas canvas').first()).toBeVisible()
  await page.waitForTimeout(1200)
}

/**
 * Where a graph element currently is, in rendered coordinates: a node's
 * centre or an edge's midpoint, as the graph has it laid out right now.
 */
async function centre(page: Page, id: string) {
  return page.evaluate(
    (elementId) => {
      type Point = { x: number; y: number }
      const cy = (document.querySelector('.graph__canvas') as HTMLElement & { _cyreg?: { cy: unknown } })
        ._cyreg?.cy as {
        getElementById: (id: string) => {
          isEdge: () => boolean
          renderedPosition: () => Point
          renderedMidpoint: () => Point
          visible: () => boolean
        }
      }
      const element = cy.getElementById(elementId)
      const at = element.isEdge() ? element.renderedMidpoint() : element.renderedPosition()
      return { ...at, visible: element.visible() }
    },
    id,
  )
}

test('the whole view is a top-down DAG with the objective on top', async ({ page }) => {
  await ready(page)

  const objective = await centre(page, 'aocs_ok')
  const structure = await centre(page, 'pointing_ok')
  const leaf = await centre(page, 'control_law_ok')

  expect(objective.y).toBeLessThan(structure.y)
  expect(structure.y).toBeLessThan(leaf.y)
  await expect(page.locator('.detail__title')).toHaveText('Whole view')
})

test('the laid-out graph fills the graph window', async ({ page }) => {
  await ready(page)

  const coverage = await page.evaluate(() => {
    const container = document.querySelector('.graph__canvas') as HTMLElement
    const cy = (container as HTMLElement & { _cyreg?: { cy: unknown } })._cyreg?.cy as {
      elements: () => { renderedBoundingBox: () => { w: number; h: number } }
    }
    const box = cy.elements().renderedBoundingBox()
    return { width: box.w / container.clientWidth, height: box.h / container.clientHeight }
  })

  // Both axes are filled: matching the window's aspect ratio is what makes the
  // fit reach the edges in both directions rather than only one.
  expect(coverage.width).toBeGreaterThan(0.8)
  expect(coverage.height).toBeGreaterThan(0.8)
})

test('a node focus shows parents above, the node in the middle and children below', async ({ page }) => {
  await ready(page)

  await page.click('.node-list__item:has-text("safe_mode_ok")')
  await page.waitForTimeout(900)

  const parent = await centre(page, 'aocs_ok')
  const focused = await centre(page, 'safe_mode_ok')
  const child = await centre(page, 'sun_pointing_stable')
  expect(parent.y).toBeLessThan(focused.y)
  expect(focused.y).toBeLessThan(child.y)

  // Only the node and its one-hop neighbours are on screen.
  expect((await centre(page, 'pointing_ok')).visible).toBe(false)
  await expect(page.locator('.detail__title')).toHaveText('safe_mode_ok')
})

test('the detail panel shows the contract, the evidence target and the deadline', async ({ page }) => {
  await ready(page)

  await page.click('.node-list__item:has-text("mass_properties_valid")')

  await expect(page.locator('.detail')).toContainText('Contract')
  await expect(page.locator('.detail')).toContainText('Until FM')
  await expect(page.locator('.detail')).toContainText('Promote to bind')
  // The comment on `until:` sits with the deadline it explains.
  await expect(page.locator('.detail__row--deadline .detail__comment')).toContainText(
    'once the mass-properties test has measured the real vehicle',
  )

  await page.click('.node-list__item:has-text("detumble_convergence_ok")')
  await expect(page.locator('.detail')).toContainText('aocs.verify_detumble')
})

test('namespace badges appear once zoomed in and focus their namespace', async ({ page }) => {
  await ready(page)

  // The whole view of this argument sits below the badge zoom threshold.
  await expect(page.locator('.badge:visible')).toHaveCount(0)

  await page.click('.node-list__item:has-text("actuator_authority_ok")')
  await page.waitForTimeout(900)
  const badges = page.locator('.badge:visible')
  await expect(badges.first()).toBeVisible()

  await badges.filter({ hasText: 'momentum' }).first().click()
  await page.waitForTimeout(900)
  await expect(page.locator('.detail__title')).toHaveText('momentum')
  await expect(page.locator('.detail')).toContainText('one hop away')
})

test('a namespace focus keeps its members plus everything one hop away', async ({ page }) => {
  await ready(page)

  await page.click('.node-list__header:has-text("safety")')
  await page.waitForTimeout(900)

  expect((await centre(page, 'detumble_ok')).visible).toBe(true)
  expect((await centre(page, 'aocs_ok')).visible).toBe(true)
  expect((await centre(page, 'control_law_ok')).visible).toBe(false)
})

test('hovering a node reveals its full description', async ({ page }) => {
  await ready(page)

  await page.click('.node-list__item:has-text("detumble_ok")')
  await page.waitForTimeout(900)
  const box = (await page.locator('.graph__canvas').boundingBox()) as { x: number; y: number; width: number; height: number }
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2)

  const tooltip = page.locator('.tooltip')
  await expect(tooltip).toBeVisible()
  await expect(tooltip).toContainText('detumbling')
})

test('hovering a commented children entry reveals its comment', async ({ page }) => {
  await ready(page)

  await page.click('.node-list__item:has-text("pointing_ok")')
  const box = (await page.locator('.graph__canvas').boundingBox()) as { x: number; y: number }
  const tooltip = page.locator('.tooltip')

  // Re-aim until the focus animation has settled and the pointer is on the
  // edge, rather than trusting one sleep to be long enough.
  await expect(async () => {
    const midpoint = await centre(page, 'pointing_ok->mass_properties_valid')
    await page.mouse.move(box.x + midpoint.x, box.y + midpoint.y)
    await expect(tooltip).toContainText('pointing_ok → mass_properties_valid', { timeout: 500 })
  }).toPass()
  await expect(tooltip).toContainText('still a fiat until the mass-properties test')

  // The same comment is readable without a pointer, under the child.
  await expect(page.locator('.detail__children li:has-text("mass_properties_valid")')).toContainText(
    'still a fiat until the mass-properties test',
  )
})

test('Esc and a background tap both restore the whole view', async ({ page }) => {
  await ready(page)

  await page.click('.node-list__item:has-text("safe_mode_ok")')
  await expect(page.locator('.detail__title')).toHaveText('safe_mode_ok')
  await page.keyboard.press('Escape')
  await expect(page.locator('.detail__title')).toHaveText('Whole view')

  await page.click('.node-list__header:has-text("safety")')
  await expect(page.locator('.detail__title')).toHaveText('safety')
  const box = (await page.locator('.graph__canvas').boundingBox()) as { x: number; y: number; width: number; height: number }
  await page.mouse.click(box.x + box.width - 20, box.y + 20)
  await expect(page.locator('.detail__title')).toHaveText('Whole view')
})

test('nothing in the viewer can change the argument', async ({ page }) => {
  await ready(page)

  await page.click('.node-list__item:has-text("safe_mode_ok")')

  await expect(page.locator('a')).toHaveCount(0)
  await expect(page.locator('input, textarea, form')).toHaveCount(0)
  // Every request the page makes is a read of the argument bundle.
  const methods: string[] = []
  page.on('request', (request) => methods.push(request.method()))
  await page.keyboard.press('Escape')
  await page.waitForTimeout(300)
  expect(methods.every((method) => method === 'GET')).toBe(true)
})

import { expect, test } from '@playwright/test'

/**
 * The review surface against the `aocs` fixture, in a real browser: the queue
 * `inka review --list` prints for it, walked one local view at a time.
 */

test('opens on the first target of the walk, with the queue beside it', async ({ page }) => {
  await page.goto('/')

  const queue = page.getByRole('navigation', { name: 'Review queue' })
  await expect(queue.getByRole('heading').first()).toContainText('Unapproved')
  await expect(page.getByRole('heading', { level: 2 })).toBeVisible()
  await expect(page.getByText(/^Target 1 of \d+/)).toBeVisible()
  await expect(page.getByText(/^established \d+\/17$/)).toBeVisible()
})

test('the whole local view is on screen without any interaction', async ({ page }) => {
  await page.goto('/#/review/detumble_ok')

  const contract = page.getByRole('region', { name: 'Contract content' })
  await expect(contract).toBeVisible()
  // Every child of the target is listed with its description, and nothing is
  // tucked behind a scroll box of its own.
  await expect(contract.getByRole('listitem')).toHaveCount(3)
  const clipped = await contract.evaluate((element) =>
    [element, ...element.querySelectorAll('*')].some(
      (part) =>
        (part.scrollHeight > part.clientHeight + 1 && getComputedStyle(part).overflowY !== 'visible') ||
        (part.scrollWidth > part.clientWidth + 1 && getComputedStyle(part).overflowX !== 'visible'),
    ),
  )
  expect(clipped).toBe(false)
})

test('skipping walks the queue in order, and a child opens its own local view', async ({ page }) => {
  await page.goto('/')
  const first = await page.getByRole('heading', { level: 2 }).textContent()

  await page.getByRole('link', { name: 'Skip' }).click()

  await expect(page.getByText(/^Target 2 of \d+/)).toBeVisible()
  await expect(page.getByRole('heading', { level: 2 })).not.toHaveText(first ?? '')

  await page.goto('/#/review/detumble_ok')
  await page.getByRole('region', { name: 'Contract content' }).getByRole('link', { name: 'mtq_authority_ok' }).click()
  await expect(page.getByRole('heading', { level: 2 })).toHaveText('fiat:mtq_authority_ok')
})

test('the source of a description is one control away', async ({ page }) => {
  await page.goto('/#/review/aocs_ok')
  const subject = page.locator('.local__subject')

  await subject.getByRole('button', { name: 'Source' }).click()

  await expect(subject.locator('pre')).toBeVisible()
  await expect(subject.getByRole('button', { name: 'Rendered' })).toBeVisible()
})

test('nothing on the review surface can change the argument', async ({ page }) => {
  const writes: string[] = []
  page.on('request', (request) => {
    if (request.method() !== 'GET') writes.push(`${request.method()} ${request.url()}`)
  })

  await page.goto('/')
  await page.getByRole('link', { name: 'Skip' }).click()
  await page.getByRole('button', { name: 'Source' }).first().click()

  expect(writes).toEqual([])
  await expect(page.getByRole('button', { name: /approve|decline/i })).toHaveCount(0)
})

test('a newly opened target is read from its top, wherever the last one was scrolled to', async ({ page }) => {
  await page.setViewportSize({ width: 1200, height: 420 })
  await page.goto('/#/review/aocs_ok')
  const pane = page.getByRole('region', { name: 'Review target' })
  await pane.evaluate((element) => element.scrollTo({ top: element.scrollHeight }))
  expect(await pane.evaluate((element) => element.scrollTop)).toBeGreaterThan(0)

  await page.getByRole('navigation', { name: 'Review queue' }).getByRole('link', { name: /safe_mode_ok/ }).click()

  await expect(page.getByRole('heading', { level: 2 })).toHaveText('structure:safe_mode_ok')
  expect(await pane.evaluate((element) => element.scrollTop)).toBe(0)
})

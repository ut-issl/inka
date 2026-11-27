import { expect, test } from '@playwright/test'

/**
 * Recording decisions, against an `inka serve --as e2e` on a throwaway copy of
 * the `aocs` fixture. The tests share that one project and run in order: each
 * decision changes what the next one finds.
 */

test.describe.configure({ mode: 'serial' })

test('the page says who decisions are recorded as', async ({ page }) => {
  await page.goto('/')

  await expect(page.locator('.masthead__reviewer')).toHaveText('recording as e2e')
  await expect(page.getByRole('form', { name: 'Decision' })).toContainText('Recording as e2e')
})

test('the controls sit under the whole local view, and a decline waits for its reason', async ({ page }) => {
  await page.goto('/')

  const view = page.getByRole('article')
  const form = page.getByRole('form', { name: 'Decision' })
  const viewBox = await view.boundingBox()
  const formBox = await form.boundingBox()
  expect(formBox!.y).toBeGreaterThanOrEqual(viewBox!.y + viewBox!.height)

  await expect(form.getByRole('button', { name: 'Decline' })).toBeDisabled()
  await form.getByLabel('Comment').fill('the margin is not shown')
  await expect(form.getByRole('button', { name: 'Decline' })).toBeEnabled()
})

test('approving records the decision: the target leaves the queue and the walk moves on', async ({ page }) => {
  await page.goto('/')
  const target = (await page.getByRole('heading', { level: 2 }).textContent()) ?? ''
  const id = target.split(':')[1] ?? ''
  const before = await page.locator('.queue__target').count()

  await page.getByRole('button', { name: 'Approve' }).click()

  await expect(page.getByRole('status')).toHaveText(`Recorded approve of ${target}.`)
  await expect(page.getByRole('heading', { level: 2 })).not.toHaveText(target)
  await expect(page.locator('.queue__target')).toHaveCount(before - 1)
  await expect(page.locator('.queue__id', { hasText: new RegExp(`^${id}$`) })).toHaveCount(0)

  // The record is in the project, not in the page: a fresh load agrees, and
  // the approved leaf now counts as established.
  await page.reload()
  await expect(page.locator('.queue__target')).toHaveCount(before - 1)
  await page.goto(`/#/review/${id}`)
  await expect(page.locator('.local__header')).toContainText('established')
  await expect(page.getByRole('form', { name: 'Decision' })).toHaveCount(0)
})

test('declining records the veto: the target is listed as declined and takes no further decision', async ({ page }) => {
  await page.goto('/')
  const target = (await page.getByRole('heading', { level: 2 }).textContent()) ?? ''
  const id = target.split(':')[1] ?? ''

  await page.getByLabel('Comment').fill('not shown to converge')
  await page.getByRole('button', { name: 'Decline' }).click()

  await expect(page.getByRole('status')).toHaveText(`Recorded decline of ${target}.`)
  const queue = page.getByRole('navigation', { name: 'Review queue' })
  await expect(queue.getByRole('heading', { name: 'Declined (1)' })).toBeVisible()

  await page.goto(`/#/review/${id}`)
  await expect(page.locator('.local__details')).toContainText('declined by e2e: not shown to converge')
  await expect(page.getByRole('form', { name: 'Decision' })).toHaveCount(0)
})

test('a decision is one append: nothing else is ever sent that could change the project', async ({ page }) => {
  const writes: string[] = []
  page.on('request', (request) => {
    if (request.method() !== 'GET') writes.push(`${request.method()} ${new URL(request.url()).pathname}`)
  })
  await page.goto('/')

  await page.getByRole('link', { name: 'Skip' }).click()
  await page.getByRole('button', { name: 'Approve' }).click()
  await expect(page.getByRole('status')).toBeVisible()

  expect(writes).toEqual(['POST /api/decisions'])
})

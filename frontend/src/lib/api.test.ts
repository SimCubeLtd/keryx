import { afterEach, expect, test, vi } from 'vite-plus/test'
import { ApiError, request } from './api'

afterEach(() => vi.unstubAllGlobals())

test.each([401, 403, 502])('non-JSON HTTP %s errors retain their status', async (status) => {
  vi.stubGlobal(
    'fetch',
    vi.fn().mockResolvedValue(new Response('<html>Proxy error</html>', { status })),
  )
  await expect(request('/test')).rejects.toMatchObject({
    status,
    message: `Request failed with HTTP ${status}`,
  })
  await expect(request('/test')).rejects.toBeInstanceOf(ApiError)
})

test('JSON API errors keep their message and successful JSON decoding stays strict', async () => {
  const fetch = vi
    .fn()
    .mockResolvedValueOnce(new Response(JSON.stringify({ error: 'Not allowed' }), { status: 403 }))
    .mockResolvedValueOnce(new Response('invalid JSON', { status: 200 }))
  vi.stubGlobal('fetch', fetch)
  await expect(request('/test')).rejects.toMatchObject({ status: 403, message: 'Not allowed' })
  await expect(request('/test')).rejects.toBeInstanceOf(SyntaxError)
})

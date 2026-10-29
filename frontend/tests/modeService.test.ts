import { beforeEach, describe, expect, it, vi } from 'vitest';
import { ModeService } from '@/services/modeService';

describe('ModeService', () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    fetchMock.mockReset();
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      statusText: 'OK',
      json: async () => ({}),
    });
    vi.stubGlobal('fetch', fetchMock);
  });

  it('sends idle when the light is turned off, never off', async () => {
    await ModeService.getInstance().sendColor(null);

    const [url, options] = fetchMock.mock.calls[0];
    expect(url).toBe('/api/mode/animation');
    expect(JSON.parse(options.body).type).toBe('idle');
  });

  it('sends a colour with its value', async () => {
    await ModeService.getInstance().sendColor('#ff0000');

    const [, options] = fetchMock.mock.calls[0];
    expect(JSON.parse(options.body)).toMatchObject({
      type: 'color',
      value: '#ff0000',
    });
  });

  it('reads the arming flag', async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      statusText: 'OK',
      json: async () => ({ enabled: false }),
    });

    expect(await ModeService.getInstance().getAlarmsEnabled()).toBe(false);
  });
});

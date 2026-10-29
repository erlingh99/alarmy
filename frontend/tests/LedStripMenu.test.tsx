import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { LedStripMenu } from '@/components/LedStripMenu';
import { ModeService } from '@/services/modeService';

function renderMenu() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <LedStripMenu />
    </QueryClientProvider>
  );
}

describe('LedStripMenu', () => {
  it('turns the light off with idle, leaving alarms armed', async () => {
    const sendColor = vi
      .spyOn(ModeService.getInstance(), 'sendColor')
      .mockResolvedValue();

    renderMenu();
    await userEvent.click(screen.getByRole('button', { name: /open menu/i }));
    await userEvent.click(await screen.findByText('Turn Off Light'));

    expect(sendColor).toHaveBeenCalledWith(null);
  });

  it('offers the rainbow animation', async () => {
    const rainbow = vi
      .spyOn(ModeService.getInstance(), 'sendRainbowAnimation')
      .mockResolvedValue();

    renderMenu();
    await userEvent.click(screen.getByRole('button', { name: /open menu/i }));
    await userEvent.click(await screen.findByText('Rainbow Animation'));

    expect(rainbow).toHaveBeenCalled();
  });
});

import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { AlarmsDisabledBanner } from '@/components/AlarmsDisabledBanner';
import { ModeService } from '@/services/modeService';

function renderBanner() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <QueryClientProvider client={client}>
      <AlarmsDisabledBanner />
    </QueryClientProvider>
  );
}

describe('AlarmsDisabledBanner', () => {
  it('stays hidden while alarms are enabled', async () => {
    vi.spyOn(ModeService.getInstance(), 'getAlarmsEnabled').mockResolvedValue(true);

    renderBanner();

    await waitFor(() => {
      expect(screen.queryByText(/alarms are disabled/i)).not.toBeInTheDocument();
    });
  });

  it('warns when alarms are disabled', async () => {
    vi.spyOn(ModeService.getInstance(), 'getAlarmsEnabled').mockResolvedValue(false);

    renderBanner();

    expect(await screen.findByText(/alarms are disabled/i)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /turn on/i })).toBeInTheDocument();
  });
});

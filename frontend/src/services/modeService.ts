import { API_MODE_URL } from "@/config";
import { apiClient } from "@/lib/apiClient";

export type AnimationType = 'idle' | 'color' | 'fire' | 'rainbow';

export interface AlarmToggleEvent {
  id: number;
  enabled: boolean;
  changed_at: string;
}

export class ModeService {
  private static instance: ModeService;

  private constructor() {}

  static getInstance(): ModeService {
    if (!ModeService.instance) {
      ModeService.instance = new ModeService();
    }
    return ModeService.instance;
  }

  async setAnimation(type: AnimationType, value?: string): Promise<void> {
    await apiClient.post(`${API_MODE_URL}/animation`, { type, value });
  }

  /** Turning the light off means idle — it must not disarm alarms. */
  async sendColor(color: string | null): Promise<void> {
    if (color === null) {
      await this.setAnimation('idle');
      return;
    }
    await this.setAnimation('color', color);
  }

  async sendFireAnimation(): Promise<void> {
    await this.setAnimation('fire');
  }

  async sendRainbowAnimation(): Promise<void> {
    await this.setAnimation('rainbow');
  }

  async getAlarmsEnabled(): Promise<boolean> {
    const { enabled } = await apiClient.get<{ enabled: boolean }>(
      `${API_MODE_URL}/alarms`
    );
    return enabled;
  }

  async setAlarmsEnabled(enabled: boolean): Promise<boolean> {
    const { enabled: confirmed } = await apiClient.put<{ enabled: boolean }>(
      `${API_MODE_URL}/alarms`,
      { enabled }
    );
    return confirmed;
  }

  async getToggleHistory(limit = 50): Promise<AlarmToggleEvent[]> {
    return apiClient.get<AlarmToggleEvent[]>(
      `${API_MODE_URL}/alarms/history?limit=${limit}`
    );
  }
}

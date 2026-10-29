
import { Alarm, RecurrencePattern, IntensityCurve } from "@/types/alarm";
import { API_BASE_URL } from "@/config";
import { INITIAL_ALARM_LENGTH, INITIAL_START_INTENSITY, INITIAL_END_INTENSITY, INITIAL_INTENSITY_TYPE, INITIAL_PLAY_MELODY } from "@/consts";
import { apiClient } from "@/lib/apiClient";

export const alarmService = {
  getAlarms: async (): Promise<Alarm[]> => {
    return apiClient.get<Alarm[]>(API_BASE_URL);
  },

  getAlarm: async (id: string): Promise<Alarm> => {
    return apiClient.get<Alarm>(`${API_BASE_URL}/${id}`);
  },

  createAlarm: async (
    name: string,
    time: string,
    recurrence: RecurrencePattern,
    color: string,
    length: number = INITIAL_ALARM_LENGTH,
    intensity_curve: IntensityCurve = {
      start_intensity: INITIAL_START_INTENSITY,
      end_intensity: INITIAL_END_INTENSITY,
      curve: INITIAL_INTENSITY_TYPE
    },
    play_melody: boolean = INITIAL_PLAY_MELODY
  ): Promise<Alarm> => {
    const newAlarm = {
      name,
      time,
      color,
      length,
      intensity_curve,
      is_active: true,
      recurrence,
      play_melody,
    };

    return apiClient.put<Alarm>(API_BASE_URL, newAlarm);
  },

  updateAlarm: async (id: string, updates: Partial<Alarm>): Promise<Alarm> => {
    return apiClient.patch<Alarm>(`${API_BASE_URL}/${id}`, updates);
  },

  deleteAlarm: async (id: string): Promise<void> => {
    await apiClient.delete(`${API_BASE_URL}/${id}`);
  },

  restoreAlarm: async (alarm: Alarm): Promise<Alarm> => {
    const { name, time, recurrence, color, length, intensity_curve, play_melody } = alarm;
    return alarmService.createAlarm(name, time, recurrence, color, length, intensity_curve, play_melody);
  },
};

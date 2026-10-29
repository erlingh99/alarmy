import { API_LOG_URL } from "@/config";

export interface LogEntry {
  id: number;
  message: string;
  timestamp: string;
}

export class LogService {
  private static instance: LogService;

  public static getInstance(): LogService {
    if (!LogService.instance) {
      LogService.instance = new LogService();
    }
    return LogService.instance;
  }

  public async getLogs(max?: number): Promise<LogEntry[]> {
    const url = max ? `${API_LOG_URL}?max=${max}` : API_LOG_URL;
    const response = await fetch(url);
    if (!response.ok) {
      throw new Error('Failed to fetch logs');
    }
    return response.json();
  }
}

import { API_FIRMWARE_URL } from "@/config";

export interface FirmwareVersion {
  major: number;
  minor: number;
  patch: number;
}

export class FirmwareService {
  private static instance: FirmwareService;

  public static getInstance(): FirmwareService {
    if (!FirmwareService.instance) {
      FirmwareService.instance = new FirmwareService();
    }
    return FirmwareService.instance;
  }

  public async getBackendVersion(): Promise<FirmwareVersion> {
    const response = await fetch(`${API_FIRMWARE_URL}/version`);
    if (!response.ok) {
      throw new Error("Failed to fetch firmware version");
    }
    return response.json();
  }

  public async getDeviceVersion(): Promise<FirmwareVersion> {
    const response = await fetch(`${API_FIRMWARE_URL}/device-version`);
    if (!response.ok) {
      throw new Error("Failed to fetch device version");
    }
    return response.json();
  }
}

export function formatVersion(v: FirmwareVersion): string {
  return `${v.major}.${v.minor}.${v.patch}`;
}
